//! `ssrTransformIf`/`For`/`SlotOutlet`, `ssrProcessTeleport`/`Suspense`/
//! `Transition`/`TransitionGroup`.

use crate::core::ast::*;
use crate::core::transform::TransformContext;
use crate::core::transforms::transform_element::{build_props, resolve_component_type};
use crate::core::transforms::transform_slot_outlet::process_slot_outlet;
use crate::core::transforms::v_for::create_for_loop_params;
use crate::core::transforms::v_slot::{SlotFnKind, build_slots};
use crate::core::utils::{find_prop, is_slot_outlet};

use super::codegen::{Parent, SsrCtx, process_children, process_children_as_statement};
use super::component::build_ssr_props;
use super::{WipSuspense, WipTransitionGroup, ssr_error, ssr_helper};

pub fn ssr_process_if(
    node: NodeId,
    sctx: &mut SsrCtx,
    ctx: &mut TransformContext,
    disable_nested_fragments: bool,
    disable_comment: bool,
) {
    let branches = ctx.a.if_node(node).branches.clone();
    let root_consequent =
        process_if_branch(branches[0], sctx, ctx, disable_nested_fragments);
    let test = ctx.a.branch(branches[0]).condition.unwrap();
    let if_statement = ctx.a.add(Node::IfStatement(Box::new(IfStatement {
        test,
        consequent: root_consequent,
        alternate: None,
    })));
    sctx.push_statement(if_statement);

    let mut current_if = if_statement;
    for branch in branches.iter().skip(1) {
        let block = process_if_branch(*branch, sctx, ctx, disable_nested_fragments);
        match ctx.a.branch(*branch).condition {
            Some(cond) => {
                let next = ctx.a.add(Node::IfStatement(Box::new(IfStatement {
                    test: cond,
                    consequent: block,
                    alternate: None,
                })));
                match ctx.a.node_mut(current_if) {
                    Node::IfStatement(i) => i.alternate = Some(next),
                    _ => unreachable!(),
                }
                current_if = next;
            }
            None => match ctx.a.node_mut(current_if) {
                Node::IfStatement(i) => i.alternate = Some(block),
                _ => unreachable!(),
            },
        }
    }

    let has_alternate = match ctx.a.node(current_if) {
        Node::IfStatement(i) => i.alternate.is_some(),
        _ => unreachable!(),
    };
    if !has_alternate && !disable_comment {
        let push = ctx.a.string("_push");
        let arg = ctx.a.string("`<!---->`");
        let call = ctx.a.create_call_expression(push, vec![arg]);
        let block = ctx.a.add(Node::BlockStatement(vec![call]));
        match ctx.a.node_mut(current_if) {
            Node::IfStatement(i) => i.alternate = Some(block),
            _ => unreachable!(),
        }
    }
}

fn process_if_branch(
    branch: NodeId,
    sctx: &SsrCtx,
    ctx: &mut TransformContext,
    disable_nested_fragments: bool,
) -> NodeId {
    let children = ctx.a.branch(branch).children.clone();
    let single_element = children.len() == 1 && ctx.a.is(children[0], NodeType::Element);
    let single_for = children.len() == 1 && ctx.a.is(children[0], NodeType::For);
    let need_fragment_wrapper = !disable_nested_fragments && !single_element && !single_for;
    process_children_as_statement(
        Parent::Node(branch),
        sctx,
        ctx,
        need_fragment_wrapper,
        sctx.with_slot_scope_id,
    )
}

pub fn ssr_process_for(
    node: NodeId,
    sctx: &mut SsrCtx,
    ctx: &mut TransformContext,
    disable_nested_fragments: bool,
) {
    let children = ctx.a.for_node(node).children.clone();
    let need_fragment_wrapper = !disable_nested_fragments
        && (children.len() != 1 || !ctx.a.is(children[0], NodeType::Element));
    let parse_result = ctx.a.for_node(node).parse_result.clone();
    let params = create_for_loop_params(&mut ctx.a, &parse_result, Vec::new());
    let params = ctx.a.nodes(params);
    let render_loop = ctx
        .a
        .create_function_expression(Some(params), None, false, false, loc_stub());
    let body = process_children_as_statement(
        Parent::Node(node),
        sctx,
        ctx,
        need_fragment_wrapper,
        sctx.with_slot_scope_id,
    );
    ctx.a.func_mut(render_loop).body = Some(body);

    if !disable_nested_fragments {
        sctx.push_str_part(&mut ctx.a, "<!--[-->");
    }
    let source = ctx.a.for_node(node).source;
    let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_LIST);
    let call = ctx
        .a
        .create_call_expression(helper, vec![source, render_loop]);
    sctx.push_statement(call);
    if !disable_nested_fragments {
        sctx.push_str_part(&mut ctx.a, "<!--]-->");
    }
}

pub fn ssr_transform_slot_outlet(node: NodeId, ctx: &mut TransformContext) {
    if !is_slot_outlet(&ctx.a, node) {
        return;
    }
    let (slot_name, slot_props) = process_slot_outlet(node, ctx);
    let ctx_slots = ctx.a.string("_ctx.$slots");
    let props = match slot_props {
        Some(p) => p,
        None => ctx.a.string("{}"),
    };
    let null = ctx.a.string("null");
    let push = ctx.a.string("_push");
    let parent = ctx.a.string("_parent");
    let mut args = vec![ctx_slots, slot_name, props, null, push, parent];

    let scoped = ctx.opts.scope_id.clone().filter(|_| ctx.opts.slotted);
    if let Some(scope_id) = &scoped {
        let s = ctx.a.string(format!("\"{scope_id}-s\""));
        args.push(s);
    }

    let mut method = RuntimeHelper::SSR_RENDER_SLOT;
    if let Some(mut parent_node) = ctx.parent {
        let children = ctx.a.list(parent_node).clone();
        if ctx.a.is(parent_node, NodeType::IfBranch) {
            // `context.grandParent`
            match ctx.grand_parent {
                Some(gp) => parent_node = gp,
                None => return set_slot_codegen(node, method, args, ctx),
            }
        }
        let is_component = ctx.a.is(parent_node, NodeType::Element)
            && ctx.a.el(parent_node).tag_type == ElementType::Component;
        if is_component {
            let component_type = resolve_component_type(parent_node, ctx, true);
            let sym = ctx.a.sym_of(component_type);
            let is_transition = matches!(
                sym,
                Some(RuntimeHelper::TRANSITION) | Some(RuntimeHelper::TRANSITION_GROUP)
            );
            let element_children = children
                .iter()
                .filter(|c| ctx.a.is(**c, NodeType::Element))
                .count();
            if is_transition && element_children == 1 {
                method = RuntimeHelper::SSR_RENDER_SLOT_INNER;
                if scoped.is_none() {
                    let n = ctx.a.string("null");
                    args.push(n);
                }
                let t = ctx.a.string("true");
                args.push(t);
            }
        }
    }
    set_slot_codegen(node, method, args, ctx);
}

fn set_slot_codegen(
    node: NodeId,
    method: RuntimeHelper,
    args: Vec<NodeId>,
    ctx: &mut TransformContext,
) {
    let helper = ssr_helper(ctx, method);
    let call = ctx.a.create_call_expression(helper, args);
    ctx.a.el_mut(node).ssr_codegen_node = Some(call);
}

pub fn ssr_process_slot_outlet(node: NodeId, sctx: &mut SsrCtx, ctx: &mut TransformContext) {
    let render_call = match ctx.a.el(node).ssr_codegen_node {
        Some(c) => c,
        None => return,
    };
    if !ctx.a.el(node).children.is_empty() {
        let params = ctx.a.nodes(Vec::new());
        let fallback = ctx
            .a
            .create_function_expression(Some(params), None, false, false, loc_stub());
        let body = process_children_as_statement(
            Parent::Node(node),
            sctx,
            ctx,
            false,
            sctx.with_slot_scope_id,
        );
        ctx.a.func_mut(fallback).body = Some(body);
        ctx.a.call_mut(render_call).arguments[3] = fallback;
    }
    if sctx.with_slot_scope_id {
        let existing = ctx.a.call(render_call).arguments.get(6).copied();
        let replacement = match existing {
            Some(e) => {
                let s = ctx.a.str_of(e).unwrap_or_default().to_string();
                ctx.a.string(format!("{s} + _scopeId"))
            }
            None => ctx.a.string("_scopeId"),
        };
        let args = &mut ctx.a.call_mut(render_call).arguments;
        if args.len() > 6 {
            args[6] = replacement;
        } else {
            args.push(replacement);
        }
    }
    sctx.push_statement(render_call);
}

pub fn ssr_process_teleport(node: NodeId, sctx: &mut SsrCtx, ctx: &mut TransformContext) {
    let target_prop = find_prop(&ctx.a, node, "to", false, false);
    let target_prop = match target_prop {
        Some(p) => p,
        None => {
            let loc = ctx.a.loc(node).clone();
            return ssr_error(ctx, 66, Some(loc));
        }
    };
    let target = match ctx.a.node(target_prop) {
        Node::Attribute(a) => a.value.as_ref().map(|v| v.content.clone()),
        _ => None,
    };
    let target = match ctx.a.node(target_prop) {
        Node::Attribute(_) => target.map(|c| ctx.a.simple_exp(c, true)),
        _ => ctx.a.dir(target_prop).exp,
    };
    let target = match target {
        Some(t) => t,
        None => {
            let loc = ctx.a.loc(target_prop).clone();
            return ssr_error(ctx, 66, Some(loc));
        }
    };

    let disabled_prop = find_prop(&ctx.a, node, "disabled", false, true);
    let disabled = match disabled_prop {
        Some(p) => match ctx.a.node(p) {
            Node::Attribute(_) => ctx.a.string("true"),
            _ => match ctx.a.dir(p).exp {
                Some(e) => e,
                None => ctx.a.string("false"),
            },
        },
        None => ctx.a.string("false"),
    };

    let loc = ctx.a.loc(node).clone();
    let push_param = ctx.a.string("_push");
    let params = ctx.a.nodes(vec![push_param]);
    let content_fn = ctx
        .a
        .create_function_expression(Some(params), None, true, false, loc);
    let body = process_children_as_statement(
        Parent::Node(node),
        sctx,
        ctx,
        false,
        sctx.with_slot_scope_id,
    );
    ctx.a.func_mut(content_fn).body = Some(body);

    let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_TELEPORT);
    let push = ctx.a.string("_push");
    let parent = ctx.a.string("_parent");
    let call = ctx
        .a
        .create_call_expression(helper, vec![push, content_fn, target, disabled, parent]);
    sctx.push_statement(call);
}

pub fn ssr_transform_suspense_exit(node: NodeId, ctx: &mut TransformContext) {
    if ctx.a.el(node).children.is_empty() {
        return;
    }
    let saved = std::mem::take(&mut ctx.ssr_state.pending_suspense_slots);
    let slots = build_slots(node, ctx, SlotFnKind::SsrSuspense).slots;
    let wip_slots = std::mem::replace(&mut ctx.ssr_state.pending_suspense_slots, saved);
    ctx.ssr_state.suspense.insert(
        node,
        WipSuspense {
            slots_exp: Some(slots),
            wip_slots,
        },
    );
}

pub fn ssr_process_suspense(node: NodeId, sctx: &mut SsrCtx, ctx: &mut TransformContext) {
    let entry = match ctx.ssr_state.suspense.get(&node) {
        Some(e) => e.clone(),
        None => return,
    };
    for (fn_id, children) in &entry.wip_slots {
        let body = process_children_as_statement(
            Parent::WipSlot(*children),
            sctx,
            ctx,
            false,
            sctx.with_slot_scope_id,
        );
        ctx.a.func_mut(*fn_id).body = Some(body);
    }
    let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_SUSPENSE);
    let push = ctx.a.string("_push");
    let call = ctx
        .a
        .create_call_expression(helper, vec![push, entry.slots_exp.unwrap()]);
    sctx.push_statement(call);
}

pub fn ssr_transform_transition_group_exit(node: NodeId, ctx: &mut TransformContext) {
    let tag = match find_prop(&ctx.a, node, "tag", false, false) {
        Some(t) => t,
        None => return,
    };
    let other_props: Vec<NodeId> = ctx
        .a
        .el(node)
        .props
        .iter()
        .copied()
        .filter(|p| *p != tag)
        .collect();
    let r = build_props(node, ctx, Some(other_props), true, false, true);
    let props_exp = if r.props.is_some() || !r.directives.is_empty() {
        let merged = build_ssr_props(r.props, &r.directives, ctx);
        let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_ATTRS);
        Some(ctx.a.create_call_expression(helper, vec![merged]))
    } else {
        None
    };
    let scope_id = ctx.opts.scope_id.clone();
    ctx.ssr_state.transition_group.insert(
        node,
        WipTransitionGroup {
            tag,
            props_exp,
            scope_id,
        },
    );
}

pub fn ssr_process_transition_group(
    node: NodeId,
    sctx: &mut SsrCtx,
    ctx: &mut TransformContext,
) {
    let entry = match ctx.ssr_state.transition_group.get(&node) {
        Some(e) => e.clone(),
        None => {
            return process_children(Parent::Node(node), sctx, ctx, true, true, true);
        }
    };
    let is_dir = ctx.a.is(entry.tag, NodeType::Directive);
    let tag_exp = if is_dir {
        ctx.a.dir(entry.tag).exp
    } else {
        None
    };
    if is_dir {
        sctx.push_str_part(&mut ctx.a, "<");
        if let Some(e) = tag_exp {
            sctx.push_node_part(&mut ctx.a, e);
        }
    } else {
        let content = ctx
            .a
            .attr(entry.tag)
            .value
            .as_ref()
            .map(|v| v.content.clone())
            .unwrap_or_default();
        sctx.push_str_part(&mut ctx.a, &format!("<{content}"));
    }
    if let Some(p) = entry.props_exp {
        sctx.push_node_part(&mut ctx.a, p);
    }
    if let Some(s) = &entry.scope_id {
        let s = format!(" {s}");
        sctx.push_str_part(&mut ctx.a, &s);
    }
    sctx.push_str_part(&mut ctx.a, ">");
    process_children(Parent::Node(node), sctx, ctx, false, true, true);
    if is_dir {
        sctx.push_str_part(&mut ctx.a, "</");
        if let Some(e) = tag_exp {
            sctx.push_node_part(&mut ctx.a, e);
        }
        sctx.push_str_part(&mut ctx.a, ">");
    } else {
        let content = ctx
            .a
            .attr(entry.tag)
            .value
            .as_ref()
            .map(|v| v.content.clone())
            .unwrap_or_default();
        sctx.push_str_part(&mut ctx.a, &format!("</{content}>"));
    }
}

pub fn ssr_transform_transition_exit(node: NodeId, ctx: &mut TransformContext) {
    let appear = find_prop(&ctx.a, node, "appear", false, true).is_some();
    ctx.ssr_state.transition_appear.insert(node, appear);
}

pub fn ssr_process_transition(node: NodeId, sctx: &mut SsrCtx, ctx: &mut TransformContext) {
    let kept: Vec<NodeId> = ctx
        .a
        .el(node)
        .children
        .iter()
        .copied()
        .filter(|c| !ctx.a.is(*c, NodeType::Comment))
        .collect();
    ctx.a.el_mut(node).children = kept;
    let appear = ctx
        .ssr_state
        .transition_appear
        .get(&node)
        .copied()
        .unwrap_or(false);
    if appear {
        sctx.push_str_part(&mut ctx.a, "<template>");
        process_children(Parent::Node(node), sctx, ctx, false, true, false);
        sctx.push_str_part(&mut ctx.a, "</template>");
    } else {
        process_children(Parent::Node(node), sctx, ctx, false, true, false);
    }
}
