//! Port of `compiler-dom/src/transforms/vText.ts`.

use crate::core::ast::*;
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::{DirectiveTransformResult, TransformContext};
use crate::core::transforms::cache_static::get_constant_type;

pub fn transform_v_text(
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
) -> DirectiveTransformResult {
    let (exp, loc) = {
        let d = ctx.a.dir(dir);
        (d.exp, d.loc.clone())
    };
    if exp.is_none() {
        let e = create_dom_compiler_error(DomErrorCode::X_V_TEXT_NO_EXPRESSION, Some(loc.clone()));
        ctx.on_error(e);
    }
    if !ctx.a.el(node).children.is_empty() {
        let e = create_dom_compiler_error(DomErrorCode::X_V_TEXT_WITH_CHILDREN, Some(loc.clone()));
        ctx.on_error(e);
        ctx.a.el_mut(node).children.clear();
    }
    let key = ctx.a.simple_exp("textContent", true);
    let value = match exp {
        Some(e) => {
            if get_constant_type(e, ctx) > ConstantType::NotConstant {
                e
            } else {
                let helper = ctx.helper_string(RuntimeHelper::TO_DISPLAY_STRING);
                let callee = ctx.a.string(helper);
                let call = ctx.a.create_call_expression(callee, vec![e]);
                ctx.a.call_mut(call).loc = loc;
                call
            }
        }
        None => ctx.a.simple_exp("", true),
    };
    let p = ctx.a.create_object_property(key, value);
    DirectiveTransformResult {
        props: vec![p],
        need_runtime: None,
    }
}
