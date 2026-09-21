//! Port of `compiler-sfc/src/style/cssVars.ts` (the parts the SFC pipeline needs).

use std::sync::LazyLock;

use regex::Regex;

use super::parse::SfcDescriptor;

static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)/\*(.*?)\*/|//[^\n\r]*").unwrap());
static V_BIND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"v-bind\s*\(").unwrap());

fn normalize_expression(exp: &str) -> String {
    let exp = exp.trim();
    let bytes: Vec<char> = exp.chars().collect();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == '\'' && last == '\'') || (first == '"' && last == '"') {
            return bytes[1..bytes.len() - 1].iter().collect();
        }
    }
    exp.to_string()
}

enum LexerState {
    InParens,
    InSingleQuote,
    InDoubleQuote,
}

/// Returns the char index (UTF-16 space is irrelevant here: we work on the
/// same `&str` we slice from) of the closing paren.
fn lex_binding(content: &[char], start: usize) -> Option<usize> {
    let mut state = LexerState::InParens;
    let mut paren_depth = 0i32;
    for (i, ch) in content.iter().enumerate().skip(start) {
        match state {
            LexerState::InParens => {
                if *ch == '\'' {
                    state = LexerState::InSingleQuote;
                } else if *ch == '"' {
                    state = LexerState::InDoubleQuote;
                } else if *ch == '(' {
                    paren_depth += 1;
                } else if *ch == ')' {
                    if paren_depth > 0 {
                        paren_depth -= 1;
                    } else {
                        return Some(i);
                    }
                }
            }
            LexerState::InSingleQuote => {
                if *ch == '\'' {
                    state = LexerState::InParens;
                }
            }
            LexerState::InDoubleQuote => {
                if *ch == '"' {
                    state = LexerState::InParens;
                }
            }
        }
    }
    None
}

pub fn parse_css_vars(sfc: &SfcDescriptor) -> Vec<String> {
    let mut vars: Vec<String> = Vec::new();
    for style in &sfc.styles {
        let content = COMMENT_RE.replace_all(&style.content, "").to_string();
        let chars: Vec<char> = content.chars().collect();
        let mut search_from = 0usize;
        // byte offsets from regex -> char offsets
        let byte_to_char = |b: usize| content[..b].chars().count();
        while let Some(m) = V_BIND_RE.find_at(&content, search_from) {
            let start = byte_to_char(m.end());
            search_from = m.end();
            if let Some(end) = lex_binding(&chars, start) {
                let raw: String = chars[start..end].iter().collect();
                let variable = normalize_expression(&raw);
                if !vars.contains(&variable) {
                    vars.push(variable);
                }
            }
        }
    }
    vars
}

/// `getEscapedCssVarName` + `genVarName` (dev mode only — `isProd` hashing is
/// not used by the SFC pipeline we target).
pub fn get_escaped_css_var_name(key: &str, double_escape: bool) -> String {
    const SYMBOLS: &str = " !\"#$%&'()*+,./:;<=>?@[\\]^`{|}~";
    let mut out = String::new();
    for c in key.chars() {
        if SYMBOLS.contains(c) {
            if double_escape {
                if c == '"' {
                    out.push_str("\\\\\\\"");
                } else {
                    out.push_str("\\\\");
                    out.push(c);
                }
            } else {
                out.push('\\');
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn gen_var_name(id: &str, raw: &str, is_ssr: bool) -> String {
    format!("{id}-{}", get_escaped_css_var_name(raw, is_ssr))
}

pub fn gen_css_vars_from_list(vars: &[String], id: &str, is_ssr: bool) -> String {
    let body: Vec<String> = vars
        .iter()
        .map(|key| {
            format!(
                "\"{}{}\": ({})",
                if is_ssr { ":--" } else { "" },
                gen_var_name(id, key, is_ssr),
                key
            )
        })
        .collect();
    format!("{{\n  {}\n}}", body.join(",\n  "))
}

pub const CSS_VARS_HELPER: &str = "useCssVars";

/// `genCssVarsCode`
pub fn gen_css_vars_code(
    vars: &[String],
    bindings: &crate::core::options::BindingMetadata,
    id: &str,
    _is_prod: bool,
) -> String {
    use crate::core::ast::Arena;
    use crate::core::options::TransformOptions;
    use crate::core::transform::TransformContext;
    use crate::core::transforms::transform_expression::{
        process_expression, stringify_expression,
    };

    let vars_exp = gen_css_vars_from_list(vars, id, false);
    let mut arena = Arena::new();
    let root = arena.create_root(Vec::new(), String::new());
    let mut opts = TransformOptions {
        prefix_identifiers: true,
        inline: true,
        ..Default::default()
    };
    if bindings.is_script_setup != Some(false) {
        opts.binding_metadata = bindings.clone();
    }
    let mut ctx = TransformContext::new(arena, root, opts);
    let exp = ctx.a.simple_exp(vars_exp, false);
    let transformed = process_expression(exp, &mut ctx, false, false, None);
    let s = stringify_expression(&ctx.a, transformed);
    format!("_{CSS_VARS_HELPER}(_ctx => ({s}))")
}

/// `genNormalScriptCssVarsCode`
pub fn gen_normal_script_css_vars_code(
    css_vars: &[String],
    bindings: &crate::core::options::BindingMetadata,
    id: &str,
    is_prod: bool,
    default_var: &str,
) -> String {
    format!(
        "\nimport {{ {CSS_VARS_HELPER} as _{CSS_VARS_HELPER} }} from 'vue'\n\
const __injectCSSVars__ = () => {{\n{}}}\n\
const __setup__ = {default_var}.setup\n\
{default_var}.setup = __setup__\n  \
? (props, ctx) => {{ __injectCSSVars__();return __setup__(props, ctx) }}\n  \
: __injectCSSVars__\n",
        gen_css_vars_code(css_vars, bindings, id, is_prod)
    )
}
