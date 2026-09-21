pub mod cache_static;
pub mod transform_expression;

use super::ast::NodeId;
use super::options::NodeTransformKind;
use super::transform::{ExitFn, TransformContext};

pub fn apply_node_transform(
    kind: NodeTransformKind,
    node: NodeId,
    ctx: &mut TransformContext,
) -> Vec<ExitFn> {
    match kind {
        NodeTransformKind::TransformExpression => {
            transform_expression::transform_expression(node, ctx);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub fn run_exit(_exit: ExitFn, _ctx: &mut TransformContext) {}
