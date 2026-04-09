//! Minimal MCP streamable-http load generator for roxy.
//!
//! `oha`/`wrk` don't know how to negotiate the MCP handshake (initialize →
//! notifications/initialized → session id header) and some of them hang on
//! the `text/event-stream` body rmcp returns. This binary speaks MCP just
//! enough to hammer `roxy` correctly and produce comparable numbers.
//!
//! Example:
//!
//!     cargo run --release --example bench_client -- \
//!         --url http://127.0.0.1:9090/mcp \
//!         --concurrency 32 --duration 10 --mode tools-list
//!
//! Modes:
//!   * tools-list — POST tools/list (cheapest path, mostly serde)
//!   * tools-call — POST tools/call echo (full proxy round-trip)
//!
//! Output: JSON line with the per-run summary that `scripts/bench.sh`
//! collects into `.bench/`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
enum Mode {
    ToolsList,
    ToolsCall,
}

impl Mode {
    fn body(self, id: u64) -> Value {
        match self {
            Mode::ToolsList => json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/list"
            }),
            Mode::ToolsCall => json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": "echo",
                    "arguments": { "message": "hi" }
                }
            }),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mode::ToolsList => "tools-list",
            Mode::ToolsCall => "tools-call",
        }
    }
}

struct Args {
    url: String,
    concurrency: usize,
    duration: Duration,
    mode: Mode,
    warmup: Duration,
}

fn parse_args() -> Args {
    let mut url = "http://127.0.0.1:9090/mcp".to_string();
    let mut concurrency = 16usize;
    let mut duration = Duration::from_secs(5);
    let mut mode = Mode::ToolsList;
    let mut warmup = Duration::from_secs(1);

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--url" => url = it.next().expect("--url needs value"),
            "--concurrency" | "-c" => {
                concurrency = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("-c needs int")
            }
            "--duration" | "-d" => {
                let secs: u64 = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("-d needs int");
                duration = Duration::from_secs(secs);
            }
            "--warmup" => {
                let secs: u64 = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--warmup needs int");
                warmup = Duration::from_secs(secs);
            }
            "--mode" => {
                mode = match it.next().as_deref() {
                    Some("tools-list") => Mode::ToolsList,
                    Some("tools-call") => Mode::ToolsCall,
                    other => panic!("--mode must be tools-list or tools-call, got {other:?}"),
                }
            }
            "-h" | "--help" => {
                eprintln!(
                    "bench_client --url URL --concurrency N --duration SECS --mode tools-list|tools-call"
                );
                std::process::exit(0);
            }
            _ => panic!("unknown arg: {arg}"),
        }
    }

    Args {
        url,
        concurrency,
        duration,
        mode,
        warmup,
    }
}

async fn init_session(client: &Client, url: &str) -> anyhow::Result<String> {
    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "clientInfo": { "name": "roxy-bench-client", "version": "0" },
            "capabilities": {}
        }
    });

    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(serde_json::to_vec(&init_body)?)
        .send()
        .await?;

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("no mcp-session-id header on initialize response"))?
        .to_string();

    // Drain body so the connection can be reused.
    let _ = resp.bytes().await?;

    // notifications/initialized — required before any other requests.
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", &sid)
        .body(serde_json::to_vec(&initialized)?)
        .send()
        .await?
        .bytes()
        .await?;

    Ok(sid)
}

#[allow(clippy::too_many_arguments)]
async fn worker(
    client: Client,
    url: String,
    sid: String,
    mode: Mode,
    deadline: Instant,
    id_base: u64,
    count: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    latencies_ns: Arc<tokio::sync::Mutex<Vec<u64>>>,
) {
    let mut i = id_base;
    while Instant::now() < deadline {
        i += 1;
        let body_bytes = match serde_json::to_vec(&mode.body(i)) {
            Ok(b) => b,
            Err(_) => {
                errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        let start = Instant::now();
        let result = client
            .post(&url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", &sid)
            .body(body_bytes)
            .send()
            .await;

        let ok = match result {
            Ok(resp) if resp.status().is_success() => {
                // Drain the body — rmcp's SSE frame has a known content-length
                // or chunked end, so this completes promptly.
                resp.bytes().await.is_ok()
            }
            _ => false,
        };
        let elapsed = start.elapsed().as_nanos() as u64;

        if ok {
            count.fetch_add(1, Ordering::Relaxed);
            let mut v = latencies_ns.lock().await;
            v.push(elapsed);
        } else {
            errors.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args();

    let client = Client::builder()
        .pool_max_idle_per_host(args.concurrency * 2)
        .build()?;

    eprintln!("initializing MCP session at {}", args.url);
    let sid = init_session(&client, &args.url).await?;
    eprintln!("session: {sid}");

    // Warmup: ignore these results, just fill connection pools and caches.
    if args.warmup > Duration::ZERO {
        eprintln!("warmup {:?}", args.warmup);
        let warmup_deadline = Instant::now() + args.warmup;
        let mut handles = Vec::new();
        let ignored_count = Arc::new(AtomicU64::new(0));
        let ignored_errs = Arc::new(AtomicU64::new(0));
        let ignored_lat = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        for w in 0..args.concurrency {
            handles.push(tokio::spawn(worker(
                client.clone(),
                args.url.clone(),
                sid.clone(),
                args.mode,
                warmup_deadline,
                (w as u64) * 1_000_000,
                Arc::clone(&ignored_count),
                Arc::clone(&ignored_errs),
                Arc::clone(&ignored_lat),
            )));
        }
        for h in handles {
            h.await.ok();
        }
    }

    eprintln!("measuring {:?} with c={}", args.duration, args.concurrency);
    let count = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(tokio::sync::Mutex::new(Vec::<u64>::new()));
    let deadline = Instant::now() + args.duration;

    let run_start = Instant::now();
    let mut handles = Vec::new();
    for w in 0..args.concurrency {
        handles.push(tokio::spawn(worker(
            client.clone(),
            args.url.clone(),
            sid.clone(),
            args.mode,
            deadline,
            (w as u64) * 1_000_000 + 10_000_000,
            Arc::clone(&count),
            Arc::clone(&errors),
            Arc::clone(&latencies),
        )));
    }
    for h in handles {
        h.await.ok();
    }
    let elapsed = run_start.elapsed();

    let total_ok = count.load(Ordering::Relaxed);
    let total_err = errors.load(Ordering::Relaxed);
    let mut lat = latencies.lock().await;
    lat.sort_unstable();

    let rps = total_ok as f64 / elapsed.as_secs_f64();
    let p50 = percentile(&lat, 0.50);
    let p95 = percentile(&lat, 0.95);
    let p99 = percentile(&lat, 0.99);
    let p999 = percentile(&lat, 0.999);
    let max = *lat.last().unwrap_or(&0);

    // Emit a single JSON line — easy to tee into a file and diff later.
    let summary = json!({
        "mode": args.mode.name(),
        "url": args.url,
        "concurrency": args.concurrency,
        "duration_s": elapsed.as_secs_f64(),
        "ok": total_ok,
        "errors": total_err,
        "rps": rps,
        "latency_ns": {
            "p50":  p50,
            "p95":  p95,
            "p99":  p99,
            "p999": p999,
            "max":  max,
        }
    });
    println!("{summary}");

    eprintln!(
        "done: {total_ok} ok / {total_err} err, {rps:.0} rps, p50={}µs p95={}µs p99={}µs",
        p50 / 1_000,
        p95 / 1_000,
        p99 / 1_000,
    );

    Ok(())
}
