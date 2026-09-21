//! Port of `compiler-core/src/transforms/vSlot.ts`.

use std::collections::HashSet;

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::transform::{ExitFn, TransformContext};
use crate::core::utils::{
    find_dir, find_dir_matching, has_scope_ref, is_comment_or_whitespace, is_static_exp,
    is_template_node, is_v_slot, is_whitespace_text,
};

use super::v_for::{create_for_loop_params, finalize_for_parse_result_on_dir};

const SLOT_STABLE: i32 = 1;
const SLOT_DYNAMIC: i32 = 2;
const SLOT_FORWARDED: i32 = 3;

fn slot_flag_text(f: i32) -> &'static str {
    match f {
        1 => "STABLE",
        2 => "DYNAMIC",
        3 => "FORWARDED",
        _ => "",
    }
}

pub fn track_slot_scopes(node: NodeId, ctx: &mut TransformContext) -> Vec<ExitFn> {
    if !ctx.a.is(node, NodeType::Element) {
        return Vec::new();
    }
    let tag_type = ctx.a.el(node).tag_type;
    if tag_type != ElementType::Component && tag_type != ElementType::Template {
        return Vec::new();
    }
    let v_slot = match find_dir(&ctx.a, node, "slot", false) {
        Some(d) => d,
        None => return Vec::new(),
    };
    let slot_props = ctx.a.dir(v_slot).exp;
    if ctx.opts.prefix_identifiers {
        if let Some(sp) = slot_props {
            ctx.add_identifiers(sp);
        }
    }
    ctx.scopes.v_slot += 1;
    vec![ExitFn::SlotScopes { slot_props }]
}

pub fn exit_slot_scopes(slot_props: Option<NodeId>, ctx: &mut TransformContext) {
    if ctx.opts.prefix_identifiers {
        if let Some(sp) = slot_props {
            ctx.remove_identifiers(sp);
        }
    }
    ctx.scopes.v_slot -= 1;
}

pub fn track_v_for_slot_scopes(node: NodeId, ctx: &mut TransformContext) -> Vec<ExitFn> {
    if !is_template_node(&ctx.a, node) {
        return Vec::new();
    }
    let has_v_slot = ctx
        .a
        .el(node)
        .props
        .iter()
        .any(|p| is_v_slot(&ctx.a, *p));
    if !has_v_slot {
        return Vec::new();
    }
    let v_for = match find_dir(&ctx.a, node, "for", false) {
        Some(d) => d,
        None => return Vec::new(),
    };
    if ctx.a.dir(v_for).for_parse_result.is_none() {
        return Vec::new();
    }
    finalize_for_parse_result_on_dir(v_for, ctx);
    let r = ctx.a.dir(v_for).for_parse_result.clone().unwrap();
    if let Some(v) = r.value {
        ctx.add_identifiers(v);
    }
    if let Some(k) = r.key {
        ctx.add_identifiers(k);
    }
    if let Some(i) = r.index {
        ctx.add_identifiers(i);
    }
    vec![ExitFn::VForSlotScopes {
        value: r.value,
        key: r.key,
        index: r.index,
    }]
}

pub fn exit_v_for_slot_scopes(
    value: Option<NodeId>,
    key: Option<NodeId>,
    index: Option<NodeId>,
    ctx: &mut TransformContext,
) {
    if let Some(v) = value {
        ctx.remove_identifiers(v);
    }
    if let Some(k) = key {
        ctx.remove_identifiers(k);
    }
    if let Some(i) = index {
        ctx.remove_identifiers(i);
    }
}

/// which `buildSlotFn` `buildSlots` was handed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotFnKind {
    Client,
    SsrComponent,
    SsrSuspense,
    SsrVNodeBranch,
}

fn build_slot_fn(
    kind: SlotFnKind,
    props: Option<NodeId>,
    v_for: Option<NodeId>,
    children: NodeId,
    children_first_loc: Option<SourceLocation>,
    loc: SourceLocation,
    ctx: &mut TransformContext,
) -> NodeId {
    match kind {
        SlotFnKind::Client => {
            let l = children_first_loc.unwrap_or(loc);
            ctx.a
                .create_function_expression(props, Some(children), false, true, l)
        }
        _ => crate::ssr::build_ssr_slot_fn(kind, props, v_for, children, loc, ctx),
    }
}

pub struct SlotsResult {
    pub slots: NodeId,
    pub has_dynamic_slots: bool,
}

pub fn build_slots(
    node: NodeId,
    ctx: &mut TransformContext,
    kind: SlotFnKind,
) -> SlotsResult {
    ctx.helper(RuntimeHelper::WITH_CTX);

    let children = ctx.a.el(node).children.clone();
    let loc = ctx.a.loc(node).clone();
    let mut slots_properties: Vec<NodeId> = Vec::new();
    let mut dynamic_slots: Vec<NodeId> = Vec::new();

    let mut has_dynamic_slots = ctx.scopes.v_slot > 0 || ctx.scopes.v_for > 0;
    if !ctx.opts.ssr && ctx.opts.prefix_identifiers {
        let ids = ctx.identifiers.clone();
        has_dynamic_slots = ctx.a.el(node).props.iter().any(|p| {
            is_v_slot(&ctx.a, *p)
                && (has_scope_ref(&ctx.a, ctx.a.dir(*p).arg, &ids)
                    || has_scope_ref(&ctx.a, ctx.a.dir(*p).exp, &ids))
        }) || children
            .iter()
            .any(|c| has_scope_ref(&ctx.a, Some(*c), &ids));
    }

    // 1. v-slot on the component itself
    let on_component_slot = find_dir(&ctx.a, node, "slot", true);
    if let Some(dir) = on_component_slot {
        let (arg, exp) = {
            let d = ctx.a.dir(dir);
            (d.arg, d.exp)
        };
        if let Some(arg) = arg {
            if !is_static_exp(&ctx.a, arg) {
                has_dynamic_slots = true;
            }
        }
        let key = match arg {
            Some(a) => a,
            None => ctx.a.simple_exp("default", true),
        };
        let children_ref = ctx.a.children_ref(node);
        let first_loc = children.first().map(|c| ctx.a.loc(*c).clone());
        let fnexp = build_slot_fn(kind, exp, None, children_ref, first_loc, loc.clone(), ctx);
        let prop = ctx.a.create_object_property(key, fnexp);
        slots_properties.push(prop);
    }

    // 2. <template v-slot:name> children
    let mut has_template_slots = false;
    let mut has_named_default_slot = false;
    let mut implicit_default_children: Vec<NodeId> = Vec::new();
    let mut seen_slot_names: HashSet<String> = HashSet::new();
    let mut conditional_branch_index = 0usize;

    for i in 0..children.len() {
        let slot_element = children[i];
        let slot_dir = if is_template_node(&ctx.a, slot_element) {
            find_dir(&ctx.a, slot_element, "slot", true)
        } else {
            None
        };
        let slot_dir = match slot_dir {
            Some(d) => d,
            None => {
                if !ctx.a.is(slot_element, NodeType::Comment) {
                    implicit_default_children.push(slot_element);
                }
                continue;
            }
        };

        if on_component_slot.is_some() {
            let dloc = ctx.a.loc(slot_dir).clone();
            ctx.error(ErrorCode::X_V_SLOT_MIXED_SLOT_USAGE, Some(dloc));
            break;
        }

        has_template_slots = true;
        let slot_children_loc = ctx.a.loc(slot_element).clone();
        let (slot_name, slot_props, dir_loc) = {
            let d = ctx.a.dir(slot_dir);
            (d.arg, d.exp, d.loc.clone())
        };
        let slot_name = match slot_name {
            Some(n) => n,
            None => ctx.a.simple_exp("default", true),
        };

        let mut static_slot_name: Option<String> = None;
        if is_static_exp(&ctx.a, slot_name) {
            static_slot_name = Some(ctx.a.exp(slot_name).content.clone());
        } else {
            has_dynamic_slots = true;
        }

        let v_for = find_dir(&ctx.a, slot_element, "for", false);
        let slot_children = ctx.a.children_ref(slot_element);
        let first_loc = ctx
            .a
            .el(slot_element)
            .children
            .first()
            .map(|c| ctx.a.loc(*c).clone());
        let slot_function = build_slot_fn(
            kind,
            slot_props,
            v_for,
            slot_children,
            first_loc,
            slot_children_loc.clone(),
            ctx,
        );

        let v_if = find_dir(&ctx.a, slot_element, "if", false);
        let v_else = find_dir_matching(
            &ctx.a,
            slot_element,
            |n| n == "else" || n == "else-if",
            true,
        );

        if let Some(v_if) = v_if {
            has_dynamic_slots = true;
            let cond = ctx.a.dir(v_if).exp.unwrap();
            let slot = build_dynamic_slot(
                slot_name,
                slot_function,
                Some(conditional_branch_index),
                ctx,
            );
            conditional_branch_index += 1;
            let fallback = ctx.a.simple_exp("undefined", false);
            let c = ctx
                .a
                .create_conditional_expression(cond, slot, fallback, true);
            dynamic_slots.push(c);
        } else if let Some(v_else) = v_else {
            // find the adjacent v-if
            let mut j = i;
            let mut prev: Option<NodeId> = None;
            while j > 0 {
                j -= 1;
                prev = Some(children[j]);
                if !is_comment_or_whitespace(&ctx.a, children[j]) {
                    break;
                }
            }
            let prev_is_if = prev
                .map(|p| {
                    is_template_node(&ctx.a, p)
                        && find_dir_matching(&ctx.a, p, |n| n == "if" || n == "else-if", false)
                            .is_some()
                })
                .unwrap_or(false);
            if prev_is_if && !dynamic_slots.is_empty() {
                let mut conditional = *dynamic_slots.last().unwrap();
                while ctx.a.is(
                    ctx.a.cond(conditional).alternate,
                    NodeType::JsConditionalExpression,
                ) {
                    conditional = ctx.a.cond(conditional).alternate;
                }
                let else_exp = ctx.a.dir(v_else).exp;
                let alternate = match else_exp {
                    Some(exp) => {
                        let slot = build_dynamic_slot(
                            slot_name,
                            slot_function,
                            Some(conditional_branch_index),
                            ctx,
                        );
                        conditional_branch_index += 1;
                        let fallback = ctx.a.simple_exp("undefined", false);
                        ctx.a
                            .create_conditional_expression(exp, slot, fallback, true)
                    }
                    None => {
                        let s = build_dynamic_slot(
                            slot_name,
                            slot_function,
                            Some(conditional_branch_index),
                            ctx,
                        );
                        conditional_branch_index += 1;
                        s
                    }
                };
                ctx.a.cond_mut(conditional).alternate = alternate;
            } else {
                let loc = ctx.a.loc(v_else).clone();
                ctx.error(ErrorCode::X_V_ELSE_NO_ADJACENT_IF, Some(loc));
            }
        } else if let Some(v_for) = v_for {
            has_dynamic_slots = true;
            if ctx.a.dir(v_for).for_parse_result.is_some() {
                finalize_for_parse_result_on_dir(v_for, ctx);
                let parse_result = ctx.a.dir(v_for).for_parse_result.clone().unwrap();
                let render_list = ctx.helper_node(RuntimeHelper::RENDER_LIST);
                let slot = build_dynamic_slot(slot_name, slot_function, None, ctx);
                let params = create_for_loop_params(&mut ctx.a, &parse_result, Vec::new());
                let params_node = ctx.a.nodes(params);
                let f = ctx.a.create_function_expression(
                    Some(params_node),
                    Some(slot),
                    true,
                    false,
                    loc_stub(),
                );
                let call = ctx
                    .a
                    .create_call_expression(render_list, vec![parse_result.source, f]);
                dynamic_slots.push(call);
            } else {
                let loc = ctx.a.loc(v_for).clone();
                ctx.error(ErrorCode::X_V_FOR_MALFORMED_EXPRESSION, Some(loc));
            }
        } else {
            if let Some(name) = &static_slot_name {
                if seen_slot_names.contains(name) {
                    ctx.error(ErrorCode::X_V_SLOT_DUPLICATE_SLOT_NAMES, Some(dir_loc));
                    continue;
                }
                seen_slot_names.insert(name.clone());
                if name == "default" {
                    has_named_default_slot = true;
                }
            }
            let prop = ctx.a.create_object_property(slot_name, slot_function);
            slots_properties.push(prop);
        }
    }

    if on_component_slot.is_none() {
        if !has_template_slots {
            let children_ref = ctx.a.children_ref(node);
            let first_loc = children.first().map(|c| ctx.a.loc(*c).clone());
            let fnexp = build_slot_fn(kind, None, None, children_ref, first_loc, loc.clone(), ctx);
            let prop = ctx.a.create_object_property_str("default", fnexp);
            slots_properties.push(prop);
        } else if !implicit_default_children.is_empty()
            && !implicit_default_children
                .iter()
                .all(|c| is_whitespace_text(&ctx.a, *c))
        {
            if has_named_default_slot {
                let l = ctx.a.loc(implicit_default_children[0]).clone();
                ctx.error(
                    ErrorCode::X_V_SLOT_EXTRANEOUS_DEFAULT_SLOT_CHILDREN,
                    Some(l),
                );
            } else {
                let list = ctx.a.nodes(implicit_default_children.clone());
                let first_loc = implicit_default_children
                    .first()
                    .map(|c| ctx.a.loc(*c).clone());
                let fnexp = build_slot_fn(kind, None, None, list, first_loc, loc.clone(), ctx);
                let prop = ctx.a.create_object_property_str("default", fnexp);
                slots_properties.push(prop);
            }
        }
    }

    let slot_flag = if has_dynamic_slots {
        SLOT_DYNAMIC
    } else if has_forwarded_slots(&ctx.a, &ctx.a.el(node).children.clone()) {
        SLOT_FORWARDED
    } else {
        SLOT_STABLE
    };
    let flag_exp = ctx.a.simple_exp(
        format!("{slot_flag} /* {} */", slot_flag_text(slot_flag)),
        false,
    );
    let flag_prop = ctx.a.create_object_property_str("_", flag_exp);
    slots_properties.push(flag_prop);
    let mut slots = ctx.a.create_object_expression(slots_properties);
    ctx.a.obj_mut(slots).loc = loc;

    if !dynamic_slots.is_empty() {
        let helper = ctx.helper_node(RuntimeHelper::CREATE_SLOTS);
        let arr = ctx.a.create_array_expression(dynamic_slots);
        slots = ctx.a.create_call_expression(helper, vec![slots, arr]);
    }

    SlotsResult {
        slots,
        has_dynamic_slots,
    }
}

fn build_dynamic_slot(
    name: NodeId,
    f: NodeId,
    index: Option<usize>,
    ctx: &mut TransformContext,
) -> NodeId {
    let p1 = ctx.a.create_object_property_str("name", name);
    let p2 = ctx.a.create_object_property_str("fn", f);
    let mut props = vec![p1, p2];
    if let Some(i) = index {
        let v = ctx.a.simple_exp(i.to_string(), true);
        let p3 = ctx.a.create_object_property_str("key", v);
        props.push(p3);
    }
    ctx.a.create_object_expression(props)
}

fn has_forwarded_slots(a: &Arena, children: &[NodeId]) -> bool {
    for child in children {
        match a.node_type(*child) {
            NodeType::Element => {
                if a.el(*child).tag_type == ElementType::Slot
                    || has_forwarded_slots(a, &a.el(*child).children)
                {
                    return true;
                }
            }
            NodeType::If => {
                if has_forwarded_slots(a, &a.if_node(*child).branches) {
                    return true;
                }
            }
            NodeType::IfBranch => {
                if has_forwarded_slots(a, &a.branch(*child).children) {
                    return true;
                }
            }
            NodeType::For => {
                if has_forwarded_slots(a, &a.for_node(*child).children) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
