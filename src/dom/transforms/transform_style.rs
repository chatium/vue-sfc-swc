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
    let mut ret = serde_json::Map::new();
    for item in split_style(css) {
        if item.is_empty() {
            continue;
        }
        if let Some(idx) = item.find(':') {
            let key = item[..idx].trim().to_string();
            let value = item[idx + 1..].trim().to_string();
            ret.insert(key, serde_json::Value::String(value));
        }
    }
    ret
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
