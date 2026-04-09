//! Micro-benchmark for `body_start_offset` — skips the CGI headers of a
//! FastCGI response. Covers fix #2 (zero-copy body extraction).

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use roxy::__bench::body_start_offset;

fn bench_body_start_with_headers(c: &mut Criterion) {
    let raw: &[u8] =
        b"Content-Type: application/json\r\nX-Powered-By: PHP/8.3\r\n\r\n{\"ok\":true}";
    c.bench_function("body_start/with_headers", |b| {
        b.iter(|| body_start_offset(black_box(raw)))
    });
}

fn bench_body_start_large_body(c: &mut Criterion) {
    let mut v = Vec::from(&b"Content-Type: application/json\r\n\r\n"[..]);
    v.extend(std::iter::repeat_n(b'x', 8 * 1024));
    c.bench_function("body_start/large_body_8k", |b| {
        b.iter(|| body_start_offset(black_box(&v)))
    });
}

fn bench_body_start_no_headers(c: &mut Criterion) {
    let raw: &[u8] = b"{\"ok\":true}";
    c.bench_function("body_start/no_headers", |b| {
        b.iter(|| body_start_offset(black_box(raw)))
    });
}

criterion_group!(
    benches,
    bench_body_start_with_headers,
    bench_body_start_large_body,
    bench_body_start_no_headers
);
criterion_main!(benches);
