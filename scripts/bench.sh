#!/usr/bin/env bash
#
# Reproducible perf snapshot for roxy.
#
# Runs, in order:
#   1. cargo bench — criterion micro-benches, saved as a baseline
#   2. starts echo_upstream + roxy in release mode
#   3. drives the running proxy with examples/bench_client for each scenario
#   4. tears everything down and saves a .bench/<timestamp>/ snapshot
#
# Usage:
#   scripts/bench.sh                  # default: tag = timestamp
#   scripts/bench.sh --tag before     # save as .bench/before/
#   scripts/bench.sh --no-micro       # skip criterion
#   scripts/bench.sh --no-macro       # skip HTTP load
#   scripts/bench.sh --compare before # diff micros against a saved baseline
#
# Periodic workflow:
#   scripts/bench.sh --tag before          # snapshot current main
#   # ...apply a perf change...
#   scripts/bench.sh --tag after --compare before
#
# All output is reproducible only on the same machine with no other load.
# Run under `caffeinate -i` on macOS or disable thermal throttling first.

set -euo pipefail

cd "$(dirname "$0")/.."

TAG="$(date +%Y%m%d-%H%M%S)"
RUN_MICRO=1
RUN_MACRO=1
RUN_FLAME=0
COMPARE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tag) TAG="$2"; shift 2 ;;
        --no-micro) RUN_MICRO=0; shift ;;
        --no-macro) RUN_MACRO=0; shift ;;
        --flame) RUN_FLAME=1; shift ;;
        --compare) COMPARE="$2"; shift 2 ;;
        -h|--help)
            head -n 30 "$0" | grep '^# ' | sed 's/^# //'
            exit 0
            ;;
        *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
done

OUT_DIR=".bench/$TAG"
mkdir -p "$OUT_DIR"

echo "=> building release binaries"
cargo build --release --bin roxy --example echo_upstream --example bench_client \
    2>&1 | tail -5

# --------------------------------------------------------------------
# 1. Criterion micro-benchmarks
# --------------------------------------------------------------------
if [[ "$RUN_MICRO" == "1" ]]; then
    echo
    echo "=> criterion micro-benches (baseline=$TAG)"
    if [[ -n "$COMPARE" ]]; then
        cargo bench -- \
            --save-baseline "$TAG" --baseline "$COMPARE" \
            --warm-up-time 1 --measurement-time 3 \
            2>&1 | tee "$OUT_DIR/criterion.txt" | grep -E "(time:|change:|Performance|^test)" || true
    else
        cargo bench -- \
            --save-baseline "$TAG" \
            --warm-up-time 1 --measurement-time 3 \
            2>&1 | tee "$OUT_DIR/criterion.txt" | grep -E "time:" || true
    fi
fi

# --------------------------------------------------------------------
# 2. End-to-end load: echo_upstream + roxy + bench_client
# --------------------------------------------------------------------
if [[ "$RUN_MACRO" == "1" ]]; then
    echo
    echo "=> starting echo_upstream + roxy"
    RUST_LOG=warn ./target/release/examples/echo_upstream >/dev/null 2>&1 &
    ECHO_PID=$!
    trap 'kill $ECHO_PID 2>/dev/null || true; kill $ROXY_PID 2>/dev/null || true; wait 2>/dev/null || true' EXIT
    sleep 0.3

    RUST_LOG=warn ./target/release/roxy \
        --transport http --port 9090 \
        --upstream http://127.0.0.1:8001/mcp >/dev/null 2>&1 &
    ROXY_PID=$!
    sleep 0.5

    # Poll until roxy is ready.
    for _ in $(seq 1 20); do
        if curl -sS -o /dev/null -m 1 -X POST http://127.0.0.1:9090/mcp \
             -H 'content-type: application/json' \
             -H 'accept: application/json, text/event-stream' \
             -d '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"probe","version":"0"},"capabilities":{}}}'; then
            break
        fi
        sleep 0.2
    done

    : > "$OUT_DIR/macro.jsonl"
    for MODE in tools-list tools-call; do
        for C in 8 32 64; do
            echo "=> bench_client mode=$MODE c=$C"
            ./target/release/examples/bench_client \
                --url http://127.0.0.1:9090/mcp \
                --concurrency "$C" --duration 5 --warmup 1 --mode "$MODE" \
                >> "$OUT_DIR/macro.jsonl" 2>"$OUT_DIR/macro.$MODE.$C.err"
            # Short summary to terminal.
            python3 - "$OUT_DIR/macro.jsonl" <<'PY'
import sys, json
r = json.loads(open(sys.argv[1]).readlines()[-1])
lat = r["latency_ns"]
print(f"   rps={r['rps']:.0f}  p50={lat['p50']//1000}µs"
      f"  p95={lat['p95']//1000}µs  p99={lat['p99']//1000}µs"
      f"  ok={r['ok']} err={r['errors']}")
PY
        done
    done

    kill $ROXY_PID $ECHO_PID 2>/dev/null || true
    wait 2>/dev/null || true
    trap - EXIT
fi

# --------------------------------------------------------------------
# 3. Flamegraph via samply (requires `cargo install samply`)
# --------------------------------------------------------------------
if [[ "$RUN_FLAME" == "1" ]]; then
    if ! command -v samply >/dev/null 2>&1; then
        echo "!! samply not found — install with: cargo install samply" >&2
        exit 1
    fi

    echo
    echo "=> building profiling binary"
    cargo build --profile profiling --bin roxy --example echo_upstream --example bench_client \
        2>&1 | tail -3

    echo "=> starting echo_upstream under profiling profile"
    RUST_LOG=warn ./target/profiling/examples/echo_upstream >/dev/null 2>&1 &
    ECHO_PID=$!
    trap 'kill $ECHO_PID 2>/dev/null || true; kill $SAMPLY_PID 2>/dev/null || true; wait 2>/dev/null || true' EXIT
    sleep 0.3

    # samply writes the recorded profile when the *child* process exits,
    # so we record for the duration of the load then kill roxy so samply
    # flushes to disk.
    LOAD_SECS=10
    WARMUP_SECS=2

    echo "=> starting roxy under samply"
    RUST_LOG=warn samply record --save-only --no-open \
        --output "$OUT_DIR/roxy.samply.json.gz" \
        -- ./target/profiling/roxy \
        --transport http --port 9090 \
        --upstream http://127.0.0.1:8001/mcp >/dev/null 2>&1 &
    SAMPLY_PID=$!

    # Wait for roxy to start listening (samply adds instrumentation overhead
    # on first launch so a probe loop is safer than a fixed sleep).
    for _ in $(seq 1 40); do
        if curl -sS -o /dev/null -m 1 -X POST http://127.0.0.1:9090/mcp \
             -H 'content-type: application/json' \
             -H 'accept: application/json, text/event-stream' \
             -d '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"probe","version":"0"},"capabilities":{}}}'; then
            break
        fi
        sleep 0.25
    done

    echo "=> driving ${LOAD_SECS}s of tools-call load at c=32"
    ./target/profiling/examples/bench_client \
        --url http://127.0.0.1:9090/mcp \
        --concurrency 32 --duration "$LOAD_SECS" --warmup "$WARMUP_SECS" \
        --mode tools-call \
        >> "$OUT_DIR/flame.jsonl" 2>"$OUT_DIR/flame.err"

    # Killing samply directly on macOS does not flush the profile and
    # orphans the child; the only reliable trigger is to terminate the
    # *child* process — samply then detaches cleanly and writes the
    # file before exiting. Find the roxy PID that samply spawned.
    # Killing samply directly does not flush; killing samply's *child*
    # (the profiled roxy) makes samply detach cleanly and write the
    # profile. `pgrep -P` walks the process tree from samply downward,
    # which also tolerates any wrapper samply may have in between.
    echo "=> stopping roxy to flush samply"
    ROXY_CHILD_PID=$(pgrep -P "$SAMPLY_PID" 2>/dev/null | head -1 || true)
    echo "   roxy child pid: ${ROXY_CHILD_PID:-<none>}"
    if [[ -n "${ROXY_CHILD_PID:-}" ]]; then
        kill "$ROXY_CHILD_PID" 2>/dev/null || true
    fi
    # Wait up to 10s for samply to flush and exit.
    for _ in $(seq 1 40); do
        if ! kill -0 "$SAMPLY_PID" 2>/dev/null; then
            break
        fi
        sleep 0.25
    done
    kill $ECHO_PID 2>/dev/null || true
    wait 2>/dev/null || true
    trap - EXIT

    if [[ -f "$OUT_DIR/roxy.samply.json.gz" ]]; then
        echo "=> profile saved to $OUT_DIR/roxy.samply.json.gz"
        echo "   view with:  samply load $OUT_DIR/roxy.samply.json.gz"
    else
        echo "!! samply profile was not written" >&2
        exit 1
    fi
fi

# --------------------------------------------------------------------
# 4. Print summary
# --------------------------------------------------------------------
echo
echo "=> snapshot saved to $OUT_DIR"
if [[ -f "$OUT_DIR/macro.jsonl" ]]; then
    echo
    echo "macro summary:"
    python3 - <<'PY' "$OUT_DIR/macro.jsonl"
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1])]
print(f"{'mode':<12}{'c':>4}{'rps':>12}{'p50_µs':>10}{'p95_µs':>10}{'p99_µs':>10}{'p999_µs':>10}")
for r in rows:
    lat = r['latency_ns']
    print(f"{r['mode']:<12}{r['concurrency']:>4}{r['rps']:>12.0f}"
          f"{lat['p50']//1000:>10}{lat['p95']//1000:>10}"
          f"{lat['p99']//1000:>10}{lat['p999']//1000:>10}")
PY
fi
