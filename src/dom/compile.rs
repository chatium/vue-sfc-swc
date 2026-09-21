//! Port of `compiler-core/src/compile.ts` + `compiler-dom/src/index.ts#compile`.

use std::collections::HashMap;

use crate::core::ast::{Arena, NodeId};
use crate::core::codegen::{CodegenResult, generate};
use crate::core::errors::CompilerError;
use crate::core::options::*;
use crate::core::parser::{ParseMode, base_parse};
use crate::core::transform::{TransformContext, transform};

use super::parser_options::dom_parser_options;

pub struct CompileOptions {
    pub transform: TransformOptions,
    pub codegen: CodegenOptions,
    /// extra node transforms appended after the platform ones
    pub extra_node_transforms: Vec<NodeTransformKind>,
}

#[allow(clippy::derivable_impls)]
impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            transform: TransformOptions::default(),
            codegen: CodegenOptions::default(),
            extra_node_transforms: Vec::new(),
        }
    }
}

pub fn base_transform_preset() -> (Vec<NodeTransformKind>, HashMap<String, DirectiveTransformKind>)
{
    use NodeTransformKind::*;
    let nodes = vec![
        TransformVBindShorthand,
        TransformOnce,
        TransformIf,
        TransformMemo,
        TransformFor,
        TrackVForSlotScopes,
        TransformExpression,
        TransformSlotOutlet,
        TransformElement,
        TrackSlotScopes,
        TransformText,
    ];
    let mut dirs = HashMap::new();
    dirs.insert("on".to_string(), DirectiveTransformKind::On);
    dirs.insert("bind".to_string(), DirectiveTransformKind::Bind);
    dirs.insert("model".to_string(), DirectiveTransformKind::Model);
    (nodes, dirs)
}

fn dom_transforms(
    extra: &[NodeTransformKind],
) -> (Vec<NodeTransformKind>, HashMap<String, DirectiveTransformKind>) {
    use NodeTransformKind::*;
    let (mut nodes, mut dirs) = base_transform_preset();
    nodes.push(IgnoreSideEffectTags);
    nodes.push(TransformStyle);
    nodes.push(TransformTransition);
    nodes.push(ValidateHtmlNesting);
    nodes.extend_from_slice(extra);
    dirs.insert("cloak".to_string(), DirectiveTransformKind::Cloak);
    dirs.insert("html".to_string(), DirectiveTransformKind::Html);
    dirs.insert("text".to_string(), DirectiveTransformKind::Text);
    dirs.insert("model".to_string(), DirectiveTransformKind::DomModel);
    dirs.insert("on".to_string(), DirectiveTransformKind::DomOn);
    dirs.insert("show".to_string(), DirectiveTransformKind::Show);
    (nodes, dirs)
}

pub struct CompileResult {
    pub code: String,
    pub preamble: String,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerError>,
    pub arena: Arena,
    pub root: NodeId,
}

/// `compile(source, options)` from compiler-dom.
pub fn compile(source: &str, options: CompileOptions) -> CompileResult {
    let mut parser_options = dom_parser_options();
    parser_options.prefix_identifiers = options.transform.prefix_identifiers;
    parser_options.is_custom_element = options.transform.is_custom_element.clone();
    let parsed = base_parse(source, parser_options);
    compile_ast(parsed.arena, parsed.root, parsed.errors, options)
}

/// `compile(ast, options)` — used when the SFC descriptor already parsed the
/// template in SFC mode.
pub fn compile_ast(
    arena: Arena,
    root: NodeId,
    parse_errors: Vec<CompilerError>,
    options: CompileOptions,
) -> CompileResult {
    let CompileOptions {
        transform: mut transform_opts,
        codegen: mut codegen_opts,
        extra_node_transforms,
    } = options;

    // compiler-dom merges its parser options into the same options object, so
    // the transform sees isBuiltInComponent / isCustomElement too
    let po = dom_parser_options();
    if transform_opts.is_built_in_component.is_none() {
        transform_opts.is_built_in_component = po.is_built_in_component;
    }

    let (node_transforms, directive_transforms) = dom_transforms(&extra_node_transforms);
    transform_opts.node_transforms = node_transforms;
    transform_opts.directive_transforms = directive_transforms;
    transform_opts.transform_hoist = true;

    if codegen_opts.mode == CodegenMode::Module {
        transform_opts.prefix_identifiers = true;
        codegen_opts.prefix_identifiers = true;
    }

    let mut ctx = TransformContext::new(arena, root, transform_opts);
    ctx.errors = parse_errors;
    transform(&mut ctx);
    let result: CodegenResult = generate(&ctx.a, root, codegen_opts);
    CompileResult {
        code: result.code,
        preamble: result.preamble,
        errors: ctx.errors,
        warnings: ctx.warnings,
        arena: ctx.a,
        root,
    }
}

/// `parse(template, options)` from compiler-dom, in SFC mode.
pub fn parse_sfc_template(source: &str) -> (Arena, NodeId, Vec<CompilerError>) {
    let mut o = dom_parser_options();
    o.parse_mode = ParseMode::Sfc;
    o.prefix_identifiers = true;
    let r = base_parse(source, o);
    (r.arena, r.root, r.errors)
}
