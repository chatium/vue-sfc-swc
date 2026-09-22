//! Replays a `check-dir-ssr.mjs` corpus: the SSR template and the inline SSR
//! render function, compared against the reference compiler.
use serde_json::Value;
use vue_sfc::sfc::compile_template::{TemplateCompileOptions, compile_template_ast, reparse_template};
use vue_sfc::sfc::parse::{SfcParseOptions, parse};
use vue_sfc::sfc::script::context::ScriptCompileOptions;

fn is_ts(lang: &Option<String>) -> bool {
    lang.as_deref().map(|l| l.starts_with("ts")).unwrap_or(false)
}

/// `Ok(None)` when there is no script block at all
fn compile_script_only(source: &str) -> Result<Option<String>, String> {
    let parsed = parse(
        source,
        SfcParseOptions { filename: "/component.vue".to_string(), source_map: false, ..Default::default() },
    );
    let d = &parsed.descriptor;
    if d.script.is_none() && d.script_setup.is_none() {
        return Ok(None);
    }
    vue_sfc::sfc::compile_script::compile_script(
        d,
        &parsed.arena,
        ScriptCompileOptions {
            id: "someid".to_string(),
            inline_template: true,
            template_ssr: true,
            ..Default::default()
        },
    )
    .map(|r| Some(r.content))
}

fn compile(source: &str) -> (Option<String>, Option<String>) {
    let parsed = parse(
        source,
        SfcParseOptions {
            filename: "/component.vue".to_string(),
            source_map: false,
            ..Default::default()
        },
    );
    let d = &parsed.descriptor;
    let ts = is_ts(&d.script.as_ref().and_then(|s| s.lang.clone()))
        || is_ts(&d.script_setup.as_ref().and_then(|s| s.lang.clone()));
    let plugins = if ts { vec!["typescript".to_string()] } else { Vec::new() };

    let template = d
        .template
        .as_ref()
        .filter(|t| t.src.is_none())
        .and_then(|_| reparse_template(source))
        .map(|(arena, children, errors)| {
            compile_template_ast(
                arena,
                children,
                source.to_string(),
                errors,
                TemplateCompileOptions {
                    filename: "component.vue".to_string(),
                    id: "someid".to_string(),
                    ssr: true,
                    ssr_css_vars: d.css_vars.clone(),
                    expression_plugins: plugins.clone(),
                    ..Default::default()
                },
            )
            .code
        });

    let script = if d.script.is_some() || d.script_setup.is_some() {
        vue_sfc::sfc::compile_script::compile_script(
            d,
            &parsed.arena,
            ScriptCompileOptions {
                id: "someid".to_string(),
                inline_template: true,
                template_ssr: true,
                ..Default::default()
            },
        )
        .ok()
        .map(|r| r.content)
    } else {
        None
    };
    (template, script)
}

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut cats = std::collections::BTreeMap::<String, Vec<usize>>::new();
    let mut ok = 0usize;
    let mut skipped = 0usize;
    for (i, case) in cases.iter().enumerate() {
        let source = case["source"].as_str().unwrap();
        // the reference itself failed: only check that this port fails too
        if case.get("parseError").is_some() || case.get("templateThrow").is_some() {
            skipped += 1;
            continue;
        }
        if case.get("scriptThrow").is_some() {
            skipped += 1;
            let r = std::panic::catch_unwind(|| compile_script_only(source));
            if matches!(r, Ok(Ok(_))) {
                cats.entry("ssr script: expected an error".into()).or_default().push(i);
            }
            continue;
        }
        let r = std::panic::catch_unwind(|| compile(source));
        let cat = match r {
            Err(p) => {
                let m = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                Some(format!("panic: {}", m.chars().take(90).collect::<String>()))
            }
            Ok((template, script)) => {
                let want_t = case.get("template").and_then(|t| t.as_str());
                let want_s = case.get("script").and_then(|s| s.as_str());
                if template.as_deref() != want_t {
                    Some("ssr template differs".to_string())
                } else if script.is_some() && want_s.is_some() && script.as_deref() != want_s {
                    Some("ssr script differs".to_string())
                } else if script.is_none() && want_s.is_some() {
                    Some("ssr script: unexpected error".to_string())
                } else {
                    ok += 1;
                    None
                }
            }
        };
        if let Some(c) = cat {
            cats.entry(c).or_default().push(i);
        }
    }
    println!(
        "{path}: {ok}/{} match ({skipped} skipped, the reference failed)",
        cases.len() - skipped
    );
    let mut v: Vec<_> = cats.into_iter().collect();
    v.sort_by_key(|(_, idx)| std::cmp::Reverse(idx.len()));
    for (c, idx) in v {
        println!("  {:5}  {c}   [cases {:?}]", idx.len(), &idx[..idx.len().min(6)]);
    }
}
