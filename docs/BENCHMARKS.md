# Benchmarking roxy

This doc describes how roxy's performance is measured, why the tools were
chosen, and how to take periodic snapshots so perf work is grounded in
numbers instead of hunches.

Three layers, in order of reproducibility:

| Layer | Tool | Measures | Run time |
|---|---|---|---|
| Micro | [criterion](https://bheisler.github.io/criterion.rs/book/) in `benches/` | Hot functions in isolation | seconds |
| Macro | `examples/bench_client.rs` + `examples/echo_upstream.rs` | End-to-end HTTP proxy over MCP | ~1 min |
| Profile | [samply](https://github.com/mstange/samply) under load | CPU time per stack | ~30 s |

Everything is orchestrated by `scripts/bench.sh`. You should basically
never need to call criterion, echo_upstream or bench_client by hand — the
script builds the right binaries, sets `RUST_LOG=warn`, drives the load,
collects the output and writes it under `.bench/<tag>/`.

## Quickstart

```bash
# Baseline before a change:
./scripts/bench.sh --tag before

# ...make your change, rebuild, commit...

# After the change, compare micro-benches against the baseline:
./scripts/bench.sh --tag after --compare before

# Add CPU profile under load (samply):
./scripts/bench.sh --tag after --flame

# View the flamegraph in a browser (local web server):
samply load .bench/after/roxy.samply.json.gz
```

Results live under `.bench/<tag>/`:

```
.bench/after/
├── criterion.txt         # raw criterion output
├── macro.jsonl           # one JSON summary per scenario
├── macro.tools-list.8.err
├── macro.tools-list.32.err
├── ...
├── roxy.samply.json.gz   # samply CPU profile (if --flame)
├── flame.jsonl           # bench_client summary for the profiled run
└── flame.err             # bench_client stderr
```

`.bench/` is git-ignored — snapshots are per machine, per run, per mood.

## What each layer actually measures

### Micro (criterion)

Tiny, hermetic benchmarks for the hot functions touched by the perf work:

| Bench | Covers |
|---|---|
| `parse/*` | `UpstreamCallResult::parse` — every upstream response |
| `body_start/*` | FastCGI header stripping |
| `request_id/*` | Stack vs heap UUID encoding |
| `envelope/*` | `UpstreamEnvelope` serialisation |

`cargo bench` stores baselines in `target/criterion/<bench>/<name>/<tag>/`.
`--compare X` makes criterion print `change: -12% ± 2%` lines against
baseline `X`, which is the only honest way to claim "this change helped".

Sample numbers on one Apple Silicon laptop, no other load:

```
parse/content_small      ~168 ns
parse/content_large      ~2.7 µs
parse/error              ~93  ns
parse/elicit             ~508 ns
body_start/with_headers  ~14  ns
body_start/large_body_8k ~8.8 ns
envelope/call_tool_small ~123 ns
envelope/discover        ~46  ns
request_id/stack         ~795 ns
request_id/heap          ~802 ns
```

Reality check from the numbers above: the `request_id/{stack,heap}`
delta is ~7 ns out of ~800 ns. The CSPRNG call inside `Uuid::new_v4()`
dominates both variants, so the point of fix #5 is to remove alloc
pressure (not wall time). Use criterion to find hot spots, not just to
rubber-stamp them.

### Macro (bench_client)

`oha`, `wrk` and friends are awkward against rmcp's streamable-http
transport: the client has to first `initialize` (two POSTs), then reuse
`mcp-session-id` on every request, then read an SSE-framed body — none
of which off-the-shelf HTTP bench tools can negotiate. Attempts to use
`oha` hang on the SSE stream.

`examples/bench_client.rs` is a ~200-line reqwest-based client that does
the handshake once and then spams either `tools/list` or `tools/call
echo` for a fixed duration, per worker, with accurate latency
histograms. It prints one JSON line per run so `.bench/<tag>/macro.jsonl`
is trivially diff-able.

`examples/echo_upstream.rs` is the other half: a minimal axum server
that replies with canned `UpstreamDiscoverResponse` /
`UpstreamContentResponse` payloads so the only thing being benched is
roxy itself, not PHP-FPM or a downstream HTTP API.

`scripts/bench.sh --tag X` runs the full cross product:

```
tools-list  c=8,32,64
tools-call  c=8,32,64
```

The 8 → 32 → 64 sweep shows where the proxy saturates. On this laptop
the knee is around `c=32`: RPS plateaus and latency starts climbing
linearly, which is the expected behaviour once the runtime is CPU-bound.

### Profile (samply)

`scripts/bench.sh --flame` builds roxy with the `profiling` cargo
profile (release + line tables), runs it under
`samply record --save-only`, drives 10 s of `tools-call` load, then
terminates the profiled roxy so samply flushes the CPU profile to
`.bench/<tag>/roxy.samply.json.gz`. View it with:

```bash
samply load .bench/<tag>/roxy.samply.json.gz
```

Samply opens a local web UI (Firefox profiler) showing sampled stacks
with percentages — jump to "flame graph" view for the traditional
bottom-up hotspot picture. The `[profile.profiling]` entry in
`Cargo.toml` is what gives you readable frame names instead of `??`.

### Installing samply

`samply` is not in the default Rust toolchain:

```bash
cargo install samply
```

On macOS it uses userland sampling and does not need `sudo`. On Linux
it uses `perf_event_open` — you may need `sysctl
kernel.perf_event_paranoid=1` in some distros.

## Periodic workflow

The point of all of this is that perf work should look like this:

1. **Snapshot current state.**
   ```
   ./scripts/bench.sh --tag base --flame
   ```
2. **Form a hypothesis.** Look at the flamegraph. Pick the function
   that owns the most samples and ask: can it do less work?
3. **Change it.** One thing at a time — otherwise the diff is useless.
4. **Snapshot again and compare.**
   ```
   ./scripts/bench.sh --tag after --compare base
   ```
   Criterion will print `change: -XX%` for each micro-bench. The macro
   numbers are in `.bench/after/macro.jsonl` next to
   `.bench/base/macro.jsonl` — diff them or load them into a notebook.
5. **Decide.** If the numbers say "negligible" or "worse", revert.
   Commit messages should include the key delta.

### Making the numbers trustworthy

- Always run on a quiet laptop. Close browsers, chat apps, anything
  doing sync in the background.
- `caffeinate -i ./scripts/bench.sh ...` on macOS to avoid going to
  sleep mid-run.
- Don't compare numbers across machines. `.bench/` snapshots are
  machine-local; a laptop on battery vs the same laptop plugged in
  can shift numbers 20% by itself.
- Run at least twice and keep the better — first runs warm the CPU
  caches, code generation, etc.
- If a bench scenario comes back with `err != 0` in `macro.jsonl`,
  treat the whole row as invalid and re-run.
- A change < 5% in criterion is probably noise on a laptop.
