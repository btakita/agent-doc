# Plan: native SQLite FTS5 content index in `index.db`

**Source:** `#b56m` "do the Best option" (2026-06-20). The zone-map audit found tsift's lexical search runs on a separate JSON `TokenIndex`, so the zone-map substrate (`file_zonemap`, already landed) has no consumer. The architecturally coherent fix — and the context that makes the zone-map pay off — is to move the inverted token index **into `index.db` as SQLite FTS5**. This retires the parallel `TokenIndex`, gives in-DB BM25 maintained incrementally alongside `symbols`/`file_state`, and lets the zone-map prune the FTS scan by kind/path/mtime.

This is a **real multi-phase project**, not a one-cycle change. Each phase is independently shippable and behavior-preserving until the final cutover. **Nothing here is started yet** — this is the plan for operator scheduling.

## Current state (grounded)
- Lexical search: `packages/tsift-search/src/sift.rs` — `Sift::search` loads/builds a `TokenIndex` (JSON cache under `.tsift/search-cache/token-index.json`), prunes via `files_matching_any(query_tokens)`, then reads + `score_file`s each candidate. `TokenIndex::build` walks the tree; rebuilt when the cache is stale.
- Index DB: `packages/tsift-index/src/index.rs` — `file_state`, `symbols`, `file_zonemap` (new), `call_edges`, `route_nodes`, `dir_state`, `meta`. Incremental apply path already reads each changed file's source + symbols in one transaction (the natural place to also feed an FTS row).
- VERSIONS 0.1.24 explicitly deferred "native content/FTS indexing" — this plan is that work.

## Progress (2026-06-20)

**Phases 0, 1, 2, and Phase 3 core (3a) landed** (tsift submodule, written-but-unread; `make check` green, release built + installed). Phase 3 engine + both soundness gates are proven; the remaining **Phase 3b** (CLI env-flag wiring + `SearchResponse` construction) is the next increment, then Phase 4 cutover.

### Phase 0 spike findings (settled)
- **FTS5 availability: confirmed, no new dep.** Bundled `rusqlite` (0.40, `features=["bundled"]`) compiles SQLite with FTS5. Already used in-repo by `tsift-tokensave` (`nodes_fts`) and `tsift-memory` (`memory_events_fts`) with passing tests — reference implementations exist.
- **Tokenizer = `unicode61`.** Matches the current `TokenIndex` tokenization exactly (split on non-alphanumeric, lowercase, no min-length — `sift.rs:442`), so Phase 3 top-K parity is achievable. `trigram` deferred (enables substring matching but isn't needed for parity; exact-identifier lookups go through external `rg -F` — `tsift-cli` `exact_search_command` — so **zero exact-lookup regression risk**).
- **Storage = inline `body`, not external-content.** Earlier "external-content saves space" reasoning is **wrong for tsift**: there is no pre-existing body table to reference (`symbols` stores byte spans, not bodies), so external-content would duplicate the body anyway. Real choice is inline-body (simple, snippets/BM25 work, ~+12 MB on a 22.2 MB `index.db`) vs contentless (smallest, but needs a rowid→path side-table + disk re-reads for snippets). Chose **inline-body** for Phase 1; contentless recorded as a Phase 4 size-optimization if `index.db` growth becomes a problem.
- **Baseline / motivation.** The JSON `TokenIndex` (`HashMap<String, Vec<String>>` at `<cache_dir>/token-index.json`) decides staleness by **file existence only** (`sift.rs:350`) — it never rebuilds on content change, a real correctness weakness FTS5 fixes by being maintained transactionally in `apply_changes`.
- **Scope difference (Phase 3 parity item).** `apply_changes` only source-reads **recognized-language** files (`if let Some(lang)`, `index.rs`) — that includes rust/markdown/bash/js/ts (live check: `content_fts` == `file_state` == 199 on the tsift repo, since every tracked file here is recognized), but **excludes unrecognized extensions** (e.g. `.txt`, extensionless). `TokenIndex` instead indexes *all* gitignore-respecting candidate files regardless of language. The Phase 3/4 cutover must either extend the index.db read path to unrecognized text or consciously narrow lexical search to recognized-language files (arguably desirable for a code-RAG tool). Documented inline at the FTS insert site.
- **Efficiency follow-up.** Per-file `DELETE FROM content_fts WHERE path = ?` is an O(n) scan (UNINDEXED column). Correct and fine at current scale; a deterministic rowid (or external-content over a rowid'd table) is a Phase 4 optimization if incremental re-index cost shows up.

## Phases

### Phase 0 — Spike + baseline ✅ DONE
- Prototype an FTS5 virtual table on a real index; compare query latency + result parity vs the `TokenIndex` path on a fixture corpus. Record token-index build/maintenance cost as the baseline to beat. Decide FTS5 tokenizer (`unicode61` vs `trigram`) against tsift's identifier-heavy queries (the exact-identifier path already uses `rg -F`; FTS must not regress that).

### Phase 1 — FTS5 schema (additive, unused) ✅ DONE
- Add `CREATE VIRTUAL TABLE content_fts USING fts5(path UNINDEXED, body, tokenize=...)` plus a `meta` schema-version bump. External-content vs contentless: prefer storing `body` so BM25 + snippets work without re-reading files. **Landed:** `content_fts(path UNINDEXED, body, tokenize='unicode61')` + `meta['content_fts_version']='1'` (`index.rs`).
- Keep it written-but-unread so existing search is untouched.

### Phase 2 — Populate incrementally ✅ DONE
- In the `apply_changes` New/Modified branch (where source is already read), upsert the file's content into `content_fts`; delete on Deleted; clear on `rebuild`. Mirror the `file_zonemap` lifecycle exactly (same transaction, same delete sites). **Landed:** `insert_fts`/`delete_fts` prepared statements mirror `insert_zonemap`/`delete_zonemap` at every site; 3 lifecycle tests (populate code-files-only + MATCH/BM25, modify-replaces-no-dup + delete-removes, rebuild-repopulates).

### Phase 3 — Query path behind a flag — 🟡 CORE DONE (engine + soundness gates); 3b wiring pending
- Add an `index.db`-backed search that runs an FTS5 `MATCH` + BM25 rank, **zone-map-pruned** (`files_possibly_containing_kind` / a new `files_in_path_scope`) for kind/path-filtered queries. Gate behind a config/env flag; default stays `TokenIndex`.
- **Soundness gate:** on a fixture corpus, FTS results ⊇ the relevance set; for shared queries, assert the flagged path's top-K matches the `TokenIndex` path's top-K (or document deliberate ranking differences). This is where the zone-map prune earns its `pruned == unpruned` regression.
- **Landed (3a, engine + gates):** `IndexDb::content_fts_search_pruned(query, kind, limit)` (zone-map-pruned BM25; the zone-map's first consumer) + `sift::fts_match_query` (OR-union query translation preserving TokenIndex semantics). Two soundness tests: `content_fts_search_pruned_is_sound` (pruned ⊆ unpruned, no false negatives, sound retention of zonemap-less files) and `fts_content_index_supersets_token_index_candidates` (FTS ⊇ TokenIndex). Finding: FTS5 defaults a multi-token MATCH to an adjacency *phrase*; `fts_match_query` ORs the tokens to keep union semantics.
- **Remaining (3b):** wire the env-flagged search path into the CLI (`run_sift_search` / `__search-worker`), constructing a `SearchResponse` from `content_fts_search` (resolve `.tsift/index.db`, line numbers/previews, BM25-vs-substring top-K reconciliation), default off. A `files_in_path_scope` zone-map prune for path-filtered queries is also a 3b add. THEN the Phase 4 cutover can flip the default.

### Phase 4 — Cutover + retire TokenIndex
- Flip the default to the FTS5 path; remove the JSON `TokenIndex` build/cache once parity holds. Update the rewrite/hook surfaces and `search-cache` docs.

### Phase 5 — Ship
- `SPEC.md` + `specs/*` search section, `VERSIONS.md` entry, `make check`, build + install, commit + push — **operator-gated**.

## Risks / invariants
- **No silent hit loss:** every phase keeps the `TokenIndex` path authoritative until Phase 4 parity is proven. FTS tokenizer choice must not regress identifier/exact lookups.
- **Index size:** storing `body` in FTS5 grows `index.db`; measure in Phase 0 and consider contentless-with-reread if size is a problem.
- The `file_zonemap` substrate (landed, `tsift f2bbcc7`) is the pruning layer for Phase 3 — no rework needed there.
