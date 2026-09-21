//! Port of `compiler-sfc/src/script/normalScript.ts`.

use crate::sfc::css_vars::gen_normal_script_css_vars_code;
use crate::sfc::magic_string::MagicString;
use crate::sfc::parse::SfcBlock;
use crate::sfc::rewrite_default::rewrite_default_ast;

use super::analyze_script_bindings::analyze_script_bindings;
use super::context::ScriptCompileContext;

pub const NORMAL_SCRIPT_DEFAULT_VAR: &str = "__default__";

pub struct ScriptCompileResult {
    pub block: SfcBlock,
    pub content: String,
    pub bindings: crate::core::options::BindingMetadata,
}

pub fn process_normal_script(ctx: &mut ScriptCompileContext, scope_id: &str) -> ScriptCompileResult {
    let script = ctx.descriptor.script.clone().unwrap();
    let mut content = script.content.clone();
    let script_ast = ctx.script_ast.as_ref().unwrap();
    let bindings = analyze_script_bindings(&script_ast.body);
    let css_vars = ctx.descriptor.css_vars.clone();
    let gen_default_as = ctx.options.gen_default_as.clone();
    let is_prod = ctx.options.is_prod;

    if !css_vars.is_empty() || gen_default_as.is_some() {
        let default_var = gen_default_as
            .clone()
            .unwrap_or_else(|| NORMAL_SCRIPT_DEFAULT_VAR.to_string());
        let mut s = MagicString::new(&content);
        rewrite_default_ast(&script_ast.body, &mut s, &default_var);
        content = s.to_string();
        if !css_vars.is_empty() && !ctx.options.template_ssr {
            content.push_str(&gen_normal_script_css_vars_code(
                &css_vars,
                &bindings,
                scope_id,
                is_prod,
                &default_var,
            ));
        }
        if gen_default_as.is_none() {
            content.push_str(&format!("\nexport default {default_var}"));
        }
    }

    ScriptCompileResult {
        block: script,
        content,
        bindings,
    }
}
