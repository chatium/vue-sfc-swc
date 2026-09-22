//! `ssrTransformComponent` / `ssrProcessComponent` and `buildSSRProps`.

use crate::core::ast::*;
use crate::core::options::{DirectiveTransformKind, NodeTransformKind};
use crate::core::transform::{TransformContext, traverse_node};
use crate::core::transforms::transform_element::{
    build_directive_args, build_props, resolve_component_type,
};
use crate::core::transforms::v_slot::{SlotFnKind, build_slots};

use super::codegen::{Parent, SsrCtx, process_children, process_children_as_statement};
use super::{WipSlot, misc, ssr_helper};

/// `buildSSRProps`
pub fn build_ssr_props(
    props: Option<NodeId>,
    directives: &[NodeId],
    ctx: &mut TransformContext,
) -> NodeId {
    let mut args: Vec<NodeId> = Vec::new();
    if let Some(props) = props {
        if ctx.a.is(props, NodeType::JsCallExpression) {
            args = ctx.a.call(props).arguments.clone();
        } else {
            args.push(props);
        }
    }
    for dir in directives {
        let args_node = build_directive_args(*dir, ctx);
        let elements = ctx.a.list(args_node).clone();
        let helper = ssr_helper(ctx, RuntimeHelper::SSR_GET_DIRECTIVE_PROPS);
        let ctx_arg = ctx.a.string("_ctx");
        let mut call_args = vec![ctx_arg];
        call_args.extend(elements);
        let call = ctx.a.create_call_expression(helper, call_args);
        args.push(call);
    }
    if args.len() > 1 {
        let merge = ctx.helper_node(RuntimeHelper::MERGE_PROPS);
        ctx.a.create_call_expression(merge, args)
    } else {
        args[0]
    }
}

/// Enter half of `ssrTransformComponent`; returns the exit kind to schedule.
pub enum ComponentExit {
    Component,
    Suspense,
    TransitionGroup,
    Transition,
    None,
}

pub fn ssr_transform_component(node: NodeId, ctx: &mut TransformContext) -> ComponentExit {
    let component = resolve_component_type(node, ctx, true);
    ctx.ssr_state.component_type.insert(node, component);
    if let Some(sym) = ctx.a.sym_of(component) {
        return match sym {
            RuntimeHelper::SUSPENSE => ComponentExit::Suspense,
            RuntimeHelper::TRANSITION_GROUP => ComponentExit::TransitionGroup,
            RuntimeHelper::TRANSITION => ComponentExit::Transition,
            _ => ComponentExit::None,
        };
    }
    let cloned = clone_node(node, ctx);
    ctx.ssr_state.component_clone.insert(node, cloned);
    ComponentExit::Component
}

pub fn ssr_transform_component_exit(node: NodeId, ctx: &mut TransformContext) {
    let component = ctx.ssr_state.component_type[&node];
    let is_dynamic_component = ctx.a.is(component, NodeType::JsCallExpression)
        && ctx.a.sym_of(ctx.a.call(component).callee)
            == Some(RuntimeHelper::RESOLVE_DYNAMIC_COMPONENT);

    // the vnode fallback branches are built from the copy taken on the way in
    let cloned = ctx.ssr_state.component_clone[&node];
    let saved_branches = std::mem::take(&mut ctx.ssr_state.pending_vnode_branches);
    if !ctx.a.el(cloned).children.is_empty() {
        build_slots(cloned, ctx, SlotFnKind::SsrVNodeBranch);
    }
    let vnode_branches =
        std::mem::replace(&mut ctx.ssr_state.pending_vnode_branches, saved_branches);

    let mut props_exp = ctx.a.string("null");
    if !ctx.a.el(node).props.is_empty() {
        let r = build_props(node, ctx, None, true, is_dynamic_component, false);
        if r.props.is_some() || !r.directives.is_empty() {
            props_exp = build_ssr_props(r.props, &r.directives, ctx);
        }
    }

    let saved_slots = std::mem::take(&mut ctx.ssr_state.pending_slots);
    let saved_vnode = std::mem::replace(&mut ctx.ssr_state.pending_vnode_branches, vnode_branches);
    let slots = if !ctx.a.el(node).children.is_empty() {
        build_slots(node, ctx, SlotFnKind::SsrComponent).slots
    } else {
        ctx.a.string("null")
    };
    let wip = std::mem::replace(&mut ctx.ssr_state.pending_slots, saved_slots);
    ctx.ssr_state.pending_vnode_branches = saved_vnode;
    ctx.ssr_state.component_slots.insert(node, wip);

    let call = if ctx.a.str_of(component).is_none() {
        let create_vnode = ctx.helper_node(RuntimeHelper::CREATE_VNODE);
        let vnode = ctx
            .a
            .create_call_expression(create_vnode, vec![component, props_exp, slots]);
        let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_VNODE);
        let push = ctx.a.string("_push");
        let parent = ctx.a.string("_parent");
        ctx.a.create_call_expression(helper, vec![push, vnode, parent])
    } else {
        let helper = ssr_helper(ctx, RuntimeHelper::SSR_RENDER_COMPONENT);
        let parent = ctx.a.string("_parent");
        ctx.a
            .create_call_expression(helper, vec![component, props_exp, slots, parent])
    };
    ctx.a.el_mut(node).ssr_codegen_node = Some(call);
}

pub fn ssr_process_component(
    node: NodeId,
    sctx: &mut SsrCtx,
    ctx: &mut TransformContext,
    parent: Parent,
) {
    let component = ctx.ssr_state.component_type.get(&node).copied();
    let sym = component.and_then(|c| ctx.a.sym_of(c));
    if ctx.a.el(node).ssr_codegen_node.is_none() {
        match sym {
            Some(RuntimeHelper::TELEPORT) => misc::ssr_process_teleport(node, sctx, ctx),
            Some(RuntimeHelper::SUSPENSE) => misc::ssr_process_suspense(node, sctx, ctx),
            Some(RuntimeHelper::TRANSITION_GROUP) => {
                misc::ssr_process_transition_group(node, sctx, ctx)
            }
            _ => {
                if matches!(parent, Parent::WipSlot(_)) {
                    sctx.push_str_part(&mut ctx.a, "");
                }
                if sym == Some(RuntimeHelper::TRANSITION) {
                    misc::ssr_process_transition(node, sctx, ctx)
                } else {
                    process_children(Parent::Node(node), sctx, ctx, false, false, false)
                }
            }
        }
        return;
    }

    let wip = ctx
        .ssr_state
        .component_slots
        .get(&node)
        .cloned()
        .unwrap_or_default();
    for WipSlot {
        fn_id,
        children,
        vnode_branch,
    } in &wip
    {
        let consequent = process_children_as_statement(
            Parent::WipSlot(*children),
            sctx,
            ctx,
            false,
            true,
        );
        let test = ctx.a.simple_exp("_push", false);
        let stmt = ctx.a.add(Node::IfStatement(Box::new(IfStatement {
            test,
            consequent,
            alternate: *vnode_branch,
        })));
        ctx.a.func_mut(*fn_id).body = Some(stmt);
    }

    let call = ctx.a.el(node).ssr_codegen_node.unwrap();
    if sctx.with_slot_scope_id {
        let s = ctx.a.string("_scopeId");
        ctx.a.call_mut(call).arguments.push(s);
    }
    if ctx.a.str_of(component.unwrap()).is_some() {
        let push = ctx.a.string("_push");
        let wrapped = ctx.a.create_call_expression(push, vec![call]);
        sctx.push_statement(wrapped);
    } else {
        sctx.push_statement(call);
    }
}

/// `createVNodeSlotBranch` — re-runs the vnode transforms over a `<template>`
/// wrapper so a client-side fallback exists for each slot.
pub fn create_vnode_slot_branch(
    slot_props: Option<NodeId>,
    v_for: Option<NodeId>,
    children: NodeId,
    ctx: &mut TransformContext,
) -> NodeId {
    let mut wrapper_props: Vec<NodeId> = Vec::new();
    if let Some(p) = slot_props {
        let dir = ctx.a.add(Node::Directive(Box::new(DirectiveNode {
            name: "slot".into(),
            raw_name: None,
            exp: Some(p),
            arg: None,
            modifiers: Vec::new(),
            for_parse_result: None,
            loc: loc_stub(),
        })));
        wrapper_props.push(dir);
    }
    if let Some(f) = v_for {
        let copy = ctx.a.node(f).clone();
        wrapper_props.push(ctx.a.add(copy));
    }
    let child_list = ctx.a.list(children).clone();
    let wrapper = ctx.a.add(Node::Element(Box::new(ElementNode {
        ns: Namespace::Html,
        tag: "template".into(),
        tag_type: ElementType::Template,
        props: wrapper_props,
        children: child_list,
        is_self_closing: false,
        inner_loc: None,
        codegen_node: None,
        ssr_codegen_node: None,
        loc: loc_stub(),
    })));
    sub_transform(wrapper, ctx);
    let returns = ctx.a.children_ref(wrapper);
    ctx.a.add(Node::ReturnStatement(returns))
}

/// `Object.getOwnPropertyNames(Object.prototype)`
const OBJECT_PROTOTYPE_MEMBERS: &[&str] = &[
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// `subTransform` — the same arena and helper/asset sets, but the vnode
/// transform preset and an isolated copy of the scope bookkeeping.
fn sub_transform(node: NodeId, ctx: &mut TransformContext) {
    let child_root = ctx.a.create_root(vec![node], String::new());
    let saved_node_transforms = std::mem::replace(
        &mut ctx.opts.node_transforms,
        vnode_node_transforms(&ctx.ssr_state.raw_node_transforms),
    );
    let saved_dir_transforms = std::mem::replace(
        &mut ctx.opts.directive_transforms,
        vnode_directive_transforms(&ctx.ssr_state.raw_directive_transforms),
    );
    let saved_ssr = std::mem::replace(&mut ctx.opts.ssr, false);
    let saved_scopes = ctx.scopes;
    let saved_identifiers = ctx.identifiers.clone();
    // `{ ...parentContext.identifiers }` is a plain object, so `Object.prototype`
    // stays in its lookup chain and answers for every member it defines
    for name in OBJECT_PROTOTYPE_MEMBERS {
        ctx.identifiers.entry(name.to_string()).or_insert(1);
    }
    let saved_parent = ctx.parent;
    let saved_grand_parent = ctx.grand_parent;
    let saved_index = ctx.child_index;
    let saved_current = ctx.current_node;
    let saved_root = ctx.root;
    ctx.root = child_root;

    traverse_node(child_root, ctx);

    ctx.root = saved_root;
    ctx.opts.node_transforms = saved_node_transforms;
    ctx.opts.directive_transforms = saved_dir_transforms;
    ctx.opts.ssr = saved_ssr;
    ctx.scopes = saved_scopes;
    ctx.identifiers = saved_identifiers;
    ctx.parent = saved_parent;
    ctx.grand_parent = saved_grand_parent;
    ctx.child_index = saved_index;
    ctx.current_node = saved_current;
}

fn vnode_node_transforms(user: &[NodeTransformKind]) -> Vec<NodeTransformKind> {
    use NodeTransformKind::*;
    let mut v = vec![
        TransformVBindShorthand,
        TransformOnce,
        TransformIf,
        TransformMemo,
        TransformFor,
        // `getBaseTransformPreset(true)`
        TrackVForSlotScopes,
        TransformExpression,
        TransformSlotOutlet,
        TransformElement,
        TrackSlotScopes,
        TransformText,
        // `DOMNodeTransforms`
        TransformStyle,
    ];
    v.extend_from_slice(user);
    v
}

fn vnode_directive_transforms(
    user: &std::collections::HashMap<String, DirectiveTransformKind>,
) -> std::collections::HashMap<String, DirectiveTransformKind> {
    use DirectiveTransformKind::*;
    let mut v: std::collections::HashMap<String, DirectiveTransformKind> = [
        ("on", On),
        ("bind", Bind),
        ("model", Model),
        // `DOMDirectiveTransforms`
        ("cloak", Cloak),
        ("html", Html),
        ("text", Text),
        ("model", DomModel),
        ("on", DomOn),
        ("show", Show),
    ]
    .iter()
    .map(|(n, k)| (n.to_string(), *k))
    .collect();
    for (n, k) in user {
        v.insert(n.clone(), *k);
    }
    v
}

/// `clone()` — a deep copy of the untransformed subtree, so the vnode branch
/// transforms rewrite their own expressions rather than the SSR ones.
fn clone_node(node: NodeId, ctx: &mut TransformContext) -> NodeId {
    let copy = ctx.a.node(node).clone();
    let new_id = ctx.a.add(copy);
    match ctx.a.node(new_id).clone() {
        Node::Element(_) => {
            let children = ctx.a.el(new_id).children.clone();
            let cloned: Vec<NodeId> = children.into_iter().map(|c| clone_node(c, ctx)).collect();
            ctx.a.el_mut(new_id).children = cloned;
            let props = ctx.a.el(new_id).props.clone();
            let cloned: Vec<NodeId> = props.into_iter().map(|p| clone_node(p, ctx)).collect();
            ctx.a.el_mut(new_id).props = cloned;
        }
        Node::Interpolation(i) => {
            let content = clone_node(i.content, ctx);
            ctx.a.interp_mut(new_id).content = content;
        }
        Node::Directive(d) => {
            let exp = d.exp.map(|e| clone_node(e, ctx));
            let arg = d.arg.map(|a| clone_node(a, ctx));
            ctx.a.dir_mut(new_id).exp = exp;
            ctx.a.dir_mut(new_id).arg = arg;
            if let Some(mut r) = d.for_parse_result {
                r.source = clone_node(r.source, ctx);
                r.value = r.value.map(|v| clone_node(v, ctx));
                r.key = r.key.map(|k| clone_node(k, ctx));
                r.index = r.index.map(|i| clone_node(i, ctx));
                ctx.a.dir_mut(new_id).for_parse_result = Some(r);
            }
        }
        Node::CompoundExpression(c) => {
            let cloned: Vec<NodeId> =
                c.children.into_iter().map(|c| clone_node(c, ctx)).collect();
            ctx.a.compound_mut(new_id).children = cloned;
        }
        _ => {}
    }
    new_id
}
