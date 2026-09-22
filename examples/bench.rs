//! `cargo run --release --example bench -- <dir> [top]` — compiles every
//! `.vue` under `dir` through `ugc::compile_vue` and prints the total and the
//! slowest files. `PAR=1` also runs it on every core, the way
//! ugc-source-compiler's `compile_in_process` does.
use std::time::Instant;

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "node_modules" && n != ".git") {
                walk(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "vue") {
            out.push(p);
        }
    }
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let top: usize = std::env::args().nth(2).map_or(15, |s| s.parse().unwrap());
    let mut files = Vec::new();
    walk(std::path::Path::new(&dir), &mut files);
    let sources: Vec<_> = files
        .iter()
        .map(|p| (p, std::fs::read_to_string(p).unwrap_or_default()))
        .collect();
    let mut times = Vec::new();
    let all = Instant::now();
    for (p, src) in &sources {
        let t = Instant::now();
        let _ = std::panic::catch_unwind(|| vue_sfc::ugc::compile_vue(src, "component.vue"));
        times.push((t.elapsed(), src.len(), *p));
    }
    let total = all.elapsed();
    let bytes: usize = sources.iter().map(|(_, s)| s.len()).sum();
    println!(
        "{} files, {:.1} MB, {:.2?} total, {:.1} MB/s",
        sources.len(),
        bytes as f64 / 1e6,
        total,
        bytes as f64 / 1e6 / total.as_secs_f64()
    );
    if std::env::var("PAR").is_ok() {
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let t = Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                std::thread::Builder::new()
                    .stack_size(8 << 20)
                    .spawn_scoped(scope, || {
                        while let Some((_, src)) =
                            sources.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                        {
                            let _ = std::panic::catch_unwind(|| {
                                vue_sfc::ugc::compile_vue(src, "component.vue")
                            });
                        }
                    })
                    .unwrap();
            }
        });
        let par = t.elapsed();
        println!(
            "{threads} threads: {par:.2?}, {:.1}x over one",
            total.as_secs_f64() / par.as_secs_f64()
        );
    }
    times.sort_by_key(|t| std::cmp::Reverse(t.0));
    for (t, len, p) in times.iter().take(top) {
        println!("{t:>10.2?} {:>8} B  {}", len, p.display());
    }
}
