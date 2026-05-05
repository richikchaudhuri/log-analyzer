//! Criterion bench for `analyze_str`. Measures end-to-end throughput of the
//! single-pass parallel rollup on a synthetic NCSA Common Log Format corpus.
//!
//! Run with: `cargo bench -p analyzer-core`
//! HTML reports land in `target/criterion/`.

use analyzer_core::{analyze_str, parse_line};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

fn synthetic_log(lines: usize) -> String {
    let paths = [
        "/", "/index.html", "/api/users", "/products/42", "/login",
        "/admin", "/.env", "/static/app.js", "/blog/post-1", "/cart",
    ];
    let methods = ["GET", "GET", "GET", "POST", "GET"];
    let statuses = [200u16, 200, 200, 200, 304, 404, 500, 401];

    let mut state: u64 = 0xdeadbeefcafebabe;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        state
    };

    let mut out = String::with_capacity(lines * 180);
    for _ in 0..lines {
        let ip_a = (next() % 224) + 1;
        let ip_b = next() % 256;
        let ip_c = next() % 256;
        let ip_d = next() % 256;
        let day = (next() % 28) + 1;
        let hour = next() % 24;
        let min = next() % 60;
        let sec = next() % 60;
        let method = methods[(next() as usize) % methods.len()];
        let path = paths[(next() as usize) % paths.len()];
        let status = statuses[(next() as usize) % statuses.len()];
        let bytes = (next() % 50_000) + 100;
        use std::fmt::Write;
        let _ = writeln!(
            out,
            r#"{ip_a}.{ip_b}.{ip_c}.{ip_d} - - [{day:02}/Jan/2026:{hour:02}:{min:02}:{sec:02} -0700] "{method} {path} HTTP/1.1" {status} {bytes}"#,
        );
    }
    out
}

fn bench_parse_line(c: &mut Criterion) {
    let line = r#"10.0.0.1 - - [03/May/2026:14:22:10 -0700] "GET /index.html HTTP/1.1" 200 4567"#;
    c.bench_function("parse_line", |b| {
        b.iter(|| parse_line(black_box(line)));
    });
}

fn bench_analyze_str(c: &mut Criterion) {
    let mut group = c.benchmark_group("analyze_str");

    for &n in &[10_000usize, 100_000, 1_000_000] {
        let corpus = synthetic_log(n);
        group.throughput(Throughput::Bytes(corpus.len() as u64));
        group.bench_with_input(format!("{}_lines", n), &corpus, |b, c| {
            b.iter(|| analyze_str(black_box(c)));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_parse_line, bench_analyze_str);
criterion_main!(benches);
