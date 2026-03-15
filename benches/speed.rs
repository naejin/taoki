use criterion::{Criterion, black_box, criterion_group, criterion_main};

use taoki::index::{index_source, extract_public_api, Language};

fn bench_index_source(c: &mut Criterion) {
    let samples: Vec<(&str, Language, &str)> = vec![
        ("Rust", Language::Rust, include_str!("fixtures/sample.rs")),
        ("Python", Language::Python, include_str!("fixtures/sample.py")),
        ("TypeScript", Language::TypeScript, include_str!("fixtures/sample.ts")),
        ("JavaScript", Language::JavaScript, include_str!("fixtures/sample.js")),
        ("Go", Language::Go, include_str!("fixtures/sample.go")),
        ("Java", Language::Java, include_str!("fixtures/sample.java")),
    ];

    let mut group = c.benchmark_group("index_source");
    for (name, lang, source) in &samples {
        group.bench_function(*name, |b| {
            b.iter(|| index_source(black_box(source.as_bytes()), *lang))
        });
    }
    group.finish();

    let mut group = c.benchmark_group("extract_public_api");
    for (name, lang, source) in &samples {
        group.bench_function(*name, |b| {
            b.iter(|| extract_public_api(black_box(source.as_bytes()), *lang))
        });
    }
    group.finish();
}

fn bench_code_map(c: &mut Criterion) {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    for i in 0..15 {
        let ext = match i % 3 { 0 => "rs", 1 => "py", _ => "ts" };
        let path = dir.path().join(format!("file_{i}.{ext}"));
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "// file {i}").unwrap();
        writeln!(f, "pub fn func_{i}() {{}}").unwrap();
    }
    std::process::Command::new("git").args(["init"]).current_dir(dir.path()).output().ok();

    let mut group = c.benchmark_group("code_map");
    group.bench_function("cold", |b| {
        b.iter(|| {
            let _ = std::fs::remove_dir_all(dir.path().join(".cache"));
            taoki::codemap::build_code_map(dir.path(), &[])
        })
    });
    let _ = taoki::codemap::build_code_map(dir.path(), &[]);
    group.bench_function("cached", |b| {
        b.iter(|| taoki::codemap::build_code_map(dir.path(), &[]))
    });
    group.finish();
}

criterion_group!(benches, bench_index_source, bench_code_map);
criterion_main!(benches);
