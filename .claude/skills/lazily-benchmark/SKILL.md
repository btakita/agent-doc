---
description: "Run, update, and verify lazily-rs benchmark results in BENCHMARKS.md"
user-invocable: true
argument-hint: "[check|update|regression|profile]"
---

# lazily-benchmark

Manage benchmark results for the `lazily` reactive primitives library in `src/lazily-rs`.

## Invocation

```
/lazily-benchmark           — check freshness (default)
/lazily-benchmark check     — verify BENCHMARKS.md matches current Criterion output
/lazily-benchmark update    — run benchmarks and regenerate BENCHMARKS.md
/lazily-benchmark regression — A/B regression workflow (baseline vs current)
/lazily-benchmark profile   — run instrumentation profile and update BENCHMARKS.md
```

## Commands

All commands run from `src/lazily-rs/`.

### check

Verify that the generated section in `BENCHMARKS.md` matches the latest Criterion output.

```bash
make benchmark-check
```

Equivalent to: `python3 scripts/update-benchmark-results.py --check`

Exit 0 = current. Non-zero = stale, run `update`.

### update

Run all benchmarks with instrumentation and regenerate the generated section in `BENCHMARKS.md`.

```bash
cargo bench --features instrumentation
python3 scripts/update-benchmark-results.py
```

Or use the Makefile target:

```bash
make benchmark-update
```

The script reads Criterion output from `target/criterion/`, the instrumentation profile from `target/lazily-instrumentation-profile.csv`, and replaces the `<!-- benchmark-results:start -->` ... `<!-- benchmark-results:end -->` section in `BENCHMARKS.md`.

### regression

A/B regression workflow comparing before/after a change:

```bash
cargo bench --features instrumentation -- --save-baseline before
# apply the performance patch
cargo bench --features instrumentation -- --baseline before
python3 scripts/update-benchmark-results.py --no-run
```

### profile

Run the instrumentation profile example and update BENCHMARKS.md:

```bash
make instrumentation-profile
python3 scripts/update-benchmark-results.py
```

## BENCHMARKS.md Structure

The file has these sections inside the generated markers:

1. **Regression budgets** — deterministic lock acquisition count thresholds per profile
2. **Synchronization strategy adoption gate** — required evidence before adopting alternative strategies
3. **Watch-item A/B follow-up** — tracked latency investigations
4. **Criterion latency tables** — p50/p95 and mean/CI per group/case
5. **Instrumentation profile tables** — alloc, recompute, lock acquisition, sidecar metrics
6. **Lock attribution tables** — per-site lock acquisition counts for ThreadSafe profiles

## Budget Enforcement

The `--check` mode enforces lock-site budgets defined in the regression budget table. A budget violation means the profile exceeds its max lock acquisitions or a named site exceeds its per-site budget.

Budgets use deterministic lock acquisition counts instead of elapsed wait/hold time for reproducibility across machines.

## Notes

- Benchmarks require `--features instrumentation` for lock attribution and profile data.
- The Python script (`scripts/update-benchmark-results.py`) is the single source of truth for parsing and formatting.
- `make benchmark-check` is part of `make check` — CI will fail if BENCHMARKS.md is stale.
- The lazily-rs submodule has its own `Makefile` with `benchmark-check`, `benchmark-update`, and `instrumentation-profile` targets.
