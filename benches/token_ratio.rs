use criterion::{Criterion, criterion_group, criterion_main, BenchmarkId};

use taoki::index::{index_source, extract_public_api, Language};

fn measure_ratios(c: &mut Criterion) {
    let samples: Vec<(&str, Language, &str)> = vec![
        ("Rust", Language::Rust, include_str!("fixtures/sample.rs")),
        ("Python", Language::Python, include_str!("fixtures/sample.py")),
        ("TypeScript", Language::TypeScript, include_str!("fixtures/sample.ts")),
        ("JavaScript", Language::JavaScript, include_str!("fixtures/sample.js")),
        ("Go", Language::Go, include_str!("fixtures/sample.go")),
        ("Java", Language::Java, include_str!("fixtures/sample.java")),
    ];

    let mut group = c.benchmark_group("byte_ratio");
    for (name, lang, source) in &samples {
        group.bench_with_input(BenchmarkId::new("index", name), source, |b, src| {
            b.iter(|| {
                let output = index_source(src.as_bytes(), *lang).unwrap();
                let ratio = 1.0 - (output.len() as f64 / src.len() as f64);
                assert!(ratio > 0.4, "{name}: byte reduction {:.0}% is below 40% threshold", ratio * 100.0);
                output
            })
        });
    }
    group.finish();

    // Also benchmark extract_public_api ratio
    let mut group2 = c.benchmark_group("byte_ratio_api");
    for (name, lang, source) in &samples {
        group2.bench_with_input(BenchmarkId::new("public_api", name), source, |b, src| {
            b.iter(|| {
                let (types, funcs) = extract_public_api(src.as_bytes(), *lang).unwrap();
                let output_len: usize = types.iter().chain(funcs.iter()).map(|s| s.len()).sum();
                output_len
            })
        });
    }
    group2.finish();

    // Print summary table
    println!("\n## Byte Efficiency Summary\n");
    println!("| Language | Source bytes | Index bytes | Reduction |");
    println!("|----------|-------------|-------------|-----------|");
    for (name, lang, source) in &samples {
        let output = index_source(source.as_bytes(), *lang).unwrap();
        let reduction = 1.0 - (output.len() as f64 / source.len() as f64);
        println!("| {name} | {} | {} | {:.0}% |", source.len(), output.len(), reduction * 100.0);
    }
}

criterion_group!(benches, measure_ratios);
criterion_main!(benches);
