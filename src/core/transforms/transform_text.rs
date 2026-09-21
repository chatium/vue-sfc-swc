//! Port of `compiler-core/src/transforms/transformText.ts`.

use crate::core::ast::*;
use crate::core::patch_flags as pf;
use crate::core::transform::{ExitFn, TransformContext};
use crate::core::utils::is_text_node;

use super::cache_static::get_constant_type;

pub fn transform_text(node: NodeId, ctx: &mut TransformContext) -> Vec<ExitFn> {
    match ctx.a.node_type(node) {
        NodeType::Root | NodeType::Element | NodeType::For | NodeType::IfBranch => {
            vec![ExitFn::Text { node }]
        }
        _ => Vec::new(),
    }
}

pub fn exit_text(node: NodeId, ctx: &mut TransformContext) {
    let mut current_container: Option<NodeId> = None;
    let mut has_text = false;

    let mut i = 0usize;
    loop {
        let children = ctx.a.children_of(node).clone();
        if i >= children.len() {
            break;
        }
        let child = children[i];
        if is_text_node(&ctx.a, child) {
            has_text = true;
            let mut j = i + 1;
            loop {
                let children = ctx.a.children_of(node).clone();
                if j >= children.len() {
                    break;
                }
                let next = children[j];
                if is_text_node(&ctx.a, next) {
                    if current_container.is_none() {
                        let loc = ctx.a.loc(child).clone();
                        let c = ctx.a.create_compound_expression(vec![child], loc);
                        ctx.a.children_of_mut(node)[i] = c;
                        current_container = Some(c);
                    }
                    let plus = ctx.a.string(" + ");
                    let cc = current_container.unwrap();
                    ctx.a.compound_mut(cc).children.push(plus);
                    ctx.a.compound_mut(cc).children.push(next);
                    ctx.a.children_of_mut(node).remove(j);
                } else {
                    current_container = None;
                    break;
                }
            }
        }
        i += 1;
    }

    let children = ctx.a.children_of(node).clone();
    let single_leave = children.len() == 1
        && (ctx.a.is(node, NodeType::Root)
            || (ctx.a.is(node, NodeType::Element)
                && ctx.a.el(node).tag_type == ElementType::Element
                && !ctx.a.el(node).props.iter().any(|p| {
                    matches!(ctx.a.node(*p), Node::Directive(d)
                        if !ctx.opts.directive_transforms.contains_key(&d.name))
                })));
    if !has_text || single_leave {
        return;
    }

    for i in 0..children.len() {
        let child = ctx.a.children_of(node)[i];
        let is_text = is_text_node(&ctx.a, child);
        let is_compound = ctx.a.is(child, NodeType::CompoundExpression);
        if is_text || is_compound {
            let mut call_args: Vec<NodeId> = Vec::new();
            let is_single_space = matches!(ctx.a.node(child), Node::Text(t) if t.content == " ");
            if !is_single_space {
                call_args.push(child);
            }
            if !ctx.opts.ssr && get_constant_type(child, ctx) == ConstantType::NotConstant {
                let text = format!("{} /* {} */", pf::TEXT, pf::patch_flag_name(pf::TEXT));
                let n = ctx.a.string(text);
                call_args.push(n);
            }
            let helper = ctx.helper_node(RuntimeHelper::CREATE_TEXT);
            let call = ctx.a.create_call_expression(helper, call_args);
            let loc = ctx.a.loc(child).clone();
            let text_call = ctx.a.add(Node::TextCall(Box::new(TextCallNode {
                content: child,
                codegen_node: Some(call),
                loc,
            })));
            ctx.a.children_of_mut(node)[i] = text_call;
        }
    }
}
