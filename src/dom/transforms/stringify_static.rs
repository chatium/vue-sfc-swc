//! Port of `compiler-dom/src/transforms/stringifyStatic.ts`.

use crate::core::ast::*;
use crate::core::js_value::{
    JsValue, eval_constant, normalize_class, stringify_normalized_style, to_display_string,
};
use crate::core::transform::TransformContext;
use crate::core::utils::{find_dir, is_static_arg_of};

use crate::dom::attrs::{
    escape_html, is_boolean_attr, is_known_html_attr, is_known_math_ml_attr, is_known_svg_attr,
};
use crate::dom::tags::is_void_tag;

const ELEMENT_WITH_BINDING_COUNT: usize = 5;
const NODE_COUNT: usize = 20;

fn is_non_stringifiable(tag: &str) -> bool {
    matches!(
        tag,
        "caption" | "thead" | "tr" | "th" | "tbody" | "td" | "tfoot" | "colgroup" | "col"
    )
}

fn is_stringifiable_attr(name: &str, ns: Namespace) -> bool {
    let known = match ns {
        Namespace::Html => is_known_html_attr(name),
        Namespace::Svg => is_known_svg_attr(name),
        Namespace::MathMl => is_known_math_ml_attr(name),
    };
    known || name.starts_with("data-") || name.starts_with("aria-")
}

fn get_cached_node(a: &Arena, node: NodeId) -> Option<NodeId> {
    let codegen = match a.node_type(node) {
        NodeType::Element if a.el(node).tag_type == ElementType::Element => {
            a.el(node).codegen_node
        }
        NodeType::TextCall => a.text_call(node).codegen_node,
        _ => None,
    }?;
    if a.is(codegen, NodeType::JsCacheExpression) {
        Some(codegen)
    } else {
        None
    }
}

pub fn stringify_static(parent: NodeId, ctx: &mut TransformContext) {
    if ctx.scopes.v_slot > 0 {
        return;
    }

    let is_parent_cached = ctx.a.is(parent, NodeType::Element)
        && ctx
            .a
            .el(parent)
            .codegen_node
            .map(|c| {
                ctx.a.is(c, NodeType::VNodeCall)
                    && ctx
                        .a
                        .vnode(c)
                        .children
                        .map(|ch| {
                            !ctx.a.is_list_like(ch) && ctx.a.is(ch, NodeType::JsCacheExpression)
                        })
                        .unwrap_or(false)
            })
            .unwrap_or(false);

    let mut nc = 0usize;
    let mut ec = 0usize;
    let mut current_chunk: Vec<NodeId> = Vec::new();

    let mut i = 0usize;
    loop {
        let len = ctx.a.children_of(parent).len();
        if i >= len {
            break;
        }
        let child = ctx.a.children_of(parent)[i];
        let is_cached = is_parent_cached || get_cached_node(&ctx.a, child).is_some();
        if is_cached {
            if let Some((n, e)) = analyze_node(child, ctx) {
                nc += n;
                ec += e;
                current_chunk.push(child);
                i += 1;
                continue;
            }
        }
        let deleted = stringify_current_chunk(
            parent,
            i,
            &mut current_chunk,
            nc,
            ec,
            is_parent_cached,
            ctx,
        );
        i = i.saturating_sub(deleted);
        nc = 0;
        ec = 0;
        current_chunk.clear();
        i += 1;
    }
    stringify_current_chunk(parent, i, &mut current_chunk, nc, ec, is_parent_cached, ctx);
}

#[allow(clippy::too_many_arguments)]
fn stringify_current_chunk(
    parent: NodeId,
    current_index: usize,
    current_chunk: &mut Vec<NodeId>,
    nc: usize,
    ec: usize,
    is_parent_cached: bool,
    ctx: &mut TransformContext,
) -> usize {
    if !(nc >= NODE_COUNT || ec >= ELEMENT_WITH_BINDING_COUNT) {
        return 0;
    }
    let joined: String = current_chunk
        .iter()
        .map(|n| stringify_node(*n, ctx))
        .collect();
    let literal = replace_exp_markers(&serde_json::to_string(&joined).unwrap());
    let arg0 = ctx.a.string(literal);
    let arg1 = ctx.a.string(current_chunk.len().to_string());
    let helper = ctx.helper_node(RuntimeHelper::CREATE_STATIC);
    let static_call = ctx.a.create_call_expression(helper, vec![arg0, arg1]);

    let delete_count = current_chunk.len() - 1;

    if is_parent_cached {
        let start = current_index - current_chunk.len();
        let list = ctx.a.children_of_mut(parent);
        list.splice(start..start + current_chunk.len(), [static_call]);
    } else {
        let first = current_chunk[0];
        let cache = get_cached_node(&ctx.a, first).expect("cached chunk node");
        ctx.a.cache_mut(cache).value = static_call;
        if current_chunk.len() > 1 {
            let start = current_index - current_chunk.len() + 1;
            let list = ctx.a.children_of_mut(parent);
            list.drain(start..start + delete_count);
            let last = *current_chunk.last().unwrap();
            if let Some(last_cache) = get_cached_node(&ctx.a, last) {
                if let Some(cache_index) =
                    ctx.cached.iter().position(|c| *c == Some(last_cache))
                {
                    for i in cache_index..ctx.cached.len() {
                        if let Some(c) = ctx.cached[i] {
                            ctx.a.cache_mut(c).index -= delete_count;
                        }
                    }
                    let from = cache_index - delete_count + 1;
                    ctx.cached.drain(from..from + delete_count);
                }
            }
        }
    }
    delete_count
}

/// `__VUE_EXP_START__x__VUE_EXP_END__` -> `" + x + "`
fn replace_exp_markers(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("__VUE_EXP_START__") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "__VUE_EXP_START__".len()..];
        match after.find("__VUE_EXP_END__") {
            Some(end) => {
                out.push_str(&format!("\" + {} + \"", &after[..end]));
                rest = &after[end + "__VUE_EXP_END__".len()..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn analyze_node(node: NodeId, ctx: &mut TransformContext) -> Option<(usize, usize)> {
    if ctx.a.is(node, NodeType::Element) && is_non_stringifiable(&ctx.a.el(node).tag) {
        return None;
    }
    if ctx.a.is(node, NodeType::Element) && find_dir(&ctx.a, node, "once", true).is_some() {
        return None;
    }
    if ctx.a.is(node, NodeType::TextCall) {
        return Some((1, 0));
    }

    let mut nc = 1usize;
    let mut ec = usize::from(!ctx.a.el(node).props.is_empty());
    if walk(node, ctx, &mut nc, &mut ec) {
        Some((nc, ec))
    } else {
        None
    }
}

fn walk(node: NodeId, ctx: &mut TransformContext, nc: &mut usize, ec: &mut usize) -> bool {
    let ns = ctx.a.el(node).ns;
    let is_option_tag = ctx.a.el(node).tag == "option" && ns == Namespace::Html;
    let props = ctx.a.el(node).props.clone();
    for p in props {
        match ctx.a.node_type(p) {
            NodeType::Attribute => {
                let name = ctx.a.attr(p).name.clone();
                if !is_stringifiable_attr(&name, ns) {
                    return false;
                }
            }
            NodeType::Directive => {
                let (name, arg, exp) = {
                    let d = ctx.a.dir(p);
                    (d.name.clone(), d.arg, d.exp)
                };
                if name != "bind" {
                    continue;
                }
                if let Some(arg) = arg {
                    if ctx.a.is(arg, NodeType::CompoundExpression) {
                        return false;
                    }
                    let e = ctx.a.exp(arg);
                    if e.is_static && !is_stringifiable_attr(&e.content, ns) {
                        return false;
                    }
                }
                if let Some(exp) = exp {
                    if ctx.a.is(exp, NodeType::CompoundExpression)
                        || ctx.a.exp(exp).const_type < ConstantType::CanStringify
                    {
                        return false;
                    }
                }
                if is_option_tag
                    && is_static_arg_of(&ctx.a, &arg, "value")
                    && exp.map(|e| !ctx.a.exp(e).is_static).unwrap_or(false)
                {
                    return false;
                }
            }
            _ => {}
        }
    }
    let children = ctx.a.el(node).children.clone();
    for child in children {
        *nc += 1;
        if ctx.a.is(child, NodeType::Element) {
            if !ctx.a.el(child).props.is_empty() {
                *ec += 1;
            }
            if !walk(child, ctx, nc, ec) {
                return false;
            }
        }
    }
    true
}

fn stringify_node(node: NodeId, ctx: &TransformContext) -> String {
    match ctx.a.node(node) {
        Node::Str(s) => s.clone(),
        Node::Sym(_) => String::new(),
        Node::Element(_) => stringify_element(node, ctx),
        Node::Text(t) => escape_html(&t.content),
        Node::Comment(c) => format!("<!--{}-->", escape_html(&c.content)),
        Node::Interpolation(i) => {
            let v = evaluate_constant(i.content, ctx);
            escape_html(&to_display_string(&v))
        }
        Node::CompoundExpression(_) => {
            let v = evaluate_constant(node, ctx);
            escape_html(&v.to_js_string())
        }
        Node::TextCall(t) => stringify_node(t.content, ctx),
        _ => String::new(),
    }
}

fn stringify_element(node: NodeId, ctx: &TransformContext) -> String {
    let el = ctx.a.el(node);
    let mut res = format!("<{}", el.tag);
    let mut inner_html = String::new();
    for p in &el.props {
        match ctx.a.node(*p) {
            Node::Attribute(a) => {
                res.push_str(&format!(" {}", a.name));
                if let Some(v) = &a.value {
                    res.push_str(&format!("=\"{}\"", escape_html(&v.content)));
                }
            }
            Node::Directive(d) => {
                if d.name == "bind" {
                    let exp = match d.exp {
                        Some(e) => e,
                        None => continue,
                    };
                    let content = ctx.a.exp(exp).content.clone();
                    let arg_content = d
                        .arg
                        .map(|a| ctx.a.exp(a).content.clone())
                        .unwrap_or_default();
                    if content.starts_with('_') {
                        res.push_str(&format!(
                            " {arg_content}=\"__VUE_EXP_START__{content}__VUE_EXP_END__\""
                        ));
                        continue;
                    }
                    if is_boolean_attr(&arg_content) && content == "false" {
                        continue;
                    }
                    if let Some(mut evaluated) = eval_constant_value(exp, ctx) {
                        if arg_content == "class" {
                            evaluated = JsValue::Str(normalize_class(&evaluated));
                        } else if arg_content == "style" {
                            evaluated = JsValue::Str(stringify_normalized_style(&evaluated));
                        }
                        res.push_str(&format!(
                            " {arg_content}=\"{}\"",
                            escape_html(&evaluated.to_js_string())
                        ));
                    }
                } else if d.name == "html" {
                    if let Some(exp) = d.exp {
                        inner_html = evaluate_constant(exp, ctx).to_js_string();
                    }
                } else if d.name == "text" {
                    if let Some(exp) = d.exp {
                        inner_html = escape_html(&to_display_string(&evaluate_constant(exp, ctx)));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(scope_id) = &ctx.opts.scope_id {
        res.push_str(&format!(" {scope_id}"));
    }
    res.push('>');
    if !inner_html.is_empty() {
        res.push_str(&inner_html);
    } else {
        for child in &ctx.a.el(node).children {
            res.push_str(&stringify_node(*child, ctx));
        }
    }
    if !is_void_tag(&ctx.a.el(node).tag) {
        res.push_str(&format!("</{}>", ctx.a.el(node).tag));
    }
    res
}

fn eval_constant_value(exp: NodeId, ctx: &TransformContext) -> Option<JsValue> {
    match ctx.a.node(exp) {
        Node::SimpleExpression(e) => eval_constant(&e.content),
        _ => Some(evaluate_constant(exp, ctx)),
    }
}

fn evaluate_constant(exp: NodeId, ctx: &TransformContext) -> JsValue {
    match ctx.a.node(exp) {
        Node::SimpleExpression(e) => eval_constant(&e.content).unwrap_or(JsValue::Undefined),
        Node::CompoundExpression(c) => {
            let mut res = String::new();
            for child in &c.children {
                match ctx.a.node(*child) {
                    Node::Str(_) | Node::Sym(_) => continue,
                    Node::Text(t) => res.push_str(&t.content),
                    Node::Interpolation(i) => {
                        res.push_str(&to_display_string(&evaluate_constant(i.content, ctx)))
                    }
                    _ => res.push_str(&evaluate_constant(*child, ctx).to_js_string()),
                }
            }
            JsValue::Str(res)
        }
        _ => JsValue::Undefined,
    }
}
