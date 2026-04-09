//! Stack vs heap encoding of per-request UUID correlation ids. Covers
//! fix #5 — the `request_id/stack` sample is what roxy does now, the
//! `request_id/heap` sample is what it used to do (`Uuid::to_string()`).

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use roxy::__bench::fresh_request_id;

fn bench_request_id_stack(c: &mut Criterion) {
    c.bench_function("request_id/stack", |b| {
        b.iter(|| {
            let mut buf = [0u8; uuid::fmt::Hyphenated::LENGTH];
            let id: &str = fresh_request_id(&mut buf);
            black_box(id.len())
        })
    });
}

fn bench_request_id_heap(c: &mut Criterion) {
    c.bench_function("request_id/heap", |b| {
        b.iter(|| {
            let id = uuid::Uuid::new_v4().to_string();
            black_box(id.len())
        })
    });
}

criterion_group!(benches, bench_request_id_stack, bench_request_id_heap);
criterion_main!(benches);
