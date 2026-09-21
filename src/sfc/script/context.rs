//! Port of `compiler-sfc/src/script/context.ts`.

use std::collections::{HashMap, HashSet};

use swc_core::ecma::ast::*;

use crate::core::options::{BindingMetadata, BindingType};
use crate::sfc::magic_string::MagicString;
use crate::sfc::parse::SfcDescriptor;

#[derive(Debug, Clone, Default)]
pub struct ScriptCompileOptions {
    pub id: String,
    pub is_prod: bool,
    pub inline_template: bool,
    pub gen_default_as: Option<String>,
    pub hoist_static: Option<bool>,
    pub props_destructure: Option<bool>,
    pub custom_element: bool,
    /// `templateOptions.ssr`
    pub template_ssr: bool,
}

#[derive(Debug, Clone)]
pub struct ImportBinding {
    pub is_type: bool,
    pub imported: String,
    pub local: String,
    pub source: String,
    pub is_from_setup: bool,
    pub is_used_in_template: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ModelDecl {
    pub type_ann: Option<Box<TsType>>,
    pub options: Option<String>,
    pub identifier: Option<String>,
    pub runtime_option_spans: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Default)]
pub struct PropsDestructureBinding {
    pub local: String,
    pub default: Option<Box<Expr>>,
}

pub struct ScriptCompileContext {
    pub is_js: bool,
    pub is_ts: bool,
    pub is_ce: bool,

    pub script_ast: Option<Module>,
    pub script_setup_ast: Option<Module>,

    pub source: String,
    pub filename: String,
    pub s: MagicString,
    /// byte offsets into `source`
    pub start_offset: usize,
    pub end_offset: usize,

    pub user_imports: Vec<(String, ImportBinding)>,

    pub has_define_props_call: bool,
    pub has_define_emit_call: bool,
    pub has_define_expose_call: bool,
    pub has_default_export_name: bool,
    pub has_default_export_render: bool,
    pub has_define_options_call: bool,
    pub has_define_slots_call: bool,
    pub has_define_model_call: bool,

    pub props_call: Option<(usize, usize)>,
    pub props_decl: Option<Pat>,
    pub props_runtime_decl: Option<Box<Expr>>,
    pub props_type_decl: Option<Box<TsType>>,
    pub props_destructure_decl: Option<ObjectPat>,
    pub props_destructured_bindings: Vec<(String, PropsDestructureBinding)>,
    pub props_destructure_rest_id: Option<String>,
    pub props_runtime_defaults: Option<Box<Expr>>,

    pub emits_runtime_decl: Option<Box<Expr>>,
    pub emits_type_decl: Option<Box<TsType>>,
    pub emit_decl: Option<Pat>,

    pub model_decls: Vec<(String, ModelDecl)>,
    pub options_runtime_decl: Option<Box<Expr>>,

    pub binding_metadata: BindingMetadata,
    pub helper_imports: Vec<String>,

    pub options: ScriptCompileOptions,
    pub descriptor: SfcDescriptor,
    /// local type declarations available to `resolveType`
    pub type_decls: HashMap<String, TypeDecl>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub seen_helpers: HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum TypeDecl {
    Interface(Box<TsInterfaceDecl>),
    Alias(Box<TsTypeAliasDecl>),
    Enum(Box<TsEnumDecl>),
}

/// `utf16` offsets from the SFC parser -> byte offsets for MagicString
pub fn utf16_to_byte(source: &str, utf16_offset: usize) -> usize {
    if utf16_offset == 0 {
        return 0;
    }
    let mut units = 0usize;
    for (byte_idx, ch) in source.char_indices() {
        if units >= utf16_offset {
            return byte_idx;
        }
        units += ch.len_utf16();
    }
    source.len()
}

impl ScriptCompileContext {
    pub fn new(
        descriptor: &SfcDescriptor,
        options: ScriptCompileOptions,
    ) -> Result<Self, String> {
        let script_lang = descriptor.script.as_ref().and_then(|s| s.lang.clone());
        let setup_lang = descriptor
            .script_setup
            .as_ref()
            .and_then(|s| s.lang.clone());
        let is_js = is_js(&script_lang) || is_js(&setup_lang);
        let is_ts = is_ts(&script_lang) || is_ts(&setup_lang);

        let source = descriptor.source.clone();
        let start_offset = descriptor
            .script_setup
            .as_ref()
            .map(|s| utf16_to_byte(&source, s.loc.start.offset.max(0) as usize))
            .unwrap_or(0);
        let end_offset = descriptor
            .script_setup
            .as_ref()
            .map(|s| utf16_to_byte(&source, s.loc.end.offset.max(0) as usize))
            .unwrap_or(0);

        // `resolveParserPlugins` only enables TypeScript for ts/tsx blocks
        let parse = |content: &str| -> Result<Module, String> {
            crate::core::jsparse::parse_module(content, is_ts)
        };

        let script_ast = match &descriptor.script {
            Some(s) => Some(parse(&s.content)?),
            None => None,
        };
        let script_setup_ast = match &descriptor.script_setup {
            Some(s) => Some(parse(&s.content)?),
            None => None,
        };

        Ok(ScriptCompileContext {
            is_js,
            is_ts,
            is_ce: options.custom_element,
            script_ast,
            script_setup_ast,
            s: MagicString::new(&source),
            filename: descriptor.filename.clone(),
            source,
            start_offset,
            end_offset,
            user_imports: Vec::new(),
            has_define_props_call: false,
            has_define_emit_call: false,
            has_define_expose_call: false,
            has_default_export_name: false,
            has_default_export_render: false,
            has_define_options_call: false,
            has_define_slots_call: false,
            has_define_model_call: false,
            props_call: None,
            props_decl: None,
            props_runtime_decl: None,
            props_type_decl: None,
            props_destructure_decl: None,
            props_destructured_bindings: Vec::new(),
            props_destructure_rest_id: None,
            props_runtime_defaults: None,
            emits_runtime_decl: None,
            emits_type_decl: None,
            emit_decl: None,
            model_decls: Vec::new(),
            options_runtime_decl: None,
            binding_metadata: BindingMetadata::default(),
            helper_imports: Vec::new(),
            options,
            descriptor: descriptor.clone(),
            type_decls: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            seen_helpers: HashSet::new(),
        })
    }

    pub fn helper(&mut self, key: &str) -> String {
        if !self.helper_imports.iter().any(|h| h == key) {
            self.helper_imports.push(key.to_string());
        }
        format!("_{key}")
    }

    /// `ctx.getString(node)` — slices the `<script setup>` (or `<script>`) block
    pub fn get_string(&self, start: usize, end: usize, script_setup: bool) -> String {
        let block = if script_setup {
            self.descriptor.script_setup.as_ref()
        } else {
            self.descriptor.script.as_ref()
        };
        match block {
            Some(b) => b.content[start.min(b.content.len())..end.min(b.content.len())].to_string(),
            None => String::new(),
        }
    }

    pub fn error(&self, msg: &str) -> String {
        format!("[@vue/compiler-sfc] {msg}\n\n{}\n", self.filename)
    }

    pub fn set_binding(&mut self, key: &str, ty: BindingType) {
        self.binding_metadata
            .bindings
            .insert(key.to_string(), ty);
    }
}

pub fn is_js(lang: &Option<String>) -> bool {
    matches!(lang.as_deref(), Some("js") | Some("jsx"))
}

pub fn is_ts(lang: &Option<String>) -> bool {
    matches!(lang.as_deref(), Some("ts") | Some("tsx"))
}
