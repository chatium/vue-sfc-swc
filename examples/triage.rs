//! Replays a `check-dir.mjs` corpus and groups the divergences.
use serde_json::Value;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let cases: Vec<Value> = serde_json::from_str(&text).unwrap();
    let mut cats = std::collections::BTreeMap::<String, Vec<usize>>::new();
    let mut ok = 0usize;
    let mut errs = 0usize;
    for (i, case) in cases.iter().enumerate() {
        let source = case["source"].as_str().unwrap();
        let out = &case["out"];
        let want_error = out.get("error").is_some();
        let r = std::panic::catch_unwind(|| vue_sfc::ugc::compile_vue(source, "component.vue"));
        let cat = match r {
            Err(p) => {
                let m = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                Some(format!("panic: {}", m.chars().take(90).collect::<String>()))
            }
            Ok(Ok(_)) if want_error => Some("expected error, got output".into()),
            Ok(Err(f)) if !want_error => Some(format!(
                "unexpected error: {}",
                f.errors
                    .first()
                    .map(|e| e.msg.chars().take(90).collect::<String>())
                    .unwrap_or_default()
            )),
            Ok(Ok(r)) => {
                if r.logic != out["logic"].as_str().unwrap_or("") {
                    Some("logic differs".into())
                } else if r.template != out["template"].as_str().unwrap_or("") {
                    Some("template differs".into())
                } else if r.code != out["code"].as_str().unwrap_or("") {
                    Some("code differs".into())
                } else {
                    ok += 1;
                    None
                }
            }
            Ok(Err(_)) => {
                ok += 1;
                errs += 1;
                None
            }
        };
        if let Some(c) = cat {
            cats.entry(c).or_default().push(i);
        }
    }
    println!("{path}: {ok}/{} match ({errs} matched as errors)", cases.len());
    let mut v: Vec<_> = cats.into_iter().collect();
    v.sort_by_key(|(_, idx)| std::cmp::Reverse(idx.len()));
    for (c, idx) in v {
        println!("  {:4}  {c}   [cases {:?}]", idx.len(), &idx[..idx.len().min(6)]);
    }
}
