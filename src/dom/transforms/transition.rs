//! Port of `compiler-dom/src/transforms/Transition.ts`.

use crate::core::ast::*;
use crate::core::errors::{DomErrorCode, create_dom_compiler_error};
use crate::core::transform::{ExitFn, TransformContext};
use crate::core::utils::is_comment_or_whitespace;

pub fn transform_transition(node: NodeId, ctx: &mut TransformContext) -> Vec<ExitFn> {
    if !ctx.a.is(node, NodeType::Element) || ctx.a.el(node).tag_type != ElementType::Component {
        return Vec::new();
    }
    let tag = ctx.a.el(node).tag.clone();
    let component = ctx.opts.is_built_in_component.and_then(|f| f(&tag));
    if component == Some(RuntimeHelper::TRANSITION) {
        return vec![ExitFn::Transition { node }];
    }
    Vec::new()
}

pub fn exit_transition(node: NodeId, ctx: &mut TransformContext) {
    if ctx.a.el(node).children.is_empty() {
        return;
    }
    if has_multiple_children(node, ctx) {
        let children = ctx.a.el(node).children.clone();
        let loc = SourceLocation {
            start: ctx.a.loc(children[0]).start,
            end: ctx.a.loc(*children.last().unwrap()).end,
            source: Default::default(),
        };
        let e = create_dom_compiler_error(DomErrorCode::X_TRANSITION_INVALID_CHILDREN, Some(loc));
        ctx.on_error(e);
    }

    let child = ctx.a.el(node).children[0];
    if ctx.a.is(child, NodeType::Element) {
        let props = ctx.a.el(child).props.clone();
        for p in props {
            if matches!(ctx.a.node(p), Node::Directive(d) if d.name == "show") {
                let loc = ctx.a.loc(node).clone();
                let attr = ctx.a.add(Node::Attribute(Box::new(AttributeNode {
                    name: "persisted".to_string(),
                    name_loc: loc.clone(),
                    value: None,
                    loc,
                })));
                ctx.a.el_mut(node).props.push(attr);
            }
        }
    }
}

fn has_multiple_children(node: NodeId, ctx: &mut TransformContext) -> bool {
    let children: Vec<NodeId> = ctx
        .a
        .children_of(node)
        .iter()
        .copied()
        .filter(|c| !is_comment_or_whitespace(&ctx.a, *c))
        .collect();
    *ctx.a.children_of_mut(node) = children.clone();
    if children.len() != 1 {
        return true;
    }
    let child = children[0];
    match ctx.a.node_type(child) {
        NodeType::For => true,
        NodeType::If => {
            let branches = ctx.a.if_node(child).branches.clone();
            branches.iter().any(|b| has_multiple_children(*b, ctx))
        }
        _ => false,
    }
}
