//! `ssrCodegenTransform` and the string-buffer context it walks with.

use crate::core::ast::*;
use crate::core::transform::TransformContext;
use crate::core::utils::is_text_node;
use crate::dom::attrs::escape_html;

use super::{component, element, misc, ssr_error};

/// `SSRTransformContext`
pub struct SsrCtx {
    pub body: Vec<NodeId>,
    /// the `TemplateLiteral` currently being appended to
    current_string: Option<NodeId>,
    pub with_slot_scope_id: bool,
}

impl SsrCtx {
    pub fn new(with_slot_scope_id: bool) -> Self {
        SsrCtx {
            body: Vec::new(),
            current_string: None,
            with_slot_scope_id,
        }
    }

    pub fn child(&self, with_slot_scope_id: bool) -> Self {
        SsrCtx::new(with_slot_scope_id)
    }

    fn open_string(&mut self, a: &mut Arena) -> NodeId {
        if let Some(s) = self.current_string {
            return s;
        }
        let push = a.string("_push");
        let call = a.create_call_expression(push, Vec::new());
        self.body.push(call);
        let lit = a.add(Node::TemplateLiteral(Vec::new()));
        a.call_mut(call).arguments.push(lit);
        self.current_string = Some(lit);
        lit
    }

    pub fn push_str_part(&mut self, a: &mut Arena, part: &str) {
        let lit = self.open_string(a);
        let last = match a.node(lit) {
            Node::TemplateLiteral(e) => e.last().copied(),
            _ => unreachable!(),
        };
        if let Some(last) = last {
            if let Node::Str(s) = a.node(last) {
                let merged = format!("{s}{part}");
                *a.node_mut(last) = Node::Str(merged);
                return;
            }
        }
        let n = a.add(Node::Str(part.to_string()));
        match a.node_mut(lit) {
            Node::TemplateLiteral(e) => e.push(n),
            _ => unreachable!(),
        }
    }

    pub fn push_node_part(&mut self, a: &mut Arena, part: NodeId) {
        if let Node::Str(s) = a.node(part) {
            let s = s.clone();
            return self.push_str_part(a, &s);
        }
        let lit = self.open_string(a);
        match a.node_mut(lit) {
            Node::TemplateLiteral(e) => e.push(part),
            _ => unreachable!(),
        }
    }

    pub fn push_statement(&mut self, statement: NodeId) {
        self.current_string = None;
        self.body.push(statement);
    }
}

/// What `processChildren` was handed as `parent`: a real node, or one of
/// `ssrTransformComponent`'s WIP slot entries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Parent {
    Node(NodeId),
    /// carries the slot's children array
    WipSlot(NodeId),
}

impl Parent {
    pub fn children(self, a: &Arena) -> Vec<NodeId> {
        match self {
            Parent::Node(n) => a.list(n).clone(),
            Parent::WipSlot(list) => a.list(list).clone(),
        }
    }
}

pub fn ssr_codegen_transform(root: NodeId, ctx: &mut TransformContext) {
    let mut sctx = SsrCtx::new(false);

    if !ctx.opts.ssr_css_vars.is_empty() {
        let raw = ctx.opts.ssr_css_vars.clone();
        let exp = ctx.a.simple_exp(raw, false);
        let vars_exp =
            crate::core::transforms::transform_expression::process_expression(
                exp, ctx, false, false, None,
            );
        let pre = ctx.a.string("const _cssVars = { style: ");
        let post = ctx.a.string("}");
        let compound = ctx
            .a
            .create_compound_expression(vec![pre, vars_exp, post], loc_stub());
        sctx.body.push(compound);
    }

    let children = ctx.a.root(root).children.clone();
    let is_fragment = children.len() > 1 && children.iter().any(|c| !is_text_node(&ctx.a, *c));
    process_children(
        Parent::Node(root),
        &mut sctx,
        ctx,
        is_fragment,
        false,
        false,
    );
    let block = ctx.a.add(Node::BlockStatement(sctx.body));
    ctx.a.root_mut(root).codegen_node = Some(block);

    // split the collected helpers into the two import sources
    let mut ssr_helpers: Vec<RuntimeHelper> = ctx
        .helpers
        .iter()
        .map(|(h, _)| *h)
        .filter(|h| h.is_ssr())
        .collect();
    for h in ctx.ssr_state.helpers.clone() {
        if !ssr_helpers.contains(&h) {
            ssr_helpers.push(h);
        }
    }
    ctx.helpers.retain(|(h, _)| !h.is_ssr());
    let helpers: Vec<RuntimeHelper> = ctx.helpers.iter().map(|(h, _)| *h).collect();
    let temps = ctx.temps;
    let r = ctx.a.root_mut(root);
    r.helpers = helpers;
    r.temps = temps;
    r.ssr_helpers = ssr_helpers;
}

pub fn process_children(
    parent: Parent,
    sctx: &mut SsrCtx,
    ctx: &mut TransformContext,
    as_fragment: bool,
    disable_nested_fragments: bool,
    disable_comment: bool,
) {
    if as_fragment {
        sctx.push_str_part(&mut ctx.a, "<!--[-->");
    }
    let children = parent.children(&ctx.a);
    for child in children {
        match ctx.a.node_type(child) {
            NodeType::Element => match ctx.a.el(child).tag_type {
                ElementType::Element => element::ssr_process_element(child, sctx, ctx),
                ElementType::Component => {
                    component::ssr_process_component(child, sctx, ctx, parent)
                }
                ElementType::Slot => misc::ssr_process_slot_outlet(child, sctx, ctx),
                ElementType::Template => {}
            },
            NodeType::Text => {
                let content = escape_html(&ctx.a.text(child).content);
                sctx.push_str_part(&mut ctx.a, &content);
            }
            NodeType::Comment => {
                if !disable_comment {
                    let content = format!("<!--{}-->", ctx.a.comment(child).content);
                    sctx.push_str_part(&mut ctx.a, &content);
                }
            }
            NodeType::Interpolation => {
                let content = ctx.a.interp(child).content;
                let helper = super::ssr_helper(ctx, RuntimeHelper::SSR_INTERPOLATE);
                let call = ctx.a.create_call_expression(helper, vec![content]);
                sctx.push_node_part(&mut ctx.a, call);
            }
            NodeType::If => misc::ssr_process_if(
                child,
                sctx,
                ctx,
                disable_nested_fragments,
                disable_comment,
            ),
            NodeType::For => misc::ssr_process_for(child, sctx, ctx, disable_nested_fragments),
            NodeType::IfBranch | NodeType::TextCall | NodeType::CompoundExpression => {}
            _ => {
                let loc = ctx.a.loc(child).clone();
                ssr_error(ctx, 67, Some(loc));
                return;
            }
        }
    }
    if as_fragment {
        sctx.push_str_part(&mut ctx.a, "<!--]-->");
    }
}

pub fn process_children_as_statement(
    parent: Parent,
    sctx: &SsrCtx,
    ctx: &mut TransformContext,
    as_fragment: bool,
    with_slot_scope_id: bool,
) -> NodeId {
    let mut child = sctx.child(with_slot_scope_id);
    process_children(parent, &mut child, ctx, as_fragment, false, false);
    ctx.a.add(Node::BlockStatement(child.body))
}
