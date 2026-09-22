//! Port of `compiler-sfc/src/compileStyle.ts`.

use super::css_vars::{gen_var_name, lex_binding_in};
use super::style::plugin_scoped::scoped_plugin;
use super::style::postcss::node::{CssKind, CssTree};
use super::style::postcss::{parse::parse, stringify::stringify};

#[derive(Debug, Clone, Default)]
pub struct StyleCompileOptions {
    pub source: String,
    pub filename: String,
    pub id: String,
    pub scoped: bool,
    pub trim: Option<bool>,
    pub is_prod: bool,
    pub modules: bool,
    pub preprocess_lang: Option<String>,
}

#[derive(Debug, Default)]
pub struct StyleCompileResult {
    pub code: String,
    pub errors: Vec<StyleError>,
    pub modules: Option<Vec<(String, String)>>,
}

/// A postcss `CssSyntaxError` carries a position; the preprocessor and
/// plugin errors are plain messages.
#[derive(Debug, Clone)]
pub struct StyleError {
    pub msg: String,
    /// 1-based line and column, as postcss reports them
    pub position: Option<(usize, usize)>,
}

impl StyleError {
    fn plain(msg: impl Into<String>) -> Self {
        StyleError {
            msg: msg.into(),
            position: None,
        }
    }

    /// `CssSyntaxError#message`: `<resolved file>:<line>:<column>: <reason>`
    fn syntax(e: &super::style::postcss::parse::CssSyntaxError, css: &str, filename: &str) -> Self {
        let (line, column) = e.line_col(css);
        let file = std::env::current_dir()
            .map(|d| d.join(filename))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| filename.to_string());
        StyleError {
            msg: format!("{file}:{line}:{column}: {}", e.reason),
            position: Some((line, column)),
        }
    }
}

pub fn compile_style(options: StyleCompileOptions) -> StyleCompileResult {
    let trim = options.trim.unwrap_or(true);
    let short_id = options
        .id
        .strip_prefix("data-v-")
        .unwrap_or(&options.id)
        .to_string();
    let long_id = format!("data-v-{short_id}");

    let mut errors = Vec::new();
    let source = match &options.preprocess_lang {
        Some(lang) => match super::style::preprocessors::preprocess_with_filename(
            lang,
            &options.source,
            &options.filename,
        ) {
            Ok(code) => code,
            Err(e) => {
                return StyleCompileResult {
                    code: String::new(),
                    errors: vec![StyleError::plain(e)],
                    modules: None,
                };
            }
        },
        None => options.source.clone(),
    };

    let mut tree = match parse(&source) {
        Ok(t) => t,
        Err(e) => {
            errors.push(StyleError::syntax(&e, &source, &options.filename));
            return StyleCompileResult {
                code: String::new(),
                errors,
                modules: None,
            };
        }
    };

    // plugin order: cssVars, trim, scoped, modules — `Once` hooks first
    if trim {
        trim_plugin(&mut tree);
    }
    css_vars_plugin(&mut tree, &short_id, options.is_prod);
    if options.scoped {
        if let Err(e) = scoped_plugin(&mut tree, &long_id) {
            errors.push(StyleError::plain(e));
            return StyleCompileResult {
                code: String::new(),
                errors,
                modules: None,
            };
        }
    }
    let mut modules = None;
    if options.modules {
        let r = super::style::css_modules::apply(&mut tree, &source);
        if let Some(e) = r.error {
            errors.push(StyleError::plain(e));
            return StyleCompileResult {
                code: String::new(),
                errors,
                modules: None,
            };
        }
        modules = Some(r.exports);
    }

    StyleCompileResult {
        code: stringify(&tree),
        errors,
        modules,
    }
}

/// `pluginTrim`
fn trim_plugin(tree: &mut CssTree) {
    for id in tree.walk_ids(tree.root) {
        let kind = tree.get(id).kind;
        if kind == CssKind::Rule || kind == CssKind::AtRule {
            let raws = &mut tree.get_mut(id).raws;
            if raws.before.as_deref().map(|b| !b.is_empty()).unwrap_or(false) {
                raws.before = Some("\n".to_string());
            }
            if raws.after.as_deref().map(|a| !a.is_empty()).unwrap_or(false) {
                raws.after = Some("\n".to_string());
            }
        }
    }
}

/// `cssVarsPlugin`
fn css_vars_plugin(tree: &mut CssTree, id: &str, is_prod: bool) {
    let _ = is_prod;
    for node in tree.walk_ids(tree.root) {
        if tree.get(node).kind != CssKind::Decl {
            continue;
        }
        let value = tree.get(node).value.clone();
        if !has_v_bind(&value) {
            continue;
        }
        let chars: Vec<char> = value.chars().collect();
        let mut transformed = String::new();
        let mut last_index = 0usize;
        let mut search = 0usize;
        while let Some((m_start, m_end)) = find_v_bind(&chars, search) {
            search = m_end;
            if let Some(end) = lex_binding_in(&chars, m_end) {
                let raw: String = chars[m_end..end].iter().collect();
                let variable = normalize_expression(&raw);
                transformed.push_str(&chars[last_index..m_start].iter().collect::<String>());
                transformed.push_str(&format!("var(--{})", gen_var_name(id, &variable, is_prod, false)));
                last_index = end + 1;
            }
        }
        transformed.push_str(&chars[last_index.min(chars.len())..].iter().collect::<String>());
        tree.get_mut(node).value = transformed;
        tree.get_mut(node).raws.value = None;
    }
}

fn has_v_bind(s: &str) -> bool {
    find_v_bind(&s.chars().collect::<Vec<_>>(), 0).is_some()
}

/// `/v-bind\s*\(/g`
fn find_v_bind(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let pat: Vec<char> = "v-bind".chars().collect();
    let mut i = from;
    while i + pat.len() <= chars.len() {
        if chars[i..i + pat.len()] == pat[..] {
            let mut j = i + pat.len();
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j] == '(' {
                return Some((i, j + 1));
            }
        }
        i += 1;
    }
    None
}

fn normalize_expression(exp: &str) -> String {
    let exp = exp.trim();
    let c: Vec<char> = exp.chars().collect();
    if c.len() >= 2
        && ((c[0] == '\'' && c[c.len() - 1] == '\'') || (c[0] == '"' && c[c.len() - 1] == '"'))
    {
        return c[1..c.len() - 1].iter().collect();
    }
    exp.to_string()
}
