# agent-doc-core compile baseline

Tracks cold-build cost over time as the `#adcr` extraction waves land.
See `tasks/agent-doc/plan-agent-doc-core-extraction.md` for context.

## Measurement protocol

From `src/agent-doc/`:

```bash
# Cold rebuild of just the core crate (includes its dep tree):
cargo clean
cargo build -p agent-doc-core --release --timings
# Note the wall time + transitive dep count.

# Cold rebuild of the full main crate from the same clean state:
cargo clean
cargo build --release --timings
# Note the wall time.
```

The timing-report HTML lands in `target/cargo-timings/`. Record the
total wall time + transitive dep count below.

The acceptance criterion from the plan: `agent-doc-core` cold-builds in
**under 30%** of the main-crate cold build time.

## 2026-05-28 — Wave 0 (stub only)

`agent-doc-core` body is empty. This row establishes the dep-graph
floor: how long it takes to compile the seven pure deps
(`anyhow`, `serde`, `serde_yaml`, `uuid`, `pulldown-cmark`, `yrs`,
`similar`) before any extracted module exists.

| Crate | Wall time | Transitive crates | Notes |
|---|---|---|---|
| `agent-doc-core` (first build, deps included) | 5.70s | 48 | `cargo build -p agent-doc-core --release` from a clean target dir; build includes dep-tree compile |
| `agent-doc-core` (warm, only the stub) | 0.05s | 1 | `cargo build -p agent-doc-core --release` after `cargo clean -p agent-doc-core --release` (deps stay warm) |
| `agent-doc` main crate (incremental on top of core build) | 112s (1m 52s) | 2 new crates after the agent-doc-core build | `cargo build --release` from the state where agent-doc-core is already compiled |

Cold-from-scratch baselines for both crates have not yet been captured
on the same `cargo clean` boundary — they should be the first thing
collected at the start of `#adcr` Wave 1. The acceptance threshold
(30%) is anchored against those clean numbers.

## Wave log

Append a row per wave landing. Include:

- Date
- Wave + extracted module(s)
- agent-doc-core cold wall time
- agent-doc main-crate cold wall time
- agent-doc-core LOC after extraction
- Ratio (agent-doc-core / agent-doc) — must be ≤ 0.30 at Wave 4

| Date | Wave | Modules | core wall time | main wall time | core LOC | ratio |
|---|---|---|---|---|---|---|
| 2026-05-28 | 0 (stub) | (none) | tbd cold | tbd cold | 0 | n/a |
