//! Port of `compiler-core/src/transforms/vMemo.ts`.

use crate::core::ast::*;
use crate::core::transform::{ExitFn, TransformContext, convert_to_block};
use crate::core::utils::find_dir;

pub fn transform_memo(node: NodeId, ctx: &mut TransformContext) -> Vec<ExitFn> {
    if !ctx.a.is(node, NodeType::Element) {
        return Vec::new();
    }
    let dir = match find_dir(&ctx.a, node, "memo", false) {
        Some(d) => d,
        None => return Vec::new(),
    };
    if ctx.seen_memo.contains(&node) || ctx.opts.in_ssr {
        return Vec::new();
    }
    ctx.seen_memo.insert(node);
    let exp = ctx.a.dir(dir).exp.unwrap();
    vec![ExitFn::Memo { node, exp }]
}

pub fn exit_memo(node: NodeId, exp: NodeId, ctx: &mut TransformContext) {
    let codegen_node = ctx
        .a
        .el(node)
        .codegen_node
        .or_else(|| ctx.current_node.and_then(|c| ctx.a.el(c).codegen_node));
    let codegen_node = match codegen_node {
        Some(c) if ctx.a.is(c, NodeType::VNodeCall) => c,
        _ => return,
    };
    if ctx.a.el(node).tag_type != ElementType::Component {
        convert_to_block(codegen_node, ctx);
    }
    let helper = ctx.helper_node(RuntimeHelper::WITH_MEMO);
    let func = ctx
        .a
        .create_function_expression(None, Some(codegen_node), false, false, loc_stub());
    let cache_str = ctx.a.string("_cache");
    let idx = ctx.a.string(ctx.cached.len().to_string());
    let call = ctx
        .a
        .create_call_expression(helper, vec![exp, func, cache_str, idx]);
    ctx.a.el_mut(node).codegen_node = Some(call);
    ctx.cached.push(None);
}
