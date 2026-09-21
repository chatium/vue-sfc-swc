//! Port of `compiler-dom/src/transforms/vHtml.ts`.

use crate::core::ast::*;
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::{DirectiveTransformResult, TransformContext};

pub fn transform_v_html(
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
) -> DirectiveTransformResult {
    let (exp, loc) = {
        let d = ctx.a.dir(dir);
        (d.exp, d.loc.clone())
    };
    if exp.is_none() {
        let e = create_dom_compiler_error(DomErrorCode::X_V_HTML_NO_EXPRESSION, Some(loc.clone()));
        ctx.on_error(e);
    }
    if !ctx.a.el(node).children.is_empty() {
        let e = create_dom_compiler_error(DomErrorCode::X_V_HTML_WITH_CHILDREN, Some(loc.clone()));
        ctx.on_error(e);
        ctx.a.el_mut(node).children.clear();
    }
    let key =
        ctx.a
            .create_simple_expression("innerHTML", true, loc, ConstantType::NotConstant);
    let value = match exp {
        Some(e) => e,
        None => ctx.a.simple_exp("", true),
    };
    let p = ctx.a.create_object_property(key, value);
    DirectiveTransformResult {
        props: vec![p],
        need_runtime: None,
    }
}
