//! Serialization of `UpstreamEnvelope` — the outgoing side of every
//! tool/resource/prompt call. Combined with `parse.rs` these two benches
//! cover the full JSON-work round-trip per request.

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::Map;
use std::hint::black_box;

use roxy::protocol::{UpstreamEnvelope, UpstreamRequest};

fn small_args() -> Map<String, serde_json::Value> {
    let mut args = Map::new();
    args.insert("city".into(), serde_json::Value::String("Moscow".into()));
    args.insert("units".into(), serde_json::Value::String("metric".into()));
    args
}

fn bench_envelope_call_tool_small(c: &mut Criterion) {
    let args = small_args();
    c.bench_function("envelope/call_tool_small", |b| {
        b.iter(|| {
            let envelope = UpstreamEnvelope {
                session_id: Some("sess-abc"),
                request_id: "req-0001",
                request: UpstreamRequest::CallTool {
                    name: "get_weather",
                    arguments: Some(&args),
                    elicitation_results: None,
                    context: None,
                },
            };
            serde_json::to_vec(black_box(&envelope)).unwrap()
        })
    });
}

fn bench_envelope_discover(c: &mut Criterion) {
    c.bench_function("envelope/discover", |b| {
        b.iter(|| {
            let envelope = UpstreamEnvelope {
                session_id: None,
                request_id: "req-0001",
                request: UpstreamRequest::Discover,
            };
            serde_json::to_vec(black_box(&envelope)).unwrap()
        })
    });
}

criterion_group!(
    benches,
    bench_envelope_call_tool_small,
    bench_envelope_discover
);
criterion_main!(benches);
