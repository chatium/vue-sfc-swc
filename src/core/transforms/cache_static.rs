//! Port of `compiler-core/src/transforms/cacheStatic.ts` (in progress).

use crate::core::ast::*;
use crate::core::transform::TransformContext;

pub fn get_single_element_root(a: &Arena, root: NodeId) -> Option<NodeId> {
    let children: Vec<NodeId> = a
        .root(root)
        .children
        .iter()
        .copied()
        .filter(|c| !a.is(*c, NodeType::Comment))
        .collect();
    if children.len() == 1
        && a.is(children[0], NodeType::Element)
        && a.el(children[0]).tag_type != ElementType::Slot
    {
        Some(children[0])
    } else {
        None
    }
}

pub fn cache_static(_root: NodeId, _ctx: &mut TransformContext) {}
