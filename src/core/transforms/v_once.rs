//! Port of `compiler-core/src/transforms/vOnce.ts`.

use crate::core::ast::*;
use crate::core::transform::{ExitFn, TransformContext};
use crate::core::utils::find_dir;

pub fn transform_once(node: NodeId, ctx: &mut TransformContext) -> Vec<ExitFn> {
    if ctx.a.is(node, NodeType::Element) && find_dir(&ctx.a, node, "once", true).is_some() {
        if ctx.seen_once.contains(&node) || ctx.in_v_once || ctx.opts.in_ssr {
            return Vec::new();
        }
        ctx.seen_once.insert(node);
        ctx.in_v_once = true;
        ctx.helper(RuntimeHelper::SET_BLOCK_TRACKING);
        return vec![ExitFn::Once { node }];
    }
    Vec::new()
}

pub fn exit_once(_node: NodeId, ctx: &mut TransformContext) {
    ctx.in_v_once = false;
    let cur = match ctx.current_node {
        Some(c) => c,
        None => return,
    };
    let codegen = match ctx.a.node_type(cur) {
        NodeType::Element => ctx.a.el(cur).codegen_node,
        NodeType::If => ctx.a.if_node(cur).codegen_node,
        NodeType::For => ctx.a.for_node(cur).codegen_node,
        _ => None,
    };
    if let Some(codegen) = codegen {
        let cached = ctx.cache(codegen, true, true);
        match ctx.a.node_type(cur) {
            NodeType::Element => ctx.a.el_mut(cur).codegen_node = Some(cached),
            NodeType::If => ctx.a.if_node_mut(cur).codegen_node = Some(cached),
            NodeType::For => ctx.a.for_node_mut(cur).codegen_node = Some(cached),
            _ => {}
        }
    }
}
