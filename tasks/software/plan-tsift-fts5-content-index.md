# Plan: native SQLite FTS5 content index in `index.db`

**Source:** `#b56m` "do the Best option" (2026-06-20). The zone-map audit found tsift's lexical search runs on a separate JSON `TokenIndex`, so the zone-map substrate (`file_zonemap`, already landed) has no consumer. The architecturally coherent fix — and the context that makes the zone-map pay off — is to move the inverted token index **into `index.db` as SQLite FTS5**. This retires the parallel `TokenIndex`, gives in-DB BM25 maintained incrementally alongside `symbols`/`file_state`, and lets the zone-map prune the FTS scan by kind/path/mtime.

This is a **real multi-phase project**, not a one-cycle change. Each phase is independently shippable and behavior-preserving until the final cutover. **Nothing here is started yet** — this is the plan for operator scheduling.

## Current state (grounded)
- Lexical search: `packages/tsift-search/src/sift.rs` — `Sift::search` loads/builds a `TokenIndex` (JSON cache under `.tsift/search-cache/token-index.json`), prunes via `files_matching_any(query_tokens)`, then reads + `score_file`s each candidate. `TokenIndex::build` walks the tree; rebuilt when the cache is stale.
- Index DB: `packages/tsift-index/src/index.rs` — `file_state`, `symbols`, `file_zonemap` (new), `call_edges`, `route_nodes`, `dir_state`, `meta`. Incremental apply path already reads each changed file's source + symbols in one transaction (the natural place to also feed an FTS row).
- VERSIONS 0.1.24 explicitly deferred "native content/FTS indexing" — this plan is that work.

## Phases

### Phase 0 — Spike + baseline
- Prototype an FTS5 virtual table on a real index; compare query latency + result parity vs the `TokenIndex` path on a fixture corpus. Record token-index build/maintenance cost as the baseline to beat. Decide FTS5 tokenizer (`unicode61` vs `trigram`) against tsift's identifier-heavy queries (the exact-identifier path already uses `rg -F`; FTS must not regress that).

### Phase 1 — FTS5 schema (additive, unused)
- Add `CREATE VIRTUAL TABLE content_fts USING fts5(path UNINDEXED, body, tokenize=...)` plus a `meta` schema-version bump. External-content vs contentless: prefer storing `body` so BM25 + snippets work without re-reading files.
- Keep it written-but-unread so existing search is untouched.

### Phase 2 — Populate incrementally
- In the `apply_changes` New/Modified branch (where source is already read), upsert the file's content into `content_fts`; delete on Deleted; clear on `rebuild`. Mirror the `file_zonemap` lifecycle exactly (same transaction, same delete sites).

### Phase 3 — Query path behind a flag
- Add an `index.db`-backed search that runs an FTS5 `MATCH` + BM25 rank, **zone-map-pruned** (`files_possibly_containing_kind` / a new `files_in_path_scope`) for kind/path-filtered queries. Gate behind a config/env flag; default stays `TokenIndex`.
- **Soundness gate:** on a fixture corpus, FTS results ⊇ the relevance set; for shared queries, assert the flagged path's top-K matches the `TokenIndex` path's top-K (or document deliberate ranking differences). This is where the zone-map prune earns its `pruned == unpruned` regression.

### Phase 4 — Cutover + retire TokenIndex
- Flip the default to the FTS5 path; remove the JSON `TokenIndex` build/cache once parity holds. Update the rewrite/hook surfaces and `search-cache` docs.

### Phase 5 — Ship
- `SPEC.md` + `specs/*` search section, `VERSIONS.md` entry, `make check`, build + install, commit + push — **operator-gated**.

## Risks / invariants
- **No silent hit loss:** every phase keeps the `TokenIndex` path authoritative until Phase 4 parity is proven. FTS tokenizer choice must not regress identifier/exact lookups.
- **Index size:** storing `body` in FTS5 grows `index.db`; measure in Phase 0 and consider contentless-with-reread if size is a problem.
- The `file_zonemap` substrate (landed, `tsift f2bbcc7`) is the pruning layer for Phase 3 — no rework needed there.
