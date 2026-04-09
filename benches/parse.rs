//! Micro-benchmarks for `UpstreamCallResult::parse` — the function every
//! upstream tool/resource/prompt response flows through. Covers fix #1
//! (single-pass parsing).

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use roxy::protocol::UpstreamCallResult;

fn bench_parse_content_small(c: &mut Criterion) {
    let payload = br#"{"content":[{"type":"text","text":"hello world"}]}"#;
    c.bench_function("parse/content_small", |b| {
        b.iter(|| UpstreamCallResult::parse(black_box(payload)).unwrap())
    });
}

fn bench_parse_content_large(c: &mut Criterion) {
    let mut s = String::from(r#"{"content":["#);
    for i in 0..20 {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            r#"{{"type":"text","text":"item {i} with a longer payload to simulate real-world tool output"}}"#
        ));
    }
    s.push_str(r#"],"structured_content":{"items":20,"status":"ok"}}"#);
    let payload = s.into_bytes();
    c.bench_function("parse/content_large", |b| {
        b.iter(|| UpstreamCallResult::parse(black_box(&payload)).unwrap())
    });
}

fn bench_parse_error(c: &mut Criterion) {
    let payload = br#"{"error":{"code":404,"message":"tool not found"}}"#;
    c.bench_function("parse/error", |b| {
        b.iter(|| UpstreamCallResult::parse(black_box(payload)).unwrap())
    });
}

fn bench_parse_elicit(c: &mut Criterion) {
    let payload = br#"{"elicit":{"message":"pick one","schema":{"type":"object","properties":{"class":{"type":"string"}}},"context":{"step":1}}}"#;
    c.bench_function("parse/elicit", |b| {
        b.iter(|| UpstreamCallResult::parse(black_box(payload)).unwrap())
    });
}

criterion_group!(
    benches,
    bench_parse_content_small,
    bench_parse_content_large,
    bench_parse_error,
    bench_parse_elicit
);
criterion_main!(benches);
