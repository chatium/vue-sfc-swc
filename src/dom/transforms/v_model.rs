//! Port of `compiler-dom/src/transforms/vModel.ts`.

use crate::core::ast::*;
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::{DirectiveTransformResult, TransformContext};
use crate::core::transforms::v_model::transform_model as base_transform;
use crate::core::utils::{find_dir, find_prop, has_dynamic_key_v_bind, is_static_arg_of};

pub fn transform_model(
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
) -> DirectiveTransformResult {
    let mut base_result = base_transform(dir, node, ctx);
    if base_result.props.is_empty() || ctx.a.el(node).tag_type == ElementType::Component {
        return base_result;
    }

    if let Some(arg) = ctx.a.dir(dir).arg {
        let loc = ctx.a.loc(arg).clone();
        let e = create_dom_compiler_error(DomErrorCode::X_V_MODEL_ARG_ON_ELEMENT, Some(loc));
        ctx.on_error(e);
    }

    let tag = ctx.a.el(node).tag.clone();
    let is_custom_element = ctx
        .opts
        .is_custom_element
        .as_ref()
        .map(|f| f(&tag))
        .unwrap_or(false);

    if tag == "input" || tag == "textarea" || tag == "select" || is_custom_element {
        let mut directive_to_use = RuntimeHelper::V_MODEL_TEXT;
        let mut is_invalid_type = false;
        if tag == "input" || is_custom_element {
            let ty = find_prop(&ctx.a, node, "type", false, false);
            if let Some(ty) = ty {
                if ctx.a.is(ty, NodeType::Directive) {
                    directive_to_use = RuntimeHelper::V_MODEL_DYNAMIC;
                } else if let Some(v) = ctx.a.attr(ty).value.clone() {
                    match v.content.as_str() {
                        "radio" => directive_to_use = RuntimeHelper::V_MODEL_RADIO,
                        "checkbox" => directive_to_use = RuntimeHelper::V_MODEL_CHECKBOX,
                        "file" => {
                            is_invalid_type = true;
                            let loc = ctx.a.loc(dir).clone();
                            let e = create_dom_compiler_error(
                                DomErrorCode::X_V_MODEL_ON_FILE_INPUT_ELEMENT,
                                Some(loc),
                            );
                            ctx.on_error(e);
                        }
                        _ => check_duplicated_value(node, ctx),
                    }
                }
            } else if has_dynamic_key_v_bind(&ctx.a, node) {
                directive_to_use = RuntimeHelper::V_MODEL_DYNAMIC;
            } else {
                check_duplicated_value(node, ctx);
            }
        } else if tag == "select" {
            directive_to_use = RuntimeHelper::V_MODEL_SELECT;
        } else {
            check_duplicated_value(node, ctx);
        }
        if !is_invalid_type {
            ctx.helper(directive_to_use);
            base_result.need_runtime = Some(Some(directive_to_use));
        }
    } else {
        let loc = ctx.a.loc(dir).clone();
        let e = create_dom_compiler_error(DomErrorCode::X_V_MODEL_ON_INVALID_ELEMENT, Some(loc));
        ctx.on_error(e);
    }

    base_result.props.retain(|p| {
        let key = ctx.a.prop(*p).key;
        !matches!(ctx.a.node(key), Node::SimpleExpression(e) if e.content == "modelValue")
    });
    base_result
}

fn check_duplicated_value(node: NodeId, ctx: &mut TransformContext) {
    if let Some(value) = find_dir(&ctx.a, node, "bind", false) {
        let arg = ctx.a.dir(value).arg;
        if is_static_arg_of(&ctx.a, &arg, "value") {
            let loc = ctx.a.loc(value).clone();
            let e =
                create_dom_compiler_error(DomErrorCode::X_V_MODEL_UNNECESSARY_VALUE, Some(loc));
            ctx.on_error(e);
        }
    }
}
