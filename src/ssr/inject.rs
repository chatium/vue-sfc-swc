//! `ssrInjectFallthroughAttrs` and `ssrInjectCssVars`.

use crate::core::ast::*;
use crate::core::transform::TransformContext;
use crate::core::utils::{find_dir, find_dir_matching, is_comment_or_whitespace};

fn filter_child(node: NodeId, ctx: &TransformContext) -> Vec<NodeId> {
    ctx.a
        .list(node)
        .iter()
        .copied()
        .filter(|n| !is_comment_or_whitespace(&ctx.a, *n))
        .collect()
}

fn has_single_child(node: NodeId, ctx: &TransformContext) -> bool {
    filter_child(node, ctx).len() == 1
}

fn push_bind(node: NodeId, exp: &str, ctx: &mut TransformContext) {
    let exp = ctx.a.simple_exp(exp, false);
    let dir = ctx.a.add(Node::Directive(Box::new(DirectiveNode {
        name: "bind".into(),
        raw_name: None,
        arg: None,
        exp: Some(exp),
        modifiers: Vec::new(),
        for_parse_result: None,
        loc: loc_stub(),
    })));
    ctx.a.el_mut(node).props.push(dir);
}

fn is_plain_or_component(node: NodeId, ctx: &TransformContext) -> bool {
    ctx.a.is(node, NodeType::Element)
        && matches!(
            ctx.a.el(node).tag_type,
            ElementType::Element | ElementType::Component
        )
}

fn inject_fallthrough_attrs(node: NodeId, ctx: &mut TransformContext) {
    if is_plain_or_component(node, ctx) && find_dir(&ctx.a, node, "for", false).is_none() {
        push_bind(node, "_attrs", ctx);
    }
}

pub fn ssr_inject_fallthrough_attrs(node: NodeId, ctx: &mut TransformContext) {
    if ctx.a.is(node, NodeType::Root) {
        ctx.identifiers.insert("_attrs".into(), 1);
    }
    if ctx.a.is(node, NodeType::Element)
        && ctx.a.el(node).tag_type == ElementType::Component
        && matches!(
            ctx.a.el(node).tag.as_str(),
            "transition" | "Transition" | "KeepAlive" | "keep-alive"
        )
    {
        let root_children = filter_child(ctx.root, ctx);
        if root_children.len() == 1 && root_children[0] == node {
            if has_single_child(node, ctx) {
                let first = ctx.a.el(node).children[0];
                inject_fallthrough_attrs(first, ctx);
            }
            return;
        }
    }
    let parent = match ctx.parent {
        Some(p) if ctx.a.is(p, NodeType::Root) => p,
        _ => return,
    };
    if ctx.a.is(node, NodeType::IfBranch) && has_single_child(node, ctx) {
        let mut has_encountered_if = false;
        for c in filter_child(parent, ctx) {
            let is_if = ctx.a.is(c, NodeType::If)
                || (ctx.a.is(c, NodeType::Element) && find_dir(&ctx.a, c, "if", false).is_some());
            if is_if {
                if has_encountered_if {
                    return;
                }
                has_encountered_if = true;
            } else {
                let is_else = ctx.a.is(c, NodeType::Element)
                    && find_dir_matching(&ctx.a, c, |n| n.contains("else"), true).is_some();
                if !has_encountered_if || !is_else {
                    return;
                }
            }
        }
        let first = ctx.a.branch(node).children[0];
        inject_fallthrough_attrs(first, ctx);
    } else if has_single_child(parent, ctx) {
        inject_fallthrough_attrs(node, ctx);
    }
}

fn inject_css_vars(node: NodeId, ctx: &mut TransformContext) {
    if !is_plain_or_component(node, ctx) || find_dir(&ctx.a, node, "for", false).is_some() {
        return;
    }
    let tag = ctx.a.el(node).tag.clone();
    if tag == "suspense" || tag == "Suspense" {
        for child in ctx.a.el(node).children.clone() {
            if ctx.a.is(child, NodeType::Element)
                && ctx.a.el(child).tag_type == ElementType::Template
            {
                for c in ctx.a.el(child).children.clone() {
                    inject_css_vars(c, ctx);
                }
            } else {
                inject_css_vars(child, ctx);
            }
        }
    } else {
        push_bind(node, "_cssVars", ctx);
    }
}

pub fn ssr_inject_css_vars(node: NodeId, ctx: &mut TransformContext) {
    if ctx.opts.ssr_css_vars.is_empty() {
        return;
    }
    if ctx.a.is(node, NodeType::Root) {
        ctx.identifiers.insert("_cssVars".into(), 1);
    }
    match ctx.parent {
        Some(p) if ctx.a.is(p, NodeType::Root) => {}
        _ => return,
    }
    if ctx.a.is(node, NodeType::IfBranch) {
        for child in ctx.a.branch(node).children.clone() {
            inject_css_vars(child, ctx);
        }
    } else {
        inject_css_vars(node, ctx);
    }
}
