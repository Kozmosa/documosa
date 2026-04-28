# Documosa Stress Benchmark

This directory contains Documosa stress benchmarks. They are intentionally kept
outside `tests/` and run through Cargo's benchmark target.

## Run

```sh
cd apps/documosa
cargo bench --bench documosa_stress
```

For a quicker local smoke run:

```sh
DOCUMOSA_BENCH_OPS_PER_ITER=10 \
DOCUMOSA_BENCH_CONCURRENCY=4 \
DOCUMOSA_BENCH_WS_CLIENTS=2 \
cargo bench --bench documosa_stress
```

For a heavier run:

```sh
DOCUMOSA_BENCH_INITIAL_LINES=1000 \
DOCUMOSA_BENCH_WS_CLIENTS=32 \
DOCUMOSA_BENCH_CONCURRENCY=64 \
DOCUMOSA_BENCH_OPS_PER_ITER=1000 \
cargo bench --bench documosa_stress
```

## Configuration

The benchmark uses development-machine defaults:

```sh
DOCUMOSA_BENCH_INITIAL_LINES=100
DOCUMOSA_BENCH_WS_CLIENTS=8
DOCUMOSA_BENCH_CONCURRENCY=16
DOCUMOSA_BENCH_OPS_PER_ITER=100
```

Each Criterion iteration starts a mixed REST/MCP workload against a real Axum
server backed by a file-based SQLite database in a temporary directory. The
server is bound to `127.0.0.1:0`, so the operating system picks an available
port.

## Scenario

Setup creates one document, seeds a stable history diff pair, and opens the
configured number of WebSocket clients. The workload then mixes:

- `GET /api/documents/{id}`
- line insert with no anchor
- history list
- history diff against the seeded event pair
- audit note set and clear
- MCP `tools/call` for `get_document`
- MCP `tools/call` for `list_history_events`

Line inserts intentionally omit an anchor so concurrent writers avoid
line-id conflicts while still exercising SQLite renumbering, audit, and
document-version paths.

## Output

Criterion reports throughput and elapsed time. Each iteration also prints:

- total operations
- successful and failed operations
- error rate
- p50/p95/p99/max request latency
- WebSocket events received so far

Individual request failures are counted in the error rate and do not fail the
benchmark. Server startup, schema migration, and seed data creation still panic
because those are functional setup failures.
