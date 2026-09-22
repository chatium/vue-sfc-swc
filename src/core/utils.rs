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

/// `isOn` — `/^on[^a-z]/`
pub fn is_on(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 2 && &b[..2] == b"on" && !(b[2] as char).is_ascii_lowercase()
}

/// `isReservedProp`
pub fn is_reserved_prop(name: &str) -> bool {
    matches!(
        name,
        "" | "key"
            | "ref"
            | "ref_for"
            | "ref_key"
            | "onVnodeBeforeMount"
            | "onVnodeMounted"
            | "onVnodeBeforeUpdate"
            | "onVnodeUpdated"
            | "onVnodeBeforeUnmount"
            | "onVnodeUnmounted"
    )
}

/// `isBuiltInDirective`
pub fn is_built_in_directive(name: &str) -> bool {
    matches!(
        name,
        "bind"
            | "cloak"
            | "else-if"
            | "else"
            | "for"
            | "html"
            | "if"
            | "model"
            | "on"
            | "once"
            | "pre"
            | "show"
            | "slot"
            | "text"
            | "memo"
    )
}

/// `toHandlerKey`
pub fn to_handler_key(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("on{}", crate::core::transform::capitalize(s))
    }
}

pub fn get_exp_source(a: &Arena, exp: NodeId) -> String {
    match a.node(exp) {
        Node::SimpleExpression(e) => e.content.clone(),
        other => other.loc().source.clone(),
    }
}

/// `hasScopeRef`
pub fn has_scope_ref(a: &Arena, node: Option<NodeId>, ids: &std::collections::HashMap<String, i32>) -> bool {
    let node = match node {
        Some(n) => n,
        None => return false,
    };
    if ids.is_empty() {
        return false;
    }
    match a.node(node) {
        Node::Element(e) => {
            for p in &e.props {
                if let Node::Directive(d) = a.node(*p) {
                    if has_scope_ref(a, d.arg, ids) || has_scope_ref(a, d.exp, ids) {
                        return true;
                    }
                }
            }
            e.children.iter().any(|c| has_scope_ref(a, Some(*c), ids))
        }
        Node::For(f) => {
            has_scope_ref(a, Some(f.source), ids)
                || f.children.iter().any(|c| has_scope_ref(a, Some(*c), ids))
        }
        Node::If(i) => i.branches.iter().any(|b| has_scope_ref(a, Some(*b), ids)),
        Node::IfBranch(b) => {
            has_scope_ref(a, b.condition, ids)
                || b.children.iter().any(|c| has_scope_ref(a, Some(*c), ids))
        }
        Node::SimpleExpression(e) => {
            !e.is_static
                && is_simple_identifier(&e.content)
                && ids.get(&e.content).copied().unwrap_or(0) > 0
        }
        Node::CompoundExpression(c) => c.children.iter().any(|ch| {
            !matches!(a.node(*ch), Node::Str(_) | Node::Sym(_))
                && has_scope_ref(a, Some(*ch), ids)
        }),
        Node::Interpolation(i) => has_scope_ref(a, Some(i.content), ids),
        Node::TextCall(t) => has_scope_ref(a, Some(t.content), ids),
        _ => false,
    }
}

/// `getMemoedVNodeCall`
pub fn get_memoed_vnode_call(a: &Arena, node: NodeId) -> NodeId {
    if let Node::CallExpression(c) = a.node(node) {
        if a.sym_of(c.callee) == Some(RuntimeHelper::WITH_MEMO) {
            if let Node::FunctionExpression(f) = a.node(c.arguments[1]) {
                if let Some(r) = f.returns {
                    return r;
                }
            }
        }
    }
    node
}

/// `injectProp`
pub fn inject_prop(a: &mut Arena, node: NodeId, prop: NodeId, merge_props_helper: NodeId) -> bool {
    // returns false when the caller must not register the MERGE_PROPS helper
    let is_vnode = a.is(node, NodeType::VNodeCall);
    let mut props: Option<NodeId> = if is_vnode {
        a.vnode(node).props
    } else {
        a.call(node).arguments.get(2).copied()
    };
    // `'{}'` placeholder counts as "no props"
    if let Some(p) = props {
        if a.str_of(p).is_some() {
            props = Some(p);
        }
    }
    let mut call_path: Vec<NodeId> = Vec::new();
    let mut parent_call: Option<NodeId> = None;
    if let Some(p) = props {
        if a.is(p, NodeType::JsCallExpression) {
            let (resolved, path) = get_unnormalized_props(a, p);
            props = Some(resolved);
            call_path = path;
            parent_call = call_path.last().copied();
        }
    }

    let mut used_merge = false;
    let props_with_injection: NodeId;
    match props {
        None => {
            props_with_injection = a.create_object_expression(vec![prop]);
        }
        Some(p) if a.str_of(p).is_some() => {
            props_with_injection = a.create_object_expression(vec![prop]);
        }
        Some(p) if a.is(p, NodeType::JsCallExpression) => {
            let first = a.call(p).arguments.first().copied();
            let first_is_object = first
                .map(|f| a.is(f, NodeType::JsObjectExpression))
                .unwrap_or(false);
            if first_is_object {
                let first = first.unwrap();
                if !has_prop(a, prop, first) {
                    a.obj_mut(first).properties.insert(0, prop);
                }
                props_with_injection = p;
            } else if a.sym_of(a.call(p).callee) == Some(RuntimeHelper::TO_HANDLERS) {
                let obj = a.create_object_expression(vec![prop]);
                props_with_injection = a.create_call_expression(merge_props_helper, vec![obj, p]);
                used_merge = true;
            } else {
                let obj = a.create_object_expression(vec![prop]);
                a.call_mut(p).arguments.insert(0, obj);
                props_with_injection = p;
            }
        }
        Some(p) if a.is(p, NodeType::JsObjectExpression) => {
            if !has_prop(a, prop, p) {
                a.obj_mut(p).properties.insert(0, prop);
            }
            props_with_injection = p;
        }
        Some(p) => {
            let obj = a.create_object_expression(vec![prop]);
            props_with_injection = a.create_call_expression(merge_props_helper, vec![obj, p]);
            used_merge = true;
            if let Some(pc) = parent_call {
                if a.sym_of(a.call(pc).callee) == Some(RuntimeHelper::GUARD_REACTIVE_PROPS) {
                    parent_call = if call_path.len() >= 2 {
                        Some(call_path[call_path.len() - 2])
                    } else {
                        None
                    };
                }
            }
        }
    }

    match parent_call {
        Some(pc) => {
            a.call_mut(pc).arguments[0] = props_with_injection;
        }
        None => {
            if is_vnode {
                a.vnode_mut(node).props = Some(props_with_injection);
            } else {
                let args = &mut a.call_mut(node).arguments;
                while args.len() < 3 {
                    args.push(0);
                }
                args[2] = props_with_injection;
            }
        }
    }
    used_merge
}

fn get_unnormalized_props(a: &Arena, props: NodeId) -> (NodeId, Vec<NodeId>) {
    let mut call_path = Vec::new();
    let mut current = props;
    loop {
        if !a.is(current, NodeType::JsCallExpression) {
            break;
        }
        let callee = a.call(current).callee;
        match a.sym_of(callee) {
            Some(RuntimeHelper::NORMALIZE_PROPS) | Some(RuntimeHelper::GUARD_REACTIVE_PROPS) => {
                call_path.push(current);
                current = a.call(current).arguments[0];
            }
            _ => break,
        }
    }
    (current, call_path)
}

fn has_prop(a: &Arena, prop: NodeId, props: NodeId) -> bool {
    let key = a.prop(prop).key;
    if let Node::SimpleExpression(k) = a.node(key) {
        let name = &k.content;
        return a.obj(props).properties.iter().any(|p| {
            let pk = a.prop(*p).key;
            matches!(a.node(pk), Node::SimpleExpression(e) if &e.content == name)
        });
    }
    false
}

pub fn unwrap_ts_node(e: &swc_core::ecma::ast::Expr) -> &swc_core::ecma::ast::Expr {
    use swc_core::ecma::ast::Expr;
    match e {
        Expr::TsAs(t) => unwrap_ts_node(&t.expr),
        // Babel spells `x as const` as a TSAsExpression
        Expr::TsConstAssertion(t) => unwrap_ts_node(&t.expr),
        Expr::TsTypeAssertion(t) => unwrap_ts_node(&t.expr),
        Expr::TsNonNull(t) => unwrap_ts_node(&t.expr),
        Expr::TsInstantiation(t) => unwrap_ts_node(&t.expr),
        Expr::TsSatisfies(t) => unwrap_ts_node(&t.expr),
        other => other,
    }
}

/// `isMemberExpression` (non-browser variant)
pub fn is_member_expression(a: &Arena, exp: NodeId) -> bool {
    use swc_core::ecma::ast::Expr;
    let parsed: Option<Expr> = match a.node(exp) {
        Node::SimpleExpression(e) => match &e.ast {
            ExpAst::Expr(x) => Some((**x).clone()),
            _ => crate::core::jsparse::parse_expression(&e.content, true).ok(),
        },
        Node::CompoundExpression(c) => match &c.ast {
            ExpAst::Expr(x) => Some((**x).clone()),
            _ => crate::core::jsparse::parse_expression(&c.loc.source, true).ok(),
        },
        _ => None,
    };
    match parsed {
        Some(e) => match unwrap_ts_node(&e) {
            Expr::Member(_) => true,
            // Babel's `OptionalMemberExpression`; an optional *call* is not one
            Expr::OptChain(o) => matches!(
                &*o.base,
                swc_core::ecma::ast::OptChainBase::Member(_)
            ),
            Expr::Ident(i) => i.sym != *"undefined",
            _ => false,
        },
        None => false,
    }
}

/// `isFnExpression` (non-browser variant)
pub fn is_fn_expression(a: &Arena, exp: NodeId) -> bool {
    use swc_core::ecma::ast::{Expr, Program, Stmt};
    let src = get_exp_source(a, exp);
    let from_ast: Option<Expr> = match a.node(exp) {
        Node::SimpleExpression(e) => match &e.ast {
            ExpAst::Expr(x) => Some((**x).clone()),
            ExpAst::Program(p) => program_first_expr(p),
            _ => None,
        },
        Node::CompoundExpression(c) => match &c.ast {
            ExpAst::Expr(x) => Some((**x).clone()),
            ExpAst::Program(p) => program_first_expr(p),
            _ => None,
        },
        _ => None,
    };
    let parsed = match from_ast {
        Some(e) => Some(e),
        None => crate::core::jsparse::parse_expression(&src, true).ok(),
    };
    fn program_first_expr(p: &Program) -> Option<Expr> {
        let stmts: &[Stmt] = match p {
            Program::Script(s) => &s.body,
            Program::Module(_) => return None,
        };
        match stmts.first() {
            Some(Stmt::Expr(e)) => Some((*e.expr).clone()),
            _ => None,
        }
    }
    match parsed {
        Some(e) => matches!(unwrap_ts_node(&e), Expr::Fn(_) | Expr::Arrow(_)),
        None => false,
    }
}
