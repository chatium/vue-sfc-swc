//! The exact pipeline `@chatium/ugc-source-compiler`'s `helper.cjs` runs for a
//! `.vue` file, as a single call.

use crate::core::ast::Arena;
use crate::sfc::compile_script::compile_script_with;
use crate::sfc::compile_style::{StyleCompileOptions, compile_style};
use crate::sfc::compile_template::{TemplateCompileOptions, compile_template_ast};
use crate::sfc::parse::{AttrValue, SfcDescriptor, SfcParseOptions, parse};
use crate::sfc::rewrite_default::rewrite_default;
use crate::sfc::script::context::{ScriptAsts, ScriptCompileOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VuePosition {
    pub line: i64,
    pub character: i64,
}

#[derive(Debug, Clone)]
pub struct VueError {
    pub msg: String,
    pub position: Option<VuePosition>,
}

#[derive(Debug, Clone)]
pub enum VueStage {
    Parse,
    Template,
    Style,
    Script,
}

#[derive(Debug, Clone)]
pub struct VueFailure {
    pub stage: VueStage,
    pub errors: Vec<VueError>,
    pub logic: Option<String>,
    pub file_path: String,
}

#[derive(Debug, Clone)]
pub struct VueOutput {
    pub logic: String,
    pub template: String,
    pub code: String,
    /// the same component compiled for `vue/server-renderer`; `None` when
    /// only its SSR compile fails
    pub ssr: Option<String>,
    /// each non-empty `<style>`'s compiled CSS, in source order; `code` does
    /// not inject them, how they reach the DOM is the consumer's call
    pub styles: Vec<String>,
    pub modules: Vec<(String, Vec<(String, String)>)>,
}

fn sha1_id(source: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(source.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    format!("{}{}", &hash[..4], &hash[hash.len() - 4..])
}

/// `compileVue` from `helper.cjs`
pub fn compile_vue(source: &str, path: &str) -> Result<VueOutput, VueFailure> {
    let slashed = format!("/{path}");
    let parsed = parse(
        source,
        SfcParseOptions {
            filename: slashed.clone(),
            source_map: true,
            ..Default::default()
        },
    );
    if !parsed.errors.is_empty() {
        // an empty SFC reports against the slash-prefixed path
        let file_path = if source.trim().is_empty() {
            slashed.clone()
        } else {
            path.to_string()
        };
        return Err(VueFailure {
            stage: VueStage::Parse,
            errors: parsed
                .errors
                .iter()
                .map(|e| VueError {
                    msg: e.message.clone(),
                    position: e.loc.as_ref().map(|l| VuePosition {
                        line: l.start.line - 1,
                        character: l.start.column - 1,
                    }),
                })
                .collect(),
            logic: None,
            file_path,
        });
    }

    let d = &parsed.descriptor;
    let is_ts = d
        .script
        .as_ref()
        .and_then(|s| s.lang.clone())
        .or_else(|| d.script_setup.as_ref().and_then(|s| s.lang.clone()))
        .map(|l| l.starts_with("ts"))
        .unwrap_or(false);

    // both script compiles below read the same blocks, so they share one parse
    let script_asts = ScriptAsts::new();

    // `logic`: the script compiled without an inline template
    let mut logic = String::from("// @shared\n");
    if d.script.is_some() || d.script_setup.is_some() {
        match compile_script_with(
            d,
            &parsed.arena,
            &script_asts,
            ScriptCompileOptions {
                id: "someid".to_string(),
                ..Default::default()
            },
        ) {
            Ok(r) => logic.push_str(&r.content),
            Err(e) => {
                return Err(VueFailure {
                    stage: VueStage::Script,
                    errors: vec![VueError {
                        msg: e,
                        position: None,
                    }],
                    logic: None,
                    file_path: path.to_string(),
                });
            }
        }
    }

    let mut all_errors: Vec<VueError> = Vec::new();
    let mut template_code = String::new();
    let mut template_failed = false;
    if let Some(template) = &d.template {
        // parse errors returned above, so the fresh AST carries none
        let (t_arena, t_children) =
            crate::sfc::compile_template::fresh_template_ast(d, &parsed.arena)
                .unwrap_or_default();
        let _ = template;
        let r = compile_template_ast(
            t_arena,
            t_children,
            source.to_string(),
            Vec::new(),
            TemplateCompileOptions {
                filename: path.to_string(),
                id: "someid".to_string(),
                expression_plugins: if is_ts {
                    vec!["typescript".to_string()]
                } else {
                    Vec::new()
                },
                ..Default::default()
            },
        );
        template_failed = !r.errors.is_empty();
        all_errors.extend(r.errors.iter().map(|e| VueError {
            msg: e.message.clone(),
            position: e.loc.as_ref().map(|l| VuePosition {
                line: l.start.line - 1,
                character: l.start.column - 1,
            }),
        }));
        template_code = r.code;
    }

    let id = sha1_id(source);
    let scoped = d.styles.iter().any(|s| s.scoped);

    let mut styles: Vec<String> = Vec::new();
    let mut modules: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for style in &d.styles {
        if style.content.is_empty() {
            continue;
        }
        let lang = style.lang.clone().filter(|l| l == "sass" || l == "scss");
        let r = compile_style(StyleCompileOptions {
            source: style.content.clone(),
            filename: path.to_string(),
            id: id.clone(),
            scoped: style.scoped,
            modules: style.module.is_some(),
            preprocess_lang: lang,
            ..Default::default()
        });
        all_errors.extend(r.errors.iter().map(|e| VueError {
            msg: e.msg.clone(),
            // `vueErrors` reads postcss's 1-based line/column
            position: e.position.map(|(line, column)| VuePosition {
                line: line as i64 - 1,
                character: column as i64 - 1,
            }),
        }));
        if let (Some(m), Some(module)) = (r.modules, style.module.as_ref()) {
            let name = match module {
                AttrValue::Str(s) => s.clone(),
                AttrValue::True => "$style".to_string(),
            };
            modules.push((name, m));
        }
        styles.push(r.code);
    }

    if !all_errors.is_empty() {
        return Err(VueFailure {
            stage: if template_failed {
                VueStage::Template
            } else {
                VueStage::Style
            },
            errors: all_errors,
            logic: Some(logic),
            file_path: path.to_string(),
        });
    }

    let component = |ssr| {
        component_code(
            d,
            &parsed.arena,
            &script_asts,
            &logic,
            &Component {
                id: &id,
                scoped,
                modules: &modules,
                is_ts,
                source,
                path,
            },
            ssr,
        )
    };
    let code = component(false)?;
    // not a build error: the component then renders in the browser only
    let ssr = component(true).ok();

    Ok(VueOutput {
        logic,
        template: template_code,
        code,
        ssr,
        styles,
        modules,
    })
}

/// What both component compiles of one SFC share.
struct Component<'a> {
    id: &'a str,
    scoped: bool,
    modules: &'a [(String, Vec<(String, String)>)],
    is_ts: bool,
    source: &'a str,
    path: &'a str,
}

/// The browser component, or with `ssr` the same component rendering to a
/// string on the server.
fn component_code(
    d: &SfcDescriptor,
    arena: &Arena,
    script_asts: &ScriptAsts,
    logic: &str,
    c: &Component,
    ssr: bool,
) -> Result<String, VueFailure> {
    let render = if ssr { "ssrRender" } else { "render" };
    let path = c.path;
    let mut code = String::from("const __sfc__ = {};");
    let mut bindings: Option<crate::core::options::BindingMetadata> = None;
    if d.script.is_some() || d.script_setup.is_some() {
        match compile_script_with(
            d,
            arena,
            script_asts,
            ScriptCompileOptions {
                id: c.id.to_string(),
                inline_template: true,
                template_ssr: ssr,
                ..Default::default()
            },
        ) {
            Ok(r) => {
                bindings = Some(r.bindings.clone());
                let rewritten = rewrite_default(&r.content, "__sfc__", c.is_ts).map_err(|e| {
                    VueFailure {
                        stage: VueStage::Script,
                        errors: vec![VueError {
                            msg: e,
                            position: None,
                        }],
                        logic: None,
                        file_path: path.to_string(),
                    }
                })?;
                code = format!("{rewritten};");
            }
            Err(e) => {
                return Err(VueFailure {
                    stage: VueStage::Script,
                    errors: vec![VueError {
                        msg: e,
                        position: None,
                    }],
                    logic: Some(logic.to_string()),
                    file_path: path.to_string(),
                });
            }
        }
    }

    if d.template.is_some() && d.script_setup.is_none() {
        // the render function is compiled separately and attached
        // parse errors returned above, so the fresh AST carries none
        let (t_arena, t_children) =
            crate::sfc::compile_template::fresh_template_ast(d, arena).unwrap_or_default();
        let r = compile_template_ast(
            t_arena,
            t_children,
            c.source.to_string(),
            Vec::new(),
            TemplateCompileOptions {
                filename: path.to_string(),
                id: c.id.to_string(),
                scoped: c.scoped,
                ssr,
                ssr_css_vars: d.css_vars.clone(),
                binding_metadata: bindings.clone().unwrap_or_default(),
                binding_metadata_provided: bindings.is_some(),
                expression_plugins: if c.is_ts {
                    vec!["typescript".to_string()]
                } else {
                    Vec::new()
                },
                ..Default::default()
            },
        );
        // `vueErrors` throws here without attaching `logic`, so the helper's
        // catch-all classifies it as the script stage
        if !r.errors.is_empty() {
            return Err(VueFailure {
                stage: VueStage::Script,
                errors: r
                    .errors
                    .iter()
                    .map(|e| VueError {
                        msg: e.message.clone(),
                        position: e.loc.as_ref().map(|l| VuePosition {
                            line: l.start.line - 1,
                            character: l.start.column - 1,
                        }),
                    })
                    .collect(),
                logic: None,
                file_path: path.to_string(),
            });
        }
        let replaced = replace_render_export(&r.code, render);
        code.push_str(&format!(
            "\n{replaced}\n__sfc__.{render} = __sfc__{render};"
        ));
    }

    if c.scoped {
        code.push_str(&format!(
            "\n(__sfc__.__vccOpts || __sfc__).__scopeId = {};",
            serde_json::to_string(&format!("data-v-{}", c.id)).unwrap()
        ));
    }
    if !c.modules.is_empty() {
        let mut obj = serde_json::Map::new();
        for (name, entries) in c.modules {
            let mut inner = serde_json::Map::new();
            for (k, v) in entries {
                inner.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
            obj.insert(name.clone(), serde_json::Value::Object(inner));
        }
        code.push_str(&format!(
            "\n(__sfc__.__vccOpts || __sfc__).__cssModules = {};",
            serde_json::to_string(&serde_json::Value::Object(obj)).unwrap()
        ));
    }
    Ok(format!("// @shared\n{code}\nexport default __sfc__;"))
}

/// `/\nexport (function|const) <name>/` -> `$1 __sfc__<name>`
fn replace_render_export(code: &str, name: &str) -> String {
    for kw in ["function", "const"] {
        let needle = format!("\nexport {kw} {name}");
        if let Some(i) = code.find(&needle) {
            let mut out = String::new();
            out.push_str(&code[..i]);
            // the regex consumes the leading newline too
            out.push_str(&format!("{kw} __sfc__{name}"));
            out.push_str(&code[i + needle.len()..]);
            return out;
        }
    }
    code.to_string()
}
