//! Port of `compiler-core/src/utils.ts` (grown as the transforms need it).

use std::sync::LazyLock;

use regex::Regex;

use super::ast::*;

pub fn is_core_component(tag: &str) -> Option<RuntimeHelper> {
    match tag {
        "Teleport" | "teleport" => Some(RuntimeHelper::TELEPORT),
        "Suspense" | "suspense" => Some(RuntimeHelper::SUSPENSE),
        "KeepAlive" | "keep-alive" => Some(RuntimeHelper::KEEP_ALIVE),
        "BaseTransition" | "base-transition" => Some(RuntimeHelper::BASE_TRANSITION),
        _ => None,
    }
}

/// `!/^$|^\d|[^\$\w\xA0-￿]/.test(name)`
pub fn is_simple_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.chars().next().unwrap().is_ascii_digit() {
        return false;
    }
    name.chars()
        .all(|c| c == '$' || c == '_' || c.is_ascii_alphanumeric() || (c as u32) >= 0xA0)
}

pub fn is_static_exp(a: &Arena, p: NodeId) -> bool {
    matches!(a.node(p), Node::SimpleExpression(e) if e.is_static)
}

pub fn is_static_arg_of(a: &Arena, arg: &Option<NodeId>, name: &str) -> bool {
    match arg {
        Some(id) => matches!(a.node(*id), Node::SimpleExpression(e) if e.is_static && e.content == name),
        None => false,
    }
}

pub fn is_text_node(a: &Arena, node: NodeId) -> bool {
    matches!(a.node(node), Node::Interpolation(_) | Node::Text(_))
}

pub fn is_v_pre(a: &Arena, p: NodeId) -> bool {
    matches!(a.node(p), Node::Directive(d) if d.name == "pre")
}

pub fn is_v_slot(a: &Arena, p: NodeId) -> bool {
    matches!(a.node(p), Node::Directive(d) if d.name == "slot")
}

pub fn is_template_node(a: &Arena, node: NodeId) -> bool {
    matches!(a.node(node), Node::Element(e) if e.tag_type == ElementType::Template)
}

pub fn is_slot_outlet(a: &Arena, node: NodeId) -> bool {
    matches!(a.node(node), Node::Element(e) if e.tag_type == ElementType::Slot)
}

pub fn is_all_whitespace(s: &str) -> bool {
    s.chars().all(|c| super::parser::is_whitespace(c as u32))
}

pub fn is_whitespace_text(a: &Arena, node: NodeId) -> bool {
    match a.node(node) {
        Node::Text(t) => is_all_whitespace(&t.content),
        Node::TextCall(t) => is_whitespace_text(a, t.content),
        _ => false,
    }
}

pub fn is_comment_or_whitespace(a: &Arena, node: NodeId) -> bool {
    matches!(a.node(node), Node::Comment(_)) || is_whitespace_text(a, node)
}

pub fn find_dir(a: &Arena, node: NodeId, name: &str, allow_empty: bool) -> Option<NodeId> {
    find_dir_matching(a, node, |n| n == name, allow_empty)
}

pub fn find_dir_matching(
    a: &Arena,
    node: NodeId,
    pred: impl Fn(&str) -> bool,
    allow_empty: bool,
) -> Option<NodeId> {
    a.el(node)
        .props
        .iter()
        .copied()
        .find(|p| {
            matches!(a.node(*p), Node::Directive(d) if (allow_empty || d.exp.is_some()) && pred(&d.name))
        })
}

pub fn find_prop(
    a: &Arena,
    node: NodeId,
    name: &str,
    dynamic_only: bool,
    allow_empty: bool,
) -> Option<NodeId> {
    for p in &a.el(node).props {
        match a.node(*p) {
            Node::Attribute(attr) => {
                if dynamic_only {
                    continue;
                }
                if attr.name == name && (attr.value.is_some() || allow_empty) {
                    return Some(*p);
                }
            }
            Node::Directive(d) => {
                if d.name == "bind"
                    && (d.exp.is_some() || allow_empty)
                    && is_static_arg_of(a, &d.arg, name)
                {
                    return Some(*p);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn has_dynamic_key_v_bind(a: &Arena, node: NodeId) -> bool {
    a.el(node).props.iter().any(|p| match a.node(*p) {
        Node::Directive(d) => {
            d.name == "bind"
                && match d.arg {
                    None => true,
                    Some(arg) => !matches!(a.node(arg), Node::SimpleExpression(e) if e.is_static),
                }
        }
        _ => false,
    })
}

pub fn to_valid_asset_id(name: &str, kind: &str) -> String {
    // `name.replace(/[^\w]/g, (c, i) => c === '-' ? '_' : name.charCodeAt(i).toString())`
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut out = String::new();
    for (i, u) in units.iter().enumerate() {
        let c = char::from_u32(*u as u32).unwrap_or('\u{fffd}');
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if c == '-' {
            out.push('_');
        } else {
            out.push_str(&units[i].to_string());
        }
    }
    format!("_{kind}_{out}")
}

static FOR_ALIAS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)([\s\S]*?)\s+(?:in|of)\s+(\S[\s\S]*)").unwrap());
static FOR_ITERATOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s),([^,\}\]]*)(?:,([^,\}\]]*))?$").unwrap());

pub fn match_for_alias(exp: &str) -> Option<(String, String)> {
    let c = FOR_ALIAS_RE.captures(exp)?;
    Some((c[1].to_string(), c[2].to_string()))
}

pub fn match_for_iterator(value: &str) -> Option<(String, Option<String>)> {
    let c = FOR_ITERATOR_RE.captures(value)?;
    Some((
        c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default(),
        c.get(2).map(|m| m.as_str().to_string()),
    ))
}

pub fn strip_for_iterator(value: &str) -> String {
    FOR_ITERATOR_RE.replace(value, "").to_string()
}

/// `advancePositionWithClone`
pub fn advance_position_with_clone(pos: &Position, source: &str, n: Option<usize>) -> Position {
    let units: Vec<u16> = source.encode_utf16().collect();
    let n = n.unwrap_or(units.len());
    let mut lines_count = 0usize;
    let mut last_newline_pos: i64 = -1;
    for (i, u) in units.iter().enumerate().take(n) {
        if *u == 10 {
            lines_count += 1;
            last_newline_pos = i as i64;
        }
    }
    Position {
        offset: pos.offset + n as i64,
        line: pos.line + lines_count as i64,
        column: if last_newline_pos == -1 {
            pos.column + n as i64
        } else {
            n as i64 - last_newline_pos
        },
    }
}
