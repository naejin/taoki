use criterion::{criterion_group, criterion_main, Criterion};

fn bench_placeholder(_c: &mut Criterion) {
    // Token ratio benchmarks — to be implemented in Task 5.
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
