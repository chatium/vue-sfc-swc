fn main() {
    let path = std::env::args().nth(1).unwrap();
    let src = std::fs::read_to_string(&path).unwrap();
    match vue_sfc::ugc::compile_vue(&src, "component.vue") {
        Ok(r) => println!("{}", r.template),
        Err(e) => println!(
            "ERR stage={:?} {:?}",
            e.stage,
            e.errors.iter().map(|x| &x.msg).collect::<Vec<_>>()
        ),
    }
}
