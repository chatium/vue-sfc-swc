//! Differential conformance: every case was produced by the real
//! `@vue/compiler-sfc@3.5.38` (see `tools/gen-fixtures.mjs`).

use serde_json::Value;
use vue_sfc::core::parser::{ParserOptions, base_parse};
use vue_sfc::core::serialize;
use vue_sfc::sfc::parse::{AttrValue, SfcBlock, SfcParseOptions};

fn load(name: &str) -> Vec<Value> {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap()
}

#[test]
fn parse_base() {
    let cases = load("parse-base");
    let mut failed: Vec<(String, String)> = Vec::new();
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let result = base_parse(input, ParserOptions::default());
        let got = serialize::root(&result.arena, result.arena.root(result.root));
        let want = &case["ast"];
        if &got != want {
            failed.push((input.to_string(), first_diff(&got, want, "$")));
        }
        let got_errors: Vec<Value> = result
            .errors
            .iter()
            .map(|e| {
                serde_json::json!({
                    "code": e.code,
                    "message": e.message,
                    "loc": e.loc.as_ref().map(|l| serde_json::json!({
                        "start": {"column": l.start.column, "line": l.start.line, "offset": l.start.offset},
                        "end": {"column": l.end.column, "line": l.end.line, "offset": l.end.offset},
                        "source": l.source,
                    })),
                })
            })
            .collect();
        let want_errors = case["errors"].as_array().unwrap();
        if got_errors.len() != want_errors.len() {
            failed.push((
                input.to_string(),
                format!(
                    "error count {} != {}: {:?} vs {:?}",
                    got_errors.len(),
                    want_errors.len(),
                    got_errors,
                    want_errors
                ),
            ));
        }
    }
    report("parse-base", cases.len(), failed);
}

fn report(name: &str, total: usize, failed: Vec<(String, String)>) {
    if failed.is_empty() {
        return;
    }
    let shown = failed.len().min(8);
    let mut msg = format!("{}/{} {name} cases differ\n", failed.len(), total);
    for (input, diff) in failed.iter().take(shown) {
        msg.push_str(&format!("--- input: {input:?}\n    {diff}\n"));
    }
    panic!("{msg}");
}

/// Reports the first structural difference, to keep failures readable.
fn first_diff(got: &Value, want: &Value, path: &str) -> String {
    match (got, want) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                match a.get(k) {
                    None => return format!("{path}.{k}: missing (want {v})"),
                    Some(av) => {
                        if av != v {
                            return first_diff(av, v, &format!("{path}.{k}"));
                        }
                    }
                }
            }
            for k in a.keys() {
                if !b.contains_key(k) {
                    return format!("{path}.{k}: unexpected (got {})", a[k]);
                }
            }
            String::new()
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return format!("{path}: length {} != {}", a.len(), b.len());
            }
            for (i, (av, bv)) in a.iter().zip(b).enumerate() {
                if av != bv {
                    return first_diff(av, bv, &format!("{path}[{i}]"));
                }
            }
            String::new()
        }
        _ => format!("{path}: {got} != {want}"),
    }
}

#[test]
fn corpus_is_not_vacuous() {
    let cases = load("parse-base");
    assert!(cases.len() > 500, "corpus too small");
    let with_children = cases
        .iter()
        .filter(|c| c["ast"]["children"].as_array().map(|a| !a.is_empty()).unwrap_or(false))
        .count();
    assert!(with_children > 500, "only {with_children} non-empty ASTs");
}


fn block_json(b: Option<&SfcBlock>) -> Value {
    match b {
        None => Value::Null,
        Some(b) => {
            let attrs: serde_json::Map<String, Value> = b
                .attrs
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match v {
                            AttrValue::True => Value::Bool(true),
                            AttrValue::Str(s) => Value::String(s.clone()),
                        },
                    )
                })
                .collect();
            serde_json::json!({
                "type": b.block_type,
                "content": b.content,
                "attrs": Value::Object(attrs),
                "lang": b.lang.clone(),
                "src": b.src.clone(),
                "scoped": b.scoped,
                "module": b.module.as_ref().map(|m| match m {
                    AttrValue::True => Value::Bool(true),
                    AttrValue::Str(s) => Value::String(s.clone()),
                }),
                "setup": b.setup.as_ref().map(|m| match m {
                    AttrValue::True => Value::Bool(true),
                    AttrValue::Str(s) => Value::String(s.clone()),
                }),
                "loc": serde_json::json!({
                    "start": {"column": b.loc.start.column, "line": b.loc.start.line, "offset": b.loc.start.offset},
                    "end": {"column": b.loc.end.column, "line": b.loc.end.line, "offset": b.loc.end.offset},
                    "source": b.loc.source,
                }),
            })
        }
    }
}

#[test]
fn sfc_parse() {
    let cases = load("sfc-parse");
    let mut failed: Vec<(String, String)> = Vec::new();
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let r = vue_sfc::sfc::parse::parse(
            input,
            SfcParseOptions {
                source_map: false,
                ..Default::default()
            },
        );
        let d = &r.descriptor;
        let got = serde_json::json!({
            "template": block_json(d.template.as_ref()),
            "script": block_json(d.script.as_ref()),
            "scriptSetup": block_json(d.script_setup.as_ref()),
            "styles": Value::Array(d.styles.iter().map(|b| block_json(Some(b))).collect()),
            "customBlocks": Value::Array(d.custom_blocks.iter().map(|b| block_json(Some(b))).collect()),
            "cssVars": d.css_vars,
            "slotted": d.slotted,
        });
        let want = &case["descriptor"];
        if &got != want {
            failed.push((input.to_string(), first_diff(&got, want, "$")));
            continue;
        }
        let want_errors = case["errors"].as_array().unwrap();
        if r.errors.len() != want_errors.len() {
            failed.push((
                input.to_string(),
                format!(
                    "error count {} != {}: got {:?}, want {:?}",
                    r.errors.len(),
                    want_errors.len(),
                    r.errors.iter().map(|e| &e.message).collect::<Vec<_>>(),
                    want_errors
                        .iter()
                        .map(|e| e["message"].as_str().unwrap())
                        .collect::<Vec<_>>()
                ),
            ));
        }
    }
    report("sfc-parse", cases.len(), failed);
}

#[test]
fn compile_dom() {
    let cases = load("compile-dom");
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut ok = 0usize;
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let want = case["code"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            let mut opts = vue_sfc::dom::compile::CompileOptions::default();
            opts.transform.prefix_identifiers = true;
            opts.transform.hoist_static = true;
            opts.transform.cache_handlers = true;
            opts.transform.filename = "template.vue.html".to_string();
            opts.codegen.mode = vue_sfc::core::options::CodegenMode::Module;
            opts.codegen.prefix_identifiers = true;
            opts.codegen.filename = "template.vue.html".to_string();
            vue_sfc::dom::compile::compile(input, opts)
        });
        match result {
            Ok(r) => {
                if r.code == want {
                    ok += 1;
                } else {
                    failed.push((
                        input.to_string(),
                        format!("got:\n{}\nwant:\n{}", r.code, want),
                    ));
                }
            }
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                failed.push((input.to_string(), format!("panic: {msg}")));
            }
        }
    }
    eprintln!("compile-dom: {ok}/{} match", cases.len());
    report("compile-dom", cases.len(), failed);
}

#[test]
fn compile_template() {
    let cases = load("compile-template");
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut ok = 0usize;
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let scoped = case["scoped"].as_bool().unwrap();
        let want = case["code"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            vue_sfc::sfc::compile_template::compile_template(
                input,
                vue_sfc::sfc::compile_template::TemplateCompileOptions {
                    filename: "anonymous.vue".to_string(),
                    id: "someid".to_string(),
                    scoped,
                    ..Default::default()
                },
            )
        });
        match result {
            Ok(r) => {
                if r.code == want {
                    ok += 1;
                } else {
                    failed.push((
                        format!("{input:?} scoped={scoped}"),
                        format!("got:\n{}\nwant:\n{}", r.code, want),
                    ));
                }
            }
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                failed.push((format!("{input:?} scoped={scoped}"), format!("panic: {msg}")));
            }
        }
    }
    eprintln!("compile-template: {ok}/{} match", cases.len());
    report("compile-template", cases.len(), failed);
}

#[test]
fn rewrite_default() {
    let cases = load("rewrite-default");
    let mut failed: Vec<(String, String)> = Vec::new();
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let ts = case["ts"].as_bool().unwrap();
        let want = case["output"].as_str().unwrap();
        let got = vue_sfc::sfc::rewrite_default::rewrite_default(input, "__sfc__", ts);
        if got != want {
            failed.push((
                format!("{input:?} ts={ts}"),
                format!("got:\n{got}\nwant:\n{want}"),
            ));
        }
    }
    report("rewrite-default", cases.len(), failed);
}

#[test]
fn compile_script() {
    let cases = load("compile-script");
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut ok = 0usize;
    let mut errored = 0usize;
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let inline = case["inlineTemplate"].as_bool().unwrap();
        let want_error = case.get("error").and_then(|e| e.as_str());
        let result = std::panic::catch_unwind(|| {
            let parsed = vue_sfc::sfc::parse::parse(
                input,
                vue_sfc::sfc::parse::SfcParseOptions {
                    source_map: false,
                    ..Default::default()
                },
            );
            let arena = vue_sfc::core::ast::Arena::new();
            let _ = &arena;
            vue_sfc::sfc::compile_script::compile_script(
                &parsed.descriptor,
                &parsed.arena,
                vue_sfc::sfc::script::context::ScriptCompileOptions {
                    id: "xxxxxxxx".to_string(),
                    inline_template: inline,
                    ..Default::default()
                },
            )
        });
        match (result, want_error) {
            (Ok(Ok(r)), None) => {
                let want = case["content"].as_str().unwrap();
                if r.content == want {
                    ok += 1;
                } else {
                    failed.push((
                        format!("{input:?} inline={inline}"),
                        format!("got:\n{}\n---want:\n{want}", r.content),
                    ));
                }
            }
            (Ok(Err(_)), Some(_)) => {
                errored += 1;
                ok += 1;
            }
            (Ok(Err(e)), None) => failed.push((
                format!("{input:?} inline={inline}"),
                format!("unexpected error: {e}"),
            )),
            (Ok(Ok(_)), Some(e)) => failed.push((
                format!("{input:?} inline={inline}"),
                format!("expected error: {e}"),
            )),
            (Err(p), _) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                failed.push((format!("{input:?} inline={inline}"), format!("panic: {msg}")));
            }
        }
    }
    eprintln!(
        "compile-script: {ok}/{} match ({errored} matched as errors)",
        cases.len()
    );
    report("compile-script", cases.len(), failed);
}
