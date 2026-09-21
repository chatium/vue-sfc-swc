//! Port of `@vue/compiler-ssr`.
//!
//! The JS package keeps its per-node bookkeeping in module-level `WeakMap`s
//! that outlive the transform pass; here they live in [`SsrState`], which
//! hangs off the transform context and is read back by the codegen pass.

pub mod codegen;
pub mod component;
pub mod element;
pub mod inject;
pub mod misc;
pub mod v_model;
pub mod v_show;

use std::collections::HashMap;

use crate::core::ast::*;
use crate::core::ast::SourceLocation;
use crate::core::errors::CompilerError;
use crate::core::transform::TransformContext;
use crate::core::transforms::v_slot::SlotFnKind;

/// `wipEntries` of `ssrTransformComponent`
#[derive(Debug, Clone)]
pub struct WipSlot {
    pub fn_id: NodeId,
    /// the slot's children array
    pub children: NodeId,
    /// the matching vnode branch built from the cloned node
    pub vnode_branch: Option<NodeId>,
}

/// `ssrTransformSuspense`'s `wipEntry`
#[derive(Debug, Clone, Default)]
pub struct WipSuspense {
    pub slots_exp: Option<NodeId>,
    pub wip_slots: Vec<(NodeId, NodeId)>,
}

#[derive(Debug, Clone)]
pub struct WipTransitionGroup {
    pub tag: NodeId,
    pub props_exp: Option<NodeId>,
    pub scope_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct SsrState {
    /// `rawChildrenMap`
    pub raw_children: HashMap<NodeId, NodeId>,
    /// `componentTypeMap`
    pub component_type: HashMap<NodeId, NodeId>,
    /// `clonedNode`, taken before the children are transformed
    pub component_clone: HashMap<NodeId, NodeId>,
    /// `wipMap` of `ssrTransformComponent`
    pub component_slots: HashMap<NodeId, Vec<WipSlot>>,
    /// `wipMap$3` of `ssrTransformSuspense`
    pub suspense: HashMap<NodeId, WipSuspense>,
    /// `wipMap$2` of `ssrTransformTransitionGroup`
    pub transition_group: HashMap<NodeId, WipTransitionGroup>,
    /// `wipMap$1` of `ssrTransformTransition`
    pub transition_appear: HashMap<NodeId, bool>,
    /// collected while a `buildSlots` call is in flight
    pub pending_slots: Vec<WipSlot>,
    pub pending_suspense_slots: Vec<(NodeId, NodeId)>,
    pub pending_vnode_branches: Vec<NodeId>,
    /// helpers imported from `vue/server-renderer`
    pub helpers: Vec<RuntimeHelper>,
    /// the options `compile()` was called with, for `createVNodeSlotBranch`
    pub raw_node_transforms: Vec<crate::core::options::NodeTransformKind>,
    pub raw_directive_transforms:
        HashMap<String, crate::core::options::DirectiveTransformKind>,
}

pub fn ssr_helper(ctx: &mut TransformContext, h: RuntimeHelper) -> NodeId {
    if !ctx.ssr_state.helpers.contains(&h) {
        ctx.ssr_state.helpers.push(h);
    }
    ctx.a.add(Node::Sym(h))
}

pub fn ssr_error(ctx: &mut TransformContext, code: i32, loc: Option<SourceLocation>) {
    let message = match code {
        65 => "Unsafe attribute name for SSR.",
        66 => "Missing the 'to' prop on teleport element.",
        _ => "Invalid AST node during SSR transform.",
    };
    ctx.errors.push(CompilerError {
        code,
        message: message.to_string(),
        loc,
    });
}

/// The SSR halves of `buildSlotFn`, dispatched from `build_slots`.
pub fn build_ssr_slot_fn(
    kind: SlotFnKind,
    props: Option<NodeId>,
    v_for: Option<NodeId>,
    children: NodeId,
    loc: SourceLocation,
    ctx: &mut TransformContext,
) -> NodeId {
    match kind {
        SlotFnKind::SsrSuspense => {
            let params = ctx.a.nodes(Vec::new());
            let f = ctx
                .a
                .create_function_expression(Some(params), None, true, false, loc);
            ctx.ssr_state.pending_suspense_slots.push((f, children));
            f
        }
        SlotFnKind::SsrComponent => {
            let param0 = match props {
                Some(p) => crate::core::transforms::transform_expression::stringify_expression(
                    &ctx.a, p,
                ),
                None => String::new(),
            };
            let param0 = if param0.is_empty() {
                "_".to_string()
            } else {
                param0
            };
            let params: Vec<NodeId> = ["_push", "_parent", "_scopeId"]
                .iter()
                .map(|s| ctx.a.string(*s))
                .collect();
            let p0 = ctx.a.string(param0);
            let mut all = vec![p0];
            all.extend(params);
            let params = ctx.a.nodes(all);
            let f = ctx
                .a
                .create_function_expression(Some(params), None, true, true, loc);
            let vnode_branch = ctx
                .ssr_state
                .pending_vnode_branches
                .get(ctx.ssr_state.pending_slots.len())
                .copied();
            ctx.ssr_state.pending_slots.push(WipSlot {
                fn_id: f,
                children,
                vnode_branch,
            });
            f
        }
        SlotFnKind::SsrVNodeBranch => {
            let branch = component::create_vnode_slot_branch(props, v_for, children, ctx);
            ctx.ssr_state.pending_vnode_branches.push(branch);
            ctx.a
                .create_function_expression(None, None, false, false, loc)
        }
        SlotFnKind::Client => unreachable!(),
    }
}

/// `compile(source, options)` from compiler-ssr.
pub fn compile(
    source: &str,
    options: crate::dom::compile::CompileOptions,
) -> crate::dom::compile::CompileResult {
    let mut parser_options = crate::dom::parser_options::dom_parser_options();
    parser_options.prefix_identifiers = true;
    parser_options.is_custom_element = options.transform.is_custom_element.clone();
    let parsed = crate::core::parser::base_parse(source, parser_options);
    compile_ast(parsed.arena, parsed.root, parsed.errors, options)
}

/// `compile(ast, options)` — the SFC pipeline parses the template itself.
pub fn compile_ast(
    arena: Arena,
    root: NodeId,
    parse_errors: Vec<CompilerError>,
    options: crate::dom::compile::CompileOptions,
) -> crate::dom::compile::CompileResult {
    use crate::core::options::{CodegenMode, DirectiveTransformKind, NodeTransformKind};

    let crate::dom::compile::CompileOptions {
        transform: mut t,
        codegen: mut c,
        extra_node_transforms,
    } = options;

    let po = crate::dom::parser_options::dom_parser_options();
    if t.is_built_in_component.is_none() {
        t.is_built_in_component = po.is_built_in_component;
    }
    t.ssr = true;
    t.in_ssr = true;
    t.prefix_identifiers = true;
    t.cache_handlers = false;
    t.hoist_static = false;
    c.ssr = true;
    c.in_ssr = true;
    c.prefix_identifiers = true;
    if c.mode == CodegenMode::Function {
        t.scope_id = None;
        c.scope_id = None;
    }

    use NodeTransformKind::*;
    let mut node_transforms = vec![
        TransformVBindShorthand,
        SsrTransformIf,
        SsrTransformFor,
        TrackVForSlotScopes,
        TransformExpression,
        SsrTransformSlotOutlet,
        SsrInjectFallthroughAttrs,
        SsrInjectCssVars,
        SsrTransformElement,
        SsrTransformComponent,
        TrackSlotScopes,
        TransformStyle,
    ];
    node_transforms.extend_from_slice(&extra_node_transforms);

    let mut directive_transforms = HashMap::new();
    directive_transforms.insert("bind".to_string(), DirectiveTransformKind::Bind);
    directive_transforms.insert("on".to_string(), DirectiveTransformKind::On);
    directive_transforms.insert("model".to_string(), DirectiveTransformKind::SsrModel);
    directive_transforms.insert("show".to_string(), DirectiveTransformKind::SsrShow);
    directive_transforms.insert("cloak".to_string(), DirectiveTransformKind::Noop);
    directive_transforms.insert("once".to_string(), DirectiveTransformKind::Noop);
    directive_transforms.insert("memo".to_string(), DirectiveTransformKind::Noop);

    t.node_transforms = node_transforms;
    t.directive_transforms = directive_transforms;

    let mut ctx = TransformContext::new(arena, root, t);
    ctx.errors = parse_errors;
    // `rawOptionsMap` — what `createVNodeSlotBranch` layers the vnode
    // transforms on top of
    ctx.ssr_state.raw_node_transforms = extra_node_transforms;
    ctx.ssr_state.raw_directive_transforms = HashMap::new();

    crate::core::transform::transform(&mut ctx);
    codegen::ssr_codegen_transform(root, &mut ctx);
    let result = crate::core::codegen::generate(&ctx.a, root, c);
    crate::dom::compile::CompileResult {
        code: result.code,
        preamble: result.preamble,
        errors: ctx.errors,
        warnings: ctx.warnings,
        arena: ctx.a,
        root,
    }
}
