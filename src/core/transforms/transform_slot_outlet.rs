//! Port of `compiler-core/src/transforms/transformSlotOutlet.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::transform::{TransformContext, camelize};
use crate::core::utils::{is_slot_outlet, is_static_arg_of, is_static_exp};

use super::transform_element::build_props;
use super::transform_expression::process_expression;

pub fn transform_slot_outlet(node: NodeId, ctx: &mut TransformContext) {
    if !is_slot_outlet(&ctx.a, node) {
        return;
    }
    let loc = ctx.a.loc(node).clone();
    let children_len = ctx.a.el(node).children.len();
    let (slot_name, slot_props) = process_slot_outlet(node, ctx);

    let ctx_slots = ctx.a.string(if ctx.opts.prefix_identifiers {
        "_ctx.$slots"
    } else {
        "$slots"
    });
    let empty_obj = ctx.a.string("{}");
    let undef = ctx.a.string("undefined");
    let tru = ctx.a.string("true");
    let mut slot_args: Vec<NodeId> = vec![ctx_slots, slot_name, empty_obj, undef, tru];
    let mut expected_len = 2usize;

    if let Some(sp) = slot_props {
        slot_args[2] = sp;
        expected_len = 3;
    }

    if children_len > 0 {
        let children_ref = ctx.a.children_ref(node);
        let empty_params = ctx.a.nodes(Vec::new());
        let f = ctx.a.create_function_expression(
            Some(empty_params),
            Some(children_ref),
            false,
            false,
            loc.clone(),
        );
        slot_args[3] = f;
        expected_len = 4;
    }

    if ctx.opts.scope_id.is_some() && !ctx.opts.slotted {
        expected_len = 5;
    }
    slot_args.truncate(expected_len);

    let helper = ctx.helper_node(RuntimeHelper::RENDER_SLOT);
    let call = ctx.a.create_call_expression(helper, slot_args);
    ctx.a.call_mut(call).loc = loc;
    ctx.a.el_mut(node).codegen_node = Some(call);
}

pub fn process_slot_outlet(
    node: NodeId,
    ctx: &mut TransformContext,
) -> (NodeId, Option<NodeId>) {
    let mut slot_name = ctx.a.string("\"default\"");
    let mut slot_props: Option<NodeId> = None;
    let mut non_name_props: Vec<NodeId> = Vec::new();

    let props = ctx.a.el(node).props.clone();
    for p in props {
        match ctx.a.node_type(p) {
            NodeType::Attribute => {
                let (name, value) = {
                    let a = ctx.a.attr(p);
                    (a.name.clone(), a.value.clone())
                };
                if value.is_some() {
                    if name == "name" {
                        let content = value.unwrap().content;
                        slot_name = ctx
                            .a
                            .string(serde_json::to_string(&content).unwrap());
                    } else {
                        ctx.a.attr_mut(p).name = camelize(&name);
                        non_name_props.push(p);
                    }
                }
            }
            NodeType::Directive => {
                let (name, arg, exp) = {
                    let d = ctx.a.dir(p);
                    (d.name.clone(), d.arg, d.exp)
                };
                if name == "bind" && is_static_arg_of(&ctx.a, &arg, "name") {
                    if let Some(exp) = exp {
                        slot_name = exp;
                    } else if let Some(arg) = arg {
                        if ctx.a.is(arg, NodeType::SimpleExpression) {
                            let n = camelize(&ctx.a.exp(arg).content);
                            let l = ctx.a.loc(arg).clone();
                            let e = ctx.a.create_simple_expression(
                                n,
                                false,
                                l,
                                ConstantType::NotConstant,
                            );
                            let processed = process_expression(e, ctx, false, false, None);
                            ctx.a.dir_mut(p).exp = Some(processed);
                            slot_name = processed;
                        }
                    }
                } else {
                    if name == "bind" {
                        if let Some(arg) = arg {
                            if is_static_exp(&ctx.a, arg) {
                                let c = camelize(&ctx.a.exp(arg).content);
                                ctx.a.exp_mut(arg).content = c;
                            }
                        }
                    }
                    non_name_props.push(p);
                }
            }
            _ => {}
        }
    }

    if !non_name_props.is_empty() {
        let result = build_props(node, ctx, Some(non_name_props), false, false, false);
        slot_props = result.props;
        if !result.directives.is_empty() {
            let loc = ctx.a.loc(result.directives[0]).clone();
            ctx.error(
                ErrorCode::X_V_SLOT_UNEXPECTED_DIRECTIVE_ON_SLOT_OUTLET,
                Some(loc),
            );
        }
    }

    (slot_name, slot_props)
}
