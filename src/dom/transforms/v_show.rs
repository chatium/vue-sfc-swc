//! Port of `compiler-dom/src/transforms/vShow.ts`.

use crate::core::ast::{NodeId, RuntimeHelper};
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::{DirectiveTransformResult, TransformContext};

pub fn transform_show(dir: NodeId, ctx: &mut TransformContext) -> DirectiveTransformResult {
    let (exp, loc) = {
        let d = ctx.a.dir(dir);
        (d.exp, d.loc.clone())
    };
    if exp.is_none() {
        let e = create_dom_compiler_error(DomErrorCode::X_V_SHOW_NO_EXPRESSION, Some(loc));
        ctx.on_error(e);
    }
    ctx.helper(RuntimeHelper::V_SHOW);
    DirectiveTransformResult {
        props: Vec::new(),
        need_runtime: Some(Some(RuntimeHelper::V_SHOW)),
    }
}
