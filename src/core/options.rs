//! Port of `compiler-core/src/options.ts` (the option surface we support).

use std::collections::HashMap;
use std::sync::Arc;

use super::ast::RuntimeHelper;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingType {
    Data,
    Props,
    PropsAliased,
    SetupLet,
    SetupConst,
    SetupReactiveConst,
    SetupMaybeRef,
    SetupRef,
    Options,
    LiteralConst,
}

impl BindingType {
    pub fn as_str(self) -> &'static str {
        match self {
            BindingType::Data => "data",
            BindingType::Props => "props",
            BindingType::PropsAliased => "props-aliased",
            BindingType::SetupLet => "setup-let",
            BindingType::SetupConst => "setup-const",
            BindingType::SetupReactiveConst => "setup-reactive-const",
            BindingType::SetupMaybeRef => "setup-maybe-ref",
            BindingType::SetupRef => "setup-ref",
            BindingType::Options => "options",
            BindingType::LiteralConst => "literal-const",
        }
    }

    pub fn from_str(s: &str) -> Option<BindingType> {
        Some(match s {
            "data" => BindingType::Data,
            "props" => BindingType::Props,
            "props-aliased" => BindingType::PropsAliased,
            "setup-let" => BindingType::SetupLet,
            "setup-const" => BindingType::SetupConst,
            "setup-reactive-const" => BindingType::SetupReactiveConst,
            "setup-maybe-ref" => BindingType::SetupMaybeRef,
            "setup-ref" => BindingType::SetupRef,
            "options" => BindingType::Options,
            "literal-const" => BindingType::LiteralConst,
            _ => return None,
        })
    }
}

/// `BindingMetadata` — the `__isScriptSetup` / `__propsAliases` specials are
/// separate fields rather than magic keys.
#[derive(Debug, Clone, Default)]
pub struct BindingMetadata {
    pub bindings: HashMap<String, BindingType>,
    /// JS `undefined` when the object was never produced by `<script setup>`
    pub is_script_setup: Option<bool>,
    pub props_aliases: HashMap<String, String>,
}

impl BindingMetadata {
    pub fn get(&self, key: &str) -> Option<BindingType> {
        self.bindings.get(key).copied()
    }
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty() && self.is_script_setup.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeTransformKind {
    // compiler-dom
    IgnoreSideEffectTags,
    TransformStyle,
    TransformTransition,
    ValidateHtmlNesting,
    // compiler-core
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
    // compiler-sfc
    TransformAssetUrl,
    TransformSrcset,
    // compiler-ssr
    SsrTransformIf,
    SsrTransformFor,
    SsrTransformSlotOutlet,
    SsrInjectFallthroughAttrs,
    SsrInjectCssVars,
    SsrTransformElement,
    SsrTransformComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveTransformKind {
    Bind,
    Cloak,
    Html,
    Model,
    DomModel,
    On,
    DomOn,
    Show,
    Text,
    Noop,
    SsrModel,
    SsrShow,
}

pub type TagPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct TransformOptions {
    pub filename: String,
    pub prefix_identifiers: bool,
    pub hoist_static: bool,
    pub hmr: bool,
    pub cache_handlers: bool,
    pub node_transforms: Vec<NodeTransformKind>,
    pub directive_transforms: HashMap<String, DirectiveTransformKind>,
    /// `transformHoist` — only ever `stringifyStatic` in compiler-dom
    pub transform_hoist: bool,
    pub is_built_in_component: Option<fn(&str) -> Option<RuntimeHelper>>,
    pub is_custom_element: Option<TagPredicate>,
    pub expression_plugins: Vec<String>,
    pub scope_id: Option<String>,
    pub slotted: bool,
    pub ssr: bool,
    pub in_ssr: bool,
    pub ssr_css_vars: String,
    pub binding_metadata: BindingMetadata,
    pub inline: bool,
    pub is_ts: bool,
    pub asset_url_options: crate::sfc::template::transform_asset_url::AssetUrlOptions,
}

impl Default for TransformOptions {
    fn default() -> Self {
        TransformOptions {
            filename: String::new(),
            prefix_identifiers: false,
            hoist_static: false,
            hmr: false,
            cache_handlers: false,
            node_transforms: Vec::new(),
            directive_transforms: HashMap::new(),
            transform_hoist: false,
            is_built_in_component: None,
            is_custom_element: None,
            expression_plugins: Vec::new(),
            scope_id: None,
            slotted: true,
            ssr: false,
            in_ssr: false,
            ssr_css_vars: String::new(),
            binding_metadata: BindingMetadata::default(),
            inline: false,
            is_ts: false,
            asset_url_options: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodegenMode {
    Function,
    Module,
}

#[derive(Debug, Clone)]
pub struct CodegenOptions {
    pub mode: CodegenMode,
    pub prefix_identifiers: bool,
    pub source_map: bool,
    pub filename: String,
    pub scope_id: Option<String>,
    pub optimize_imports: bool,
    pub runtime_module_name: String,
    pub ssr_runtime_module_name: String,
    pub runtime_global_name: String,
    pub ssr: bool,
    pub in_ssr: bool,
    pub is_ts: bool,
    pub inline: bool,
    /// `options.bindingMetadata` being present at all (it adds render args)
    pub has_binding_metadata: bool,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        CodegenOptions {
            mode: CodegenMode::Function,
            prefix_identifiers: false,
            source_map: false,
            filename: "template.vue.html".to_string(),
            scope_id: None,
            optimize_imports: false,
            runtime_module_name: "vue".to_string(),
            ssr_runtime_module_name: "vue/server-renderer".to_string(),
            runtime_global_name: "Vue".to_string(),
            ssr: false,
            in_ssr: false,
            is_ts: false,
            inline: false,
            has_binding_metadata: false,
        }
    }
}
