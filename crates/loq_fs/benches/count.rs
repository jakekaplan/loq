//! Benchmarks for line counting performance.

use std::io::Write;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use tempfile::NamedTempFile;

use loq_fs::count::inspect_file;

fn create_test_file(lines: usize) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    for i in 0..lines {
        writeln!(
            file,
            "This is line number {i} with some content to make it realistic"
        )
        .unwrap();
    }
    file
}

fn bench_line_counting(c: &mut Criterion) {
    let mut group = c.benchmark_group("line_counting");

    for size in [100, 1000, 10000, 100000] {
        let file = create_test_file(size);
        group.bench_with_input(BenchmarkId::new("inspect_file", size), &file, |b, file| {
            b.iter(|| inspect_file(black_box(file.path())))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_line_counting);
criterion_main!(benches);
