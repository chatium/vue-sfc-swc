//! `cargo run --release --example case-diff -- <corpus.json> <index> [out.vue]` —
//! where one case of a `check-dir` corpus differs from the reference: the first
//! differing byte of each output field, with context. `out.vue` saves the source.

fn show(label: &str, got: &str, want: &str) {
    let p = got.bytes().zip(want.bytes()).take_while(|(x, y)| x == y).count();
    let p = (0..=p).rev().find(|&i| got.is_char_boundary(i) && want.is_char_boundary(i)).unwrap();
    let cut = |s: &str| { let a = (0..=p.saturating_sub(120)).rev().find(|&i| s.is_char_boundary(i)).unwrap(); let b = (p + 120).min(s.len()); let b = (b..=s.len()).find(|&i| s.is_char_boundary(i)).unwrap(); s[a..b].to_string() };
    println!("{label}: first diff at {p} (len got {} want {})\n  got:  {:?}\n  want: {:?}", got.len(), want.len(), cut(got), cut(want));
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let c = &cases[a[2].parse::<usize>().unwrap()];
    let src = c["source"].as_str().unwrap();
    if a.len() > 3 { std::fs::write(&a[3], src).unwrap(); }
    match vue_sfc::ugc::compile_vue(src, "component.vue") {
        Ok(r) => {
            let out = &c["out"];
            if let Some(e) = out.get("error") { println!("want error: {e}"); return; }
            for (k, g) in [("logic", &r.logic), ("template", &r.template), ("code", &r.code)] {
                let w = out[k].as_str().unwrap_or("");
                if g != w { show(k, g, w); }
            }
            let ws = out["styles"].as_array().unwrap();
            if ws.len() != r.styles.len() { println!("styles count got {} want {}", r.styles.len(), ws.len()); }
            for (i, (g, w)) in r.styles.iter().zip(ws).enumerate() {
                if g != w.as_str().unwrap() { show(&format!("styles[{i}]"), g, w.as_str().unwrap()); }
            }
        }
        Err(e) => println!("got error {:?} {:?}; want {}", e.stage, e.errors.iter().map(|x| &x.msg).collect::<Vec<_>>(), c["out"]),
    }
}
