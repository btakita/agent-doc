# lazily-benchmark Runbook

Detailed workflow for running, verifying, and updating lazily-rs benchmark results.

## Prerequisites

- `src/lazily-rs` submodule initialized
- Python 3.11+ (for `tomllib`)
- Rust toolchain with Criterion benchmarking support

## Freshness Check

Run from `src/lazily-rs/`:

```bash
make benchmark-check
```

This runs `python3 scripts/update-benchmark-results.py --check`, which:

1. Reads the current Criterion output from `target/criterion/`
2. Reads the instrumentation profile from `target/lazily-instrumentation-profile.csv`
3. Compares the parsed results against the generated section in `BENCHMARKS.md`
4. Reports stale/out-of-date or budget violations

Exit 0 = everything current and within budget. Non-zero = needs update.

## Full Update Workflow

### 1. Run benchmarks

```bash
cargo bench --features instrumentation
```

This produces:

- `target/criterion/<group>/<case>/new/` — Criterion estimate files
- `target/lazily-instrumentation-profile.csv` — lock attribution data

### 2. Regenerate BENCHMARKS.md

```bash
make benchmark-update
```

Or manually:

```bash
python3 scripts/update-benchmark-results.py
```

The script:

- Parses all Criterion results and groups them by test
- Reads the instrumentation profile CSV
- Generates markdown tables (latency, mean/CI, profile, lock attribution)
- Replaces the `<!-- benchmark-results:start -->` ... `<!-- benchmark-results:end -->` section
- Preserves everything outside the markers (Multi-Language, Related, License)

### 3. Verify

```bash
make benchmark-check
```

### 4. Commit (within submodule)

```bash
git add BENCHMARKS.md
git commit -m "benchmarks: update results"
```

Then update the submodule ref from the parent repo.

## Regression Workflow

When evaluating a performance-sensitive change:

```bash
# 1. Save baseline before the change
cargo bench --features instrumentation -- --save-baseline before

# 2. Apply the performance patch

# 3. Run against the baseline
cargo bench --features instrumentation -- --baseline before

# 4. Update BENCHMARKS.md without re-running benches
python3 scripts/update-benchmark-results.py --no-run

# 5. Check budgets
make benchmark-check
```

## Instrumentation Profile

The instrumentation profile captures per-profile metrics (allocs, recomputes, lock acquisitions, sidecar operations):

```bash
make instrumentation-profile
```

This runs `examples/instrumentation_profile.rs` and writes `target/lazily-instrumentation-profile.csv`. The update script reads this CSV alongside Criterion results.

## Budget Configuration

Lock-site budgets are defined inline in `BENCHMARKS.md` within the regression budget table. The `--check` mode enforces:

- Max total lock acquisitions per profile
- Per-site budgets (e.g., `set_cell_invalidation<=16, dependency_edge<=64`)

To adjust budgets, edit the table in `BENCHMARKS.md` and re-run `make benchmark-check`.

## Integration with `make check`

`make check` includes `benchmark-check`, so CI will fail if:

- BENCHMARKS.md is stale (Criterion output doesn't match the generated section)
- Any profile exceeds its lock acquisition budget

This ensures benchmark data stays synchronized with code changes.

## Troubleshooting

| Issue | Cause | Fix |
|-------|-------|-----|
| `benchmark-check` fails with "stale" | Code changed but benchmarks not re-run | `make benchmark-update` after running `cargo bench` |
| Budget violation | Performance regression increased lock acquisitions | Investigate the profile, fix the regression, or adjust budget if intentional |
| Missing `target/criterion/` | Benchmarks never run | `cargo bench --features instrumentation` |
| Missing profile CSV | Instrumentation example not run | `make instrumentation-profile` |
