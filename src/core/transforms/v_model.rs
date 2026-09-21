//! Port of `compiler-core/src/transforms/vModel.ts`.

use crate::core::ast::*;
use crate::core::errors::ErrorCode;
use crate::core::options::BindingType;
use crate::core::transform::{DirectiveTransformResult, TransformContext, camelize};
use crate::core::utils::{has_scope_ref, is_member_expression, is_simple_identifier, is_static_exp};

pub fn transform_model(
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
) -> DirectiveTransformResult {
    let (exp, arg) = {
        let d = ctx.a.dir(dir);
        (d.exp, d.arg)
    };
    let exp = match exp {
        Some(e) => e,
        None => {
            let loc = ctx.a.loc(dir).clone();
            ctx.error(ErrorCode::X_V_MODEL_NO_EXPRESSION, Some(loc));
            return DirectiveTransformResult {
                props: Vec::new(),
                need_runtime: None,
            };
        }
    };

    let raw_exp = ctx.a.loc(exp).source.trim().to_string();
    let exp_string = if ctx.a.is(exp, NodeType::SimpleExpression) {
        ctx.a.exp(exp).content.clone()
    } else {
        raw_exp.clone()
    };
    let binding_type = ctx.opts.binding_metadata.get(&raw_exp);

    if matches!(
        binding_type,
        Some(BindingType::Props) | Some(BindingType::PropsAliased)
    ) {
        let loc = ctx.a.loc(exp).clone();
        ctx.error(ErrorCode::X_V_MODEL_ON_PROPS, Some(loc));
        return DirectiveTransformResult {
            props: Vec::new(),
            need_runtime: None,
        };
    }
    if matches!(
        binding_type,
        Some(BindingType::LiteralConst) | Some(BindingType::SetupConst)
    ) {
        let loc = ctx.a.loc(exp).clone();
        ctx.error(ErrorCode::X_V_MODEL_ON_CONST, Some(loc));
        return DirectiveTransformResult {
            props: Vec::new(),
            need_runtime: None,
        };
    }

    let maybe_ref = ctx.opts.inline
        && matches!(
            binding_type,
            Some(BindingType::SetupLet)
                | Some(BindingType::SetupRef)
                | Some(BindingType::SetupMaybeRef)
        );

    if exp_string.trim().is_empty() || (!is_member_expression(&ctx.a, exp) && !maybe_ref) {
        let loc = ctx.a.loc(exp).clone();
        ctx.error(ErrorCode::X_V_MODEL_MALFORMED_EXPRESSION, Some(loc));
        return DirectiveTransformResult {
            props: Vec::new(),
            need_runtime: None,
        };
    }

    if ctx.opts.prefix_identifiers
        && is_simple_identifier(&exp_string)
        && ctx.identifiers.get(&exp_string).copied().unwrap_or(0) > 0
    {
        let loc = ctx.a.loc(exp).clone();
        ctx.error(ErrorCode::X_V_MODEL_ON_SCOPE_VARIABLE, Some(loc));
        return DirectiveTransformResult {
            props: Vec::new(),
            need_runtime: None,
        };
    }

    let prop_name = match arg {
        Some(a) => a,
        None => ctx.a.simple_exp("modelValue", true),
    };
    let event_name: NodeId = match arg {
        Some(a) => {
            if is_static_exp(&ctx.a, a) {
                let c = camelize(&ctx.a.exp(a).content);
                ctx.a.string(format!("onUpdate:{c}"))
            } else {
                let head = ctx.a.string("\"onUpdate:\" + ");
                ctx.a
                    .create_compound_expression(vec![head, a], loc_stub())
            }
        }
        None => ctx.a.string("onUpdate:modelValue"),
    };

    let event_arg = if ctx.opts.is_ts {
        "($event: any)"
    } else {
        "$event"
    };
    let exp_loc = ctx.a.loc(exp).clone();
    let assignment_exp: NodeId = if maybe_ref {
        if binding_type == Some(BindingType::SetupRef) {
            let head = ctx.a.string(format!("{event_arg} => (("));
            let mid = ctx.a.create_simple_expression(
                raw_exp.clone(),
                false,
                exp_loc.clone(),
                ConstantType::NotConstant,
            );
            let tail = ctx.a.string(").value = $event)");
            ctx.a
                .create_compound_expression(vec![head, mid, tail], loc_stub())
        } else {
            let alt = if binding_type == Some(BindingType::SetupLet) {
                format!("{raw_exp} = $event")
            } else {
                "null".to_string()
            };
            let is_ref = ctx.helper_string(RuntimeHelper::IS_REF);
            let head = ctx
                .a
                .string(format!("{event_arg} => ({is_ref}({raw_exp}) ? ("));
            let mid = ctx.a.create_simple_expression(
                raw_exp.clone(),
                false,
                exp_loc.clone(),
                ConstantType::NotConstant,
            );
            let tail = ctx.a.string(format!(").value = $event : {alt})"));
            ctx.a
                .create_compound_expression(vec![head, mid, tail], loc_stub())
        }
    } else {
        let head = ctx.a.string(format!("{event_arg} => (("));
        let tail = ctx.a.string(") = $event)");
        ctx.a
            .create_compound_expression(vec![head, exp, tail], loc_stub())
    };

    let dir_exp = ctx.a.dir(dir).exp.unwrap();
    let p1 = ctx.a.create_object_property(prop_name, dir_exp);
    let p2 = ctx.a.create_object_property(event_name, assignment_exp);
    let mut props = vec![p1, p2];

    if ctx.opts.prefix_identifiers && !ctx.in_v_once && ctx.opts.cache_handlers {
        let ids = ctx.identifiers.clone();
        if !has_scope_ref(&ctx.a, Some(exp), &ids) {
            let v = ctx.a.prop(props[1]).value;
            let cached = ctx.cache(v, false, false);
            ctx.a.prop_mut(props[1]).value = cached;
        }
    }

    let modifiers = ctx.a.dir(dir).modifiers.clone();
    if !modifiers.is_empty() && ctx.a.el(node).tag_type == ElementType::Component {
        let mods = modifiers
            .iter()
            .map(|m| ctx.a.exp(*m).content.clone())
            .map(|m| {
                if is_simple_identifier(&m) {
                    format!("{m}: true")
                } else {
                    format!("{}: true", serde_json::to_string(&m).unwrap())
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let modifiers_key: NodeId = match arg {
            Some(a) => {
                if is_static_exp(&ctx.a, a) {
                    let c = ctx.a.exp(a).content.clone();
                    ctx.a.string(format!("{c}Modifiers"))
                } else {
                    let tail = ctx.a.string(" + \"Modifiers\"");
                    ctx.a
                        .create_compound_expression(vec![a, tail], loc_stub())
                }
            }
            None => ctx.a.string("modelModifiers"),
        };
        let dir_loc = ctx.a.loc(dir).clone();
        let value = ctx.a.create_simple_expression(
            format!("{{ {mods} }}"),
            false,
            dir_loc,
            ConstantType::CanCache,
        );
        let p = ctx.a.create_object_property(modifiers_key, value);
        props.push(p);
    }

    DirectiveTransformResult {
        props,
        need_runtime: None,
    }
}
