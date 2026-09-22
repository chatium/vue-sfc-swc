//! Port of `compiler-sfc/src/compileTemplate.ts`.
//!
//! Source maps are not produced (see `core::codegen`).

use crate::core::ast::{Arena, NodeId};
use crate::core::errors::CompilerError;
use crate::core::options::*;
use crate::dom::compile::{CompileOptions, compile, compile_ast};

use super::css_vars::gen_css_vars_from_list;

#[derive(Debug, Clone, Default)]
pub struct TemplateCompileOptions {
    pub filename: String,
    pub id: String,
    pub scoped: bool,
    /// `slotted` defaults to true in the transform context
    pub slotted: Option<bool>,
    pub is_prod: bool,
    pub ssr: bool,
    pub ssr_css_vars: Vec<String>,
    // compilerOptions passthrough
    pub binding_metadata: BindingMetadata,
    /// whether `compilerOptions.bindingMetadata` was passed at all — codegen
    /// adds the `$props, $setup, $data, $options` render args when it was
    pub binding_metadata_provided: bool,
    pub expression_plugins: Vec<String>,
    pub inline: bool,
    pub is_ts: bool,
    /// disables the asset-url transforms (`transformAssetUrls: false`)
    pub no_asset_urls: bool,
}

pub struct TemplateCompileResult {
    pub code: String,
    pub preamble: String,
    pub errors: Vec<CompilerError>,
    pub tips: Vec<String>,
    pub arena: Arena,
    pub root: NodeId,
}

fn build_options(o: &TemplateCompileOptions) -> CompileOptions {
    let short_id = o.id.strip_prefix("data-v-").unwrap_or(&o.id).to_string();
    let long_id = format!("data-v-{short_id}");

    let mut opts = CompileOptions::default();
    opts.transform.prefix_identifiers = true;
    opts.transform.hoist_static = true;
    opts.transform.cache_handlers = true;
    opts.transform.filename = o.filename.clone();
    opts.transform.scope_id = if o.scoped { Some(long_id) } else { None };
    if let Some(s) = o.slotted {
        opts.transform.slotted = s;
    }
    opts.transform.ssr_css_vars = if o.ssr && !o.ssr_css_vars.is_empty() {
        gen_css_vars_from_list(&o.ssr_css_vars, &short_id, o.is_prod, true)
    } else {
        String::new()
    };
    opts.transform.binding_metadata = o.binding_metadata.clone();
    opts.transform.inline = o.inline;
    opts.transform.is_ts = o.is_ts;
    opts.transform.expression_plugins = o.expression_plugins.clone();
    opts.transform.hmr = !o.is_prod;
    opts.transform.ssr = o.ssr;
    opts.transform.in_ssr = o.ssr;

    opts.codegen.mode = CodegenMode::Module;
    opts.codegen.prefix_identifiers = true;
    opts.codegen.filename = o.filename.clone();
    opts.codegen.scope_id = opts.transform.scope_id.clone();
    opts.codegen.inline = o.inline;
    opts.codegen.is_ts = o.is_ts;
    opts.codegen.ssr = o.ssr;
    opts.codegen.in_ssr = o.ssr;
    opts.codegen.has_binding_metadata = o.binding_metadata_provided;

    if !o.no_asset_urls {
        opts.extra_node_transforms = vec![
            NodeTransformKind::TransformAssetUrl,
            NodeTransformKind::TransformSrcset,
        ];
    }
    opts
}

pub fn compile_template(source: &str, o: TemplateCompileOptions) -> TemplateCompileResult {
    let opts = build_options(&o);
    let r = if o.ssr {
        crate::ssr::compile(source, opts)
    } else {
        compile(source, opts)
    };
    TemplateCompileResult {
        code: r.code,
        preamble: r.preamble,
        errors: r.errors,
        tips: r.warnings.into_iter().map(|w| w.message).collect(),
        arena: r.arena,
        root: r.root,
    }
}

/// `compileTemplate` with the descriptor's already-parsed template block.
pub fn compile_template_ast(
    arena: Arena,
    children: Vec<NodeId>,
    source: String,
    parse_errors: Vec<CompilerError>,
    o: TemplateCompileOptions,
) -> TemplateCompileResult {
    let mut arena = arena;
    let root = arena.create_root(children, source);
    let opts = build_options(&o);
    let r = if o.ssr {
        crate::ssr::compile_ast(arena, root, parse_errors, opts)
    } else {
        compile_ast(arena, root, parse_errors, opts)
    };
    TemplateCompileResult {
        code: r.code,
        preamble: r.preamble,
        errors: r.errors,
        tips: r.warnings.into_iter().map(|w| w.message).collect(),
        arena: r.arena,
        root: r.root,
    }
}

/// Re-parses the SFC and returns the `<template>` block's children, matching
/// what `compileTemplate` does when the descriptor AST was already transformed.
pub fn reparse_template(
    source: &str,
) -> Option<(Arena, Vec<NodeId>, Vec<CompilerError>)> {
    use crate::core::ast::{Node, NodeType};
    let (arena, root, errors) = crate::dom::compile::parse_sfc_template(source);
    let children = arena.root(root).children.clone();
    for c in children {
        if arena.is(c, NodeType::Element) {
            if let Node::Element(e) = arena.node(c) {
                if e.tag == "template" {
                    let inner = e.children.clone();
                    return Some((arena, inner, errors));
                }
            }
        }
    }
    None
}
