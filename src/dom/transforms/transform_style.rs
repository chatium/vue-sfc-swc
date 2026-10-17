//! Port of `compiler-dom/src/transforms/transformStyle.ts`.

use crate::core::ast::*;
use crate::core::transform::TransformContext;

pub fn transform_style(node: NodeId, ctx: &mut TransformContext) {
    if !ctx.a.is(node, NodeType::Element) {
        return;
    }
    let props = ctx.a.el(node).props.clone();
    for (i, p) in props.iter().enumerate() {
        let (is_style, loc, content) = match ctx.a.node(*p) {
            Node::Attribute(a) if a.name == "style" && a.value.is_some() => (
                true,
                a.loc.clone(),
                a.value.as_ref().unwrap().content.clone(),
            ),
            _ => continue,
        };
        if !is_style {
            continue;
        }
        let normalized = parse_string_style(&content);
        let json = serde_json::to_string(&normalized).unwrap();
        let exp =
            ctx.a
                .create_simple_expression(json, false, loc.clone(), ConstantType::CanStringify);
        let arg = ctx
            .a
            .create_simple_expression("style", true, loc.clone(), ConstantType::NotConstant);
        let dir = ctx.a.add(Node::Directive(Box::new(DirectiveNode {
            name: "bind".to_string(),
            raw_name: None,
            arg: Some(arg),
            exp: Some(exp),
            modifiers: Vec::new(),
            for_parse_result: None,
            loc,
        })));
        ctx.a.el_mut(node).props[i] = dir;
    }
}

/// `parseStringStyle` from `@vue/shared`
fn parse_string_style(css: &str) -> serde_json::Map<String, serde_json::Value> {
    string_style_entries(css)
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect()
}

/// `parseStringStyle`'s entries in object order: comments dropped, split on
/// `listDelimiterRE`, then each item on its first `:` with something after it
/// (`propertyDelimiterRE = /:([^]+)/`); a repeated key keeps its first slot
pub fn string_style_entries(css: &str) -> Vec<(String, String)> {
    let mut ret: Vec<(String, String)> = Vec::new();
    for item in split_style(&strip_style_comments(css)) {
        let Some(idx) = item.find(':').filter(|&i| i + 1 < item.len()) else {
            continue;
        };
        let key = item[..idx].trim().to_string();
        let value = item[idx + 1..].trim().to_string();
        match ret.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => ret.push((key, value)),
        }
    }
    ret
}

/// `cssText.replace(/\/\*[^]*?\*\//g, '')`
fn strip_style_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(open) = rest.find("/*") {
        let Some(close) = rest[open + 2..].find("*/") else {
            break;
        };
        out.push_str(&rest[..open]);
        rest = &rest[open + 2 + close + 2..];
    }
    out.push_str(rest);
    out
}

/// `listDelimiterRE = /;(?![^(]*\))/g`
fn split_style(css: &str) -> Vec<String> {
    let chars: Vec<char> = css.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    for i in 0..chars.len() {
        if chars[i] != ';' {
            continue;
        }
        // negative lookahead: `[^(]*\)`
        let mut j = i + 1;
        let mut lookahead_matches = false;
        while j < chars.len() {
            if chars[j] == '(' {
                break;
            }
            if chars[j] == ')' {
                lookahead_matches = true;
                break;
            }
            j += 1;
        }
        if !lookahead_matches {
            out.push(chars[start..i].iter().collect());
            start = i + 1;
        }
    }
    out.push(chars[start..].iter().collect());
    out
}
