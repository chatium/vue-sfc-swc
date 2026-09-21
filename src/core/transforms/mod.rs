pub mod cache_static;
pub mod transform_element;
pub mod transform_expression;
pub mod transform_slot_outlet;
pub mod transform_text;
pub mod v_bind;
pub mod v_bind_shorthand;
pub mod v_for;
pub mod v_if;
pub mod v_memo;
pub mod v_model;
pub mod v_on;
pub mod v_once;
pub mod v_slot;

use super::ast::NodeId;
use super::options::{DirectiveTransformKind, NodeTransformKind};
use super::transform::{
    DirectiveTransformResult, ExitFn, TransformContext, take_structural_directives,
};

pub fn apply_node_transform(
    kind: NodeTransformKind,
    node: NodeId,
    ctx: &mut TransformContext,
) -> Vec<ExitFn> {
    match kind {
        NodeTransformKind::TransformVBindShorthand => {
            v_bind_shorthand::transform_v_bind_shorthand(node, ctx);
            Vec::new()
        }
        NodeTransformKind::TransformOnce => v_once::transform_once(node, ctx),
        NodeTransformKind::TransformIf => {
            let dirs = take_structural_directives(node, ctx, |n| {
                n == "if" || n == "else" || n == "else-if"
            });
            let mut exits = Vec::new();
            for d in dirs {
                exits.extend(v_if::transform_if(node, d, ctx, true));
            }
            exits
        }
        NodeTransformKind::TransformMemo => v_memo::transform_memo(node, ctx),
        NodeTransformKind::TransformFor => {
            let dirs = take_structural_directives(node, ctx, |n| n == "for");
            let mut exits = Vec::new();
            for d in dirs {
                exits.extend(v_for::transform_for(node, d, ctx, true));
            }
            exits
        }
        NodeTransformKind::TrackVForSlotScopes => v_slot::track_v_for_slot_scopes(node, ctx),
        NodeTransformKind::TransformExpression => {
            transform_expression::transform_expression(node, ctx);
            Vec::new()
        }
        NodeTransformKind::TransformSlotOutlet => {
            transform_slot_outlet::transform_slot_outlet(node, ctx);
            Vec::new()
        }
        NodeTransformKind::TransformElement => transform_element::transform_element(node, ctx),
        NodeTransformKind::TrackSlotScopes => v_slot::track_slot_scopes(node, ctx),
        NodeTransformKind::TransformText => transform_text::transform_text(node, ctx),
        NodeTransformKind::IgnoreSideEffectTags => {
            crate::dom::transforms::ignore_side_effect_tags::ignore_side_effect_tags(node, ctx);
            Vec::new()
        }
        NodeTransformKind::TransformStyle => {
            crate::dom::transforms::transform_style::transform_style(node, ctx);
            Vec::new()
        }
        NodeTransformKind::TransformTransition => {
            crate::dom::transforms::transition::transform_transition(node, ctx)
        }
        NodeTransformKind::ValidateHtmlNesting => {
            crate::dom::transforms::validate_html_nesting::validate_html_nesting(node, ctx);
            Vec::new()
        }
        NodeTransformKind::TransformAssetUrl => {
            crate::sfc::template::transform_asset_url::transform_asset_url(node, ctx);
            Vec::new()
        }
        NodeTransformKind::TransformSrcset => {
            crate::sfc::template::transform_srcset::transform_srcset(node, ctx);
            Vec::new()
        }
        NodeTransformKind::SsrTransformIf => {
            let dirs = take_structural_directives(node, ctx, |n| {
                n == "if" || n == "else" || n == "else-if"
            });
            for d in dirs {
                v_if::transform_if(node, d, ctx, false);
            }
            Vec::new()
        }
        NodeTransformKind::SsrTransformFor => {
            let dirs = take_structural_directives(node, ctx, |n| n == "for");
            let mut exits = Vec::new();
            for d in dirs {
                exits.extend(v_for::transform_for(node, d, ctx, false));
            }
            exits
        }
        NodeTransformKind::SsrTransformSlotOutlet => {
            crate::ssr::misc::ssr_transform_slot_outlet(node, ctx);
            Vec::new()
        }
        NodeTransformKind::SsrInjectFallthroughAttrs => {
            crate::ssr::inject::ssr_inject_fallthrough_attrs(node, ctx);
            Vec::new()
        }
        NodeTransformKind::SsrInjectCssVars => {
            crate::ssr::inject::ssr_inject_css_vars(node, ctx);
            Vec::new()
        }
        NodeTransformKind::SsrTransformElement => {
            let is_plain = ctx.a.is(node, crate::core::ast::NodeType::Element)
                && ctx.a.el(node).tag_type == crate::core::ast::ElementType::Element;
            if is_plain {
                vec![ExitFn::SsrElement { node }]
            } else {
                Vec::new()
            }
        }
        NodeTransformKind::SsrTransformComponent => {
            let is_component = ctx.a.is(node, crate::core::ast::NodeType::Element)
                && ctx.a.el(node).tag_type == crate::core::ast::ElementType::Component;
            if !is_component {
                return Vec::new();
            }
            use crate::ssr::component::ComponentExit;
            match crate::ssr::component::ssr_transform_component(node, ctx) {
                ComponentExit::Component => vec![ExitFn::SsrComponent { node }],
                ComponentExit::Suspense => vec![ExitFn::SsrSuspense { node }],
                ComponentExit::TransitionGroup => vec![ExitFn::SsrTransitionGroup { node }],
                ComponentExit::Transition => vec![ExitFn::SsrTransition { node }],
                ComponentExit::None => Vec::new(),
            }
        }
    }
}

pub fn run_exit(exit: ExitFn, ctx: &mut TransformContext) {
    match exit {
        ExitFn::Once { node } => v_once::exit_once(node, ctx),
        ExitFn::IfRoot {
            if_node,
            branch,
            key,
        } => v_if::exit_if_root(if_node, branch, key, ctx),
        ExitFn::Memo { node, dir } => v_memo::exit_memo(node, dir, ctx),
        ExitFn::For {
            for_node,
            node,
            render_exp,
            key_property,
            key_exp,
            memo,
            is_stable_fragment,
            is_template,
            value,
            key,
            index,
        } => v_for::exit_for(
            for_node,
            node,
            render_exp,
            key_property,
            key_exp,
            memo,
            is_stable_fragment,
            is_template,
            value,
            key,
            index,
            ctx,
        ),
        ExitFn::Element { node } => transform_element::exit_element(node, ctx),
        ExitFn::SlotScopes { slot_props } => v_slot::exit_slot_scopes(slot_props, ctx),
        ExitFn::VForSlotScopes { value, key, index } => {
            v_slot::exit_v_for_slot_scopes(value, key, index, ctx)
        }
        ExitFn::Text { node } => transform_text::exit_text(node, ctx),
        ExitFn::Transition { node } => {
            crate::dom::transforms::transition::exit_transition(node, ctx)
        }
        ExitFn::ForTeardown { value, key, index } => {
            ctx.scopes.v_for -= 1;
            if ctx.opts.prefix_identifiers {
                for id in [value, key, index].into_iter().flatten() {
                    ctx.remove_identifiers(id);
                }
            }
        }
        ExitFn::SsrElement { node } => crate::ssr::element::ssr_transform_element_exit(node, ctx),
        ExitFn::SsrComponent { node } => {
            crate::ssr::component::ssr_transform_component_exit(node, ctx)
        }
        ExitFn::SsrSuspense { node } => crate::ssr::misc::ssr_transform_suspense_exit(node, ctx),
        ExitFn::SsrTransitionGroup { node } => {
            crate::ssr::misc::ssr_transform_transition_group_exit(node, ctx)
        }
        ExitFn::SsrTransition { node } => {
            crate::ssr::misc::ssr_transform_transition_exit(node, ctx)
        }
    }
}

pub fn apply_directive_transform(
    kind: DirectiveTransformKind,
    dir: NodeId,
    node: NodeId,
    ctx: &mut TransformContext,
) -> DirectiveTransformResult {
    match kind {
        DirectiveTransformKind::Bind => v_bind::transform_bind(dir, ctx),
        DirectiveTransformKind::On => v_on::transform_on(dir, node, ctx, None),
        DirectiveTransformKind::DomOn => crate::dom::transforms::v_on::transform_on(dir, node, ctx),
        DirectiveTransformKind::Model => v_model::transform_model(dir, node, ctx),
        DirectiveTransformKind::DomModel => {
            crate::dom::transforms::v_model::transform_model(dir, node, ctx)
        }
        DirectiveTransformKind::Html => crate::dom::transforms::v_html::transform_v_html(dir, node, ctx),
        DirectiveTransformKind::Text => crate::dom::transforms::v_text::transform_v_text(dir, node, ctx),
        DirectiveTransformKind::Show => crate::dom::transforms::v_show::transform_show(dir, ctx),
        DirectiveTransformKind::SsrShow => crate::ssr::v_show::ssr_transform_show(dir, ctx),
        DirectiveTransformKind::SsrModel => {
            let r = crate::ssr::v_model::ssr_transform_model(dir, node, ctx);
            DirectiveTransformResult {
                props: r.props,
                need_runtime: None,
            }
        }
        DirectiveTransformKind::Cloak | DirectiveTransformKind::Noop => {
            DirectiveTransformResult {
                props: Vec::new(),
                need_runtime: None,
            }
        }
    }
}
