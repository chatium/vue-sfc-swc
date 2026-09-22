//! Port of `compiler-sfc/src/script/context.ts`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

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

type ParsedBlocks = (Option<Arc<Module>>, Option<Arc<Module>>);

/// The parsed `<script>` and `<script setup>`. The parse depends on those
/// blocks alone, so one `ScriptAsts` can serve every `compile_script_with`
/// call over the same descriptor. It parses on first use and keeps a parse
/// error to return again, so a call that never needs the AST (a non-JS
/// `lang`) never parses.
#[derive(Default)]
pub struct ScriptAsts(OnceLock<Result<ParsedBlocks, String>>);

impl ScriptAsts {
    pub fn new() -> Self {
        Self::default()
    }

    /// `descriptor` must be the one every call pairs this with
    fn get(&self, descriptor: &SfcDescriptor) -> Result<ParsedBlocks, String> {
        self.0.get_or_init(|| parse_blocks(descriptor)).clone()
    }
}

fn parse_blocks(descriptor: &SfcDescriptor) -> Result<ParsedBlocks, String> {
    let script_lang = descriptor.script.as_ref().and_then(|s| s.lang.clone());
    let setup_lang = descriptor.script_setup.as_ref().and_then(|s| s.lang.clone());
    let is_ts = is_ts(&script_lang) || is_ts(&setup_lang);
    let source = &descriptor.source;
    // `resolveParserPlugins` enables TypeScript for ts/tsx and JSX for
    // jsx/tsx blocks
    let is_jsx = is_jsx_lang(&script_lang) || is_jsx_lang(&setup_lang);
    let parse = |content: &str, block_start: usize| -> Result<Module, String> {
        #[cfg(test)]
        tests::SCRIPT_PARSES.with(|c| c.set(c.get() + 1));
        crate::core::jsparse::parse_module_with_pos(content, is_ts, is_jsx).map_err(
            |(msg, pos)| {
                // `parse()` in compileScript re-throws babel errors with
                // the block-relative `(line:col)` and a frame over the SFC
                let (line, col) = line_col(content, pos);
                let at = byte_to_utf16(source, block_start + pos);
                format!(
                    "[vue/compiler-sfc] {msg} ({line}:{col})\n\n{}\n{}",
                    descriptor.filename,
                    crate::core::codeframe::generate_code_frame(source, at, at + 1)
                )
            },
        )
    };

    let script_ast = match &descriptor.script {
        Some(s) => {
            let start = utf16_to_byte(source, s.loc.start.offset.max(0) as usize);
            Some(Arc::new(parse(&s.content, start)?))
        }
        None => None,
    };
    let script_setup_ast = match &descriptor.script_setup {
        Some(s) => {
            let start = utf16_to_byte(source, s.loc.start.offset.max(0) as usize);
            Some(Arc::new(parse(&s.content, start)?))
        }
        None => None,
    };
    Ok((script_ast, script_setup_ast))
}

pub struct ScriptCompileContext {
    pub is_js: bool,
    pub is_ts: bool,
    pub is_ce: bool,

    pub script_ast: Option<Arc<Module>>,
    pub script_setup_ast: Option<Arc<Module>>,

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
    /// `_ownerScope`: which block each declaration's spans belong to
    pub type_decl_in_setup: HashMap<String, bool>,
    /// the block the type currently being resolved came from
    pub current_type_in_setup: bool,
    pub errors: Vec<String>,
    /// `inferRuntimeType` tolerates an unresolvable `extends`
    pub silent_on_extends_failure: bool,
    pub warnings: Vec<String>,
    pub seen_helpers: HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum TypeDecl {
    Interface(Box<TsInterfaceDecl>),
    Alias(Box<TsTypeAliasDecl>),
    Enum(Box<TsEnumDecl>),
}

/// byte offsets -> UTF-16 offsets, for code frames
pub fn byte_to_utf16(source: &str, byte: usize) -> usize {
    source[..byte.min(source.len())].encode_utf16().count()
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
        asts: &ScriptAsts,
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

        let (script_ast, script_setup_ast) = asts.get(descriptor)?;

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
            type_decl_in_setup: HashMap::new(),
            current_type_in_setup: true,
            errors: Vec::new(),
            silent_on_extends_failure: false,
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

    /// `ctx.error(msg, node)` — includes the code frame for the node's span
    pub fn error_at(&self, msg: &str, span: (usize, usize), script_setup: bool) -> String {
        let offset = if script_setup {
            self.start_offset
        } else {
            self.descriptor
                .script
                .as_ref()
                .map(|s| utf16_to_byte(&self.source, s.loc.start.offset.max(0) as usize))
                .unwrap_or(0)
        };
        let start = byte_to_utf16(&self.source, span.0 + offset);
        let end = byte_to_utf16(&self.source, span.1 + offset);
        format!(
            "[@vue/compiler-sfc] {msg}\n\n{}\n{}",
            self.filename,
            crate::core::codeframe::generate_code_frame(&self.source, start, end)
        )
    }

    /// `ctx.error(msg, node)` for a top-level `<script setup>` item
    pub fn error_at_item(&self, msg: &str, span: (usize, usize)) -> String {
        self.error_at(msg, span, true)
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

/// the langs `resolveParserPlugins` adds the `jsx` plugin for
pub fn is_jsx_lang(lang: &Option<String>) -> bool {
    matches!(lang.as_deref(), Some("jsx") | Some("tsx") | Some("mtsx"))
}

pub fn is_ts(lang: &Option<String>) -> bool {
    matches!(lang.as_deref(), Some("ts") | Some("tsx"))
}

/// 1-based line, 0-based column of `pos` (babel's error position format)
fn line_col(src: &str, pos: usize) -> (usize, usize) {
    let head = &src[..pos.min(src.len())];
    let line = head.matches('\n').count() + 1;
    let col = head.rsplit('\n').next().unwrap_or("").chars().count();
    (line, col)
}

#[cfg(test)]
mod tests {
    thread_local! {
        pub(super) static SCRIPT_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// `compile_vue` compiles the script twice (for `logic`, then inline); the
    /// blocks are the same both times, so each is parsed once
    #[test]
    fn compile_vue_parses_each_script_block_once() {
        let src = "<script>export const a = 1</script>\n\
                   <script setup>\nconst b = a\n</script>\n\
                   <template><div>{{ b }}</div></template>";
        SCRIPT_PARSES.with(|c| c.set(0));
        crate::ugc::compile_vue(src, "a.vue").unwrap();
        assert_eq!(SCRIPT_PARSES.with(|c| c.get()), 2);
    }
}
