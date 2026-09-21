//! Port of `compiler-dom/src/transforms/ignoreSideEffectTags.ts`.

use crate::core::ast::*;
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::TransformContext;

pub fn ignore_side_effect_tags(node: NodeId, ctx: &mut TransformContext) {
    if ctx.a.is(node, NodeType::Element)
        && ctx.a.el(node).tag_type == ElementType::Element
        && (ctx.a.el(node).tag == "script" || ctx.a.el(node).tag == "style")
    {
        let loc = ctx.a.loc(node).clone();
        let e = create_dom_compiler_error(DomErrorCode::X_IGNORED_SIDE_EFFECT_TAG, Some(loc));
        ctx.on_error(e);
        ctx.remove_node(None);
    }
}
