//! `ssrTransformShow`.

use crate::core::ast::*;
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::{DirectiveTransformResult, TransformContext};

pub fn ssr_transform_show(dir: NodeId, ctx: &mut TransformContext) -> DirectiveTransformResult {
    let exp = ctx.a.dir(dir).exp;
    if exp.is_none() {
        let e = create_dom_compiler_error(DomErrorCode::X_V_SHOW_NO_EXPRESSION, None);
        ctx.on_error(e);
    }
    let display = ctx.a.simple_exp("none", true);
    let display_prop = ctx.a.create_object_property_str("display", display);
    let hidden = ctx.a.create_object_expression(vec![display_prop]);
    let null = ctx.a.simple_exp("null", false);
    let test = exp.unwrap_or(null);
    let cond = ctx
        .a
        .create_conditional_expression(test, null, hidden, false);
    let prop = ctx.a.create_object_property_str("style", cond);
    DirectiveTransformResult {
        props: vec![prop],
        need_runtime: None,
    }
}
