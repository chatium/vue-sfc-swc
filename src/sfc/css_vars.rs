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
pub fn get_escaped_css_var_name(key: &str, do_double_escape: bool) -> String {
    let mut out = String::new();
    for c in key.chars() {
        if "!\"#$%&'()*+,./:;<=>?@[\\]^`{|}~".contains(c) {
            out.push('\\');
            if do_double_escape {
                out.push('\\');
            }
            out.push(c);
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
