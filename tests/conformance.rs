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
