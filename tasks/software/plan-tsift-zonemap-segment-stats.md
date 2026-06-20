# Plan: tsift zone-map / min-max segment statistics for pre-scan pruning

**Source:** `#sbtk` (from the duckdb-internals-part-1 review). DuckDB skips entire 122K-row row-groups by checking per-group min/max + null counts before reading any data. tsift's analog: stamp cheap per-file/per-segment statistics into the index so scoped / `--path` / kind-filtered searches skip whole files **before** building or scanning the BM25 corpus.

**Status (updated 2026-06-20):** operator greenlit implementation ("implement [this plan]"). **Phases 1, 2, 4 are DONE** in `tsift-index` (additive `file_zonemap` table + populate/delete + read API + soundness tests; `make check` green, 0 failures). **Phase 3 (live query-time pruning) is deferred pending redesign** — see the discovery note below. Committed locally to the tsift submodule; **push held** for operator review.

### Discovery (2026-06-20): the lexical search path does not read `index.db`
While wiring Phase 3 I found the lexical `Sift` search (`packages/tsift-search/src/sift.rs`, `search()`) builds/loads its **own `TokenIndex`** (a JSON token cache) and prunes via `files_matching_any(query_tokens)` — it never consults `index.db`'s `file_state`/`symbols`. So the original Phase 3 premise ("consult the index zonemap in `sift.rs`") is wrong: token-level pruning already exists there, and the zonemap's natural consumers are the **`index.db`-backed** commands (symbol/kind/scope/path queries, `explain`, `graph`, evidence), not the lexical corpus. The shipped read API `files_possibly_containing_kind` is the first sound consumer for that path. Revised Phase 3 below.

### Phase 3 investigation (2026-06-20): no current consumer benefits
Audited every index.db-backed consumer for a kind/path file-scan the zonemap could prune. **None qualify today:**
- `symbol_search`, `symbol_info`, `symbols_for_file`, `all_symbols` query the `symbols` table directly through SQL indexes (`idx_symbols_name`/`idx_symbols_file`) with bounded `WHERE` + `LIMIT` — they never iterate files and read contents, so a file-skip prune has nothing to accelerate.
- The only scan-heavy path, lexical `Sift` content search, runs on its own `TokenIndex` and already prunes via `files_matching_any` (token-level); it doesn't touch `index.db` and carries no symbol-kind data.
- `explain`/`graph`/evidence follow `call_edges` (graph traversal, not file scans); `impact.rs` path-scoping is already bounded at the scoped-DB level.

So the substrate (`file_zonemap` + `files_possibly_containing_kind`) is sound and tested but **currently inert — it has no beneficiary**. The next step is a direction decision, not a wiring:
1. **Make the lexical `Sift` search `index.db`-aware** so the zonemap can prune the *content* scan (mtime/path/kind). Biggest win, but a real `Sift` architecture change.
2. **Add a new `--kind`-filtered symbol/search command** that consumes `files_possibly_containing_kind` — a new feature that creates the consumer.
3. **Leave the substrate inert** as a tested foundation until a consumer naturally appears.

Soundness invariant for whichever path is chosen: pruned ⊇ true matches; add a per-consumer `pruned == unpruned` regression on a fixture.

## Why it fits tsift (grounded in current schema)

The index already carries most of the raw material, so this is additive, not a rewrite:

- `packages/tsift-index/src/index.rs` `file_state(path, mtime_secs, mtime_nanos, language)` — per-file row already exists; mtime already tracked.
- `symbols(name, kind, language, file, line, end_line, …)` — per-symbol line ranges + `kind` already stored, indexed by `file` (`idx_symbols_file`).
- `source_content_hash(source)` already computed during indexing (index.rs ~L1019).
- Search corpus is the separate BM25 "sift" corpus in `packages/tsift-search/src/sift.rs` (`search()` ~L279); scope/path resolution is in `packages/tsift-search/src/impact.rs` (`scope`, `impact_source_root`, `impact_db_path`).

The win, per the article's own caveat, is largest when the pruning key is well-ordered — i.e. path/scope-clustered indexes, which is exactly how tsift's scoped/workspace indexes are laid out.

## Phases (one actionable phase per increment)

### Phase 0 — Baseline (measure before building)
- Instrument a scoped / `--path` search and a kind-filtered search on a large index; record files-scanned and corpus-build cost. Capture a before-number so Phase 3 pruning has a measurable target. No schema change.

### Phase 1 — Schema: per-file zonemap columns
- Extend `file_state` (or add a sibling `file_zonemap` table keyed by `path`) with:
  - `min_line INTEGER` / `max_line INTEGER` (file line span; `max_line` = line count),
  - `symbol_count INTEGER`,
  - `kind_mask INTEGER` (bitfield over the known `kind` enum) **and/or** `kind_histogram TEXT` (compact JSON) for kind-filter pruning,
  - `content_hash TEXT` (reuse `source_content_hash`).
- Bump the index schema version key in `meta` so stale indexes reindex cleanly (no silent partial-column reads).
- Keep it a pure superset — existing readers ignore new columns.

### Phase 2 — Populate at index time
- In the file-indexing path that already writes `file_state` + `symbols`, aggregate the per-file stats from the parsed symbol set + source (min/max line, count, kind mask/histogram, content hash) and upsert the zonemap row in the same transaction.
- Incremental reindex: recompute the zonemap row whenever a file's `file_state` row is rewritten (mtime/hash change) so it never goes stale relative to `symbols`.

### Phase 3 — Query-time pruning (the actual payoff)
- Before building/scanning the BM25 corpus for a candidate file in `sift.rs` / scoped path in `impact.rs`, consult the zonemap and skip files that provably cannot match:
  - **path/scope predicate** — skip files outside the resolved scope cheaply (mostly already done; fold into one zonemap pass),
  - **kind-filtered search** — skip files whose `kind_mask` has zero of the requested kind,
  - **exact-identifier / bounded queries** — skip files whose stats rule out a hit.
- **Invariant (must be test-enforced): pruning is sound — the pruned result set is a strict subset of the unpruned set with zero false negatives.** A zonemap may only skip a file when the predicate cannot match; when in doubt, do not skip.

### Phase 4 — Tests (deterministic, no false negatives)
- `tsift-index`: zonemap row is populated and matches `symbols` aggregates; survives incremental reindex.
- `tsift-search`: for a corpus of fixtures, `pruned_hits == unpruned_hits` across path-scoped, kind-filtered, and exact queries (soundness); plus an assertion that files *were* actually skipped (pruning is effective, not a no-op).

### Phase 5 — Ship (operator-gated)
- Update `SPEC.md` / relevant `specs/*.md` + `VERSIONS.md` entry.
- `make check`, then `cargo install --path .` + `cargo build --release` for local testing.
- Commit + push — **only on explicit operator go** (this feature originated from an AI recommendation, not a direct build request).

## Related

- `#wzg6` (BackendEval weighting) is the sibling recommendation: this zonemap work is **substrate-agnostic** (lands in tsift's own SQLite/sift layer) and is independent of whether DuckDB/DuckPGQ is later promoted as a GraphStore. If DuckDB is adopted, zonemaps become native row-group stats for free; if not, this gives tsift the same pruning win on SQLite today.
- Ties to icebox `#4z0w` (increase max-hop caps): cheaper pre-scan pruning is what makes broader scans / deeper hops affordable.
