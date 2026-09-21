//! Port of `compiler-dom/src/transforms/vOn.ts`.

use crate::core::ast::*;
use crate::core::transform::{DirectiveTransformResult, TransformContext, capitalize};
use crate::core::transforms::v_on::transform_on as base_transform;
use crate::core::utils::is_static_exp;

fn is_event_option_modifier(m: &str) -> bool {
    matches!(m, "passive" | "once" | "capture")
}

fn is_non_key_modifier(m: &str) -> bool {
    matches!(
        m,
        "stop" | "prevent" | "self" | "ctrl" | "shift" | "alt" | "meta" | "exact" | "middle"
    )
}

fn maybe_key_modifier(m: &str) -> bool {
    m == "left" || m == "right"
}

fn is_keyboard_event(m: &str) -> bool {
    matches!(m, "onkeyup" | "onkeydown" | "onkeypress")
}

pub fn transform_on(
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
) -> DirectiveTransformResult {
    base_transform(dir, node, ctx, Some(augment))
}

fn augment(
    base_result: DirectiveTransformResult,
    dir: NodeId,
    _node: NodeId,
    ctx: &mut TransformContext,
) -> DirectiveTransformResult {
    let modifiers = ctx.a.dir(dir).modifiers.clone();
    if modifiers.is_empty() {
        return base_result;
    }
    let mut key = ctx.a.prop(base_result.props[0]).key;
    let mut handler_exp = ctx.a.prop(base_result.props[0]).value;

    let mut key_modifiers: Vec<String> = Vec::new();
    let mut non_key_modifiers: Vec<String> = Vec::new();
    let mut event_option_modifiers: Vec<String> = Vec::new();

    for m in &modifiers {
        let modifier = ctx.a.exp(*m).content.clone();
        if is_event_option_modifier(&modifier) {
            event_option_modifiers.push(modifier);
        } else if maybe_key_modifier(&modifier) {
            if is_static_exp(&ctx.a, key) {
                if is_keyboard_event(&ctx.a.exp(key).content.to_lowercase()) {
                    key_modifiers.push(modifier);
                } else {
                    non_key_modifiers.push(modifier);
                }
            } else {
                key_modifiers.push(modifier.clone());
                non_key_modifiers.push(modifier);
            }
        } else if is_non_key_modifier(&modifier) {
            non_key_modifiers.push(modifier);
        } else {
            key_modifiers.push(modifier);
        }
    }

    if non_key_modifiers.iter().any(|m| m == "right") {
        key = transform_click(key, "onContextmenu", ctx);
    }
    if non_key_modifiers.iter().any(|m| m == "middle") {
        key = transform_click(key, "onMouseup", ctx);
    }

    if !non_key_modifiers.is_empty() {
        let helper = ctx.helper_node(RuntimeHelper::V_ON_WITH_MODIFIERS);
        let json = ctx
            .a
            .string(serde_json::to_string(&non_key_modifiers).unwrap());
        handler_exp = ctx
            .a
            .create_call_expression(helper, vec![handler_exp, json]);
    }

    if !key_modifiers.is_empty()
        && (!is_static_exp(&ctx.a, key)
            || is_keyboard_event(&ctx.a.exp(key).content.to_lowercase()))
    {
        let helper = ctx.helper_node(RuntimeHelper::V_ON_WITH_KEYS);
        let json = ctx
            .a
            .string(serde_json::to_string(&key_modifiers).unwrap());
        handler_exp = ctx
            .a
            .create_call_expression(helper, vec![handler_exp, json]);
    }

    if !event_option_modifiers.is_empty() {
        let postfix: String = event_option_modifiers
            .iter()
            .map(|m| capitalize(m))
            .collect::<Vec<_>>()
            .join("");
        key = if is_static_exp(&ctx.a, key) {
            let c = ctx.a.exp(key).content.clone();
            ctx.a.simple_exp(format!("{c}{postfix}"), true)
        } else {
            let open = ctx.a.string("(");
            let close = ctx.a.string(format!(") + \"{postfix}\""));
            ctx.a
                .create_compound_expression(vec![open, key, close], loc_stub())
        };
    }

    let p = ctx.a.create_object_property(key, handler_exp);
    DirectiveTransformResult {
        props: vec![p],
        need_runtime: None,
    }
}

fn transform_click(key: NodeId, event: &str, ctx: &mut TransformContext) -> NodeId {
    let is_static_click =
        is_static_exp(&ctx.a, key) && ctx.a.exp(key).content.to_lowercase() == "onclick";
    if is_static_click {
        ctx.a.simple_exp(event, true)
    } else if !ctx.a.is(key, NodeType::SimpleExpression) {
        let a = ctx.a.string("(");
        let b = ctx.a.string(format!(") === \"onClick\" ? \"{event}\" : ("));
        let c = ctx.a.string(")");
        ctx.a
            .create_compound_expression(vec![a, key, b, key, c], loc_stub())
    } else {
        key
    }
}
