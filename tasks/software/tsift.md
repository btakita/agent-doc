---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260620-064417.md`*

Compacted content:
❯ - Archived 8 response topic(s): DuckDB internals; drain #sbtk / #wzg6; implement plan-tsift-zonemap-segment-stats.md; 5 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54...

### Re: session compaction — state recap — opus-4-8

The exchange was compacted (8 response topics archived to `…-20260620-064417.md`); committing it left a prompt-only summary tail, so this response closes the turn. No new work was requested.

**Current state of `#015t` (FTS5 content index):**
- **Phases 0/1/2 + Phase 3 core (3a) — done + pushed** (tsift `4105aba`, superproject `c64241785`): `content_fts` scaffold, `content_fts_search_pruned` (zone-map's first consumer), `fts_match_query` (OR-union translation), both soundness gates green. All written-but-unread — zero search-behavior change.
- **`#ph7n` — done** (crates.io trusted-publishing runbook).
- **Next (operator-scheduled):** Phase 3b — wire the env-flagged FTS path into the CLI `SearchResponse`. Say **"do #015t"** to continue. The queued `do [#015t]` is the go-mode re-mirror of the operator-verify umbrella (non-drainable), not a fresh directive.

**Unchanged blocker (out of scope here):** `stale_install` — installed agent-doc `0.34.20` predates source `d48b30ea`, `src/agent-doc` has uncommitted WIP. Durable fix is an operator `make install` in `src/agent-doc` + supervisor restart.

### Re: queue re-mirror — no new directive — opus-4-8

Nothing new was requested. The only change this cycle is the go-mode re-mirror of `do [#015t]` back into the queue. Preflight classifies it correctly: `queue_drainable_head_count: 0`, `queue_continuation_required: false` — `#015t` is `[operator-verify]` and is deferred from auto-drain, so the in-session loop does not take it. I'm not re-running work and not looping.

**`#015t` state (unchanged):** Phases 0/1/2 + Phase 3 core (3a) done + pushed (tsift `4105aba`). NEXT = Phase 3b — wire the env-flagged FTS search path into the CLI (`run_sift_search` / `__search-worker` + `SearchResponse`), default off. It's operator-scheduled and multi-session: say **"do #015t"** / **"do phase 3b"** to start it. The queued `do [#015t]` is the umbrella mirror, not that directive.

**Standing blocker (out of scope here):** `stale_install` — installed agent-doc `0.34.23` vs source WIP in `src/agent-doc`. Durable fix is an operator `make install` in `src/agent-doc` + supervisor restart.

### Re: do #015t — Phase 3b shipped — opus-4-8

You chose **Advance Phase 3b now**, so I implemented and pushed it (the umbrella `#015t` stays `[operator-verify]` because Phase 4/5 remain).

**What landed** (tsift `f69f6a6`, superproject pointer `38bef064a`):
- **`sift::fts_search(db_path, root, query, limit)`** — builds a `SearchResponse` from the `index.db` content hits. **BM25 owns file ordering**; the lexical `score_file` picks each file's representative line + snippet from the **inline FTS body** (no disk re-read) — the plan's "BM25-vs-substring top-K reconciliation". `strategy="fts"`.
- **`IndexDb::content_fts_search_with_body`** — BM25 search returning the inline body so the CLI can reconcile ranking with substring line selection.
- **`run_sift_search` gate** — opt-in `TSIFT_FTS_SEARCH` (`1/true/yes/on`), resolves `<root>/.tsift/index.db`, **falls back to lexical on a missing db** (sound: never worse than today). Both the in-process and `__search-worker` subprocess routes pass through it. **Default off** — the JSON `TokenIndex` stays authoritative until the Phase 4 cutover.

**Verification:**
- `make check` green (clippy + full suite, exit 0).
- New tests: `fts_search` response/ordering/empty (tsift-search), flag parser + default-off lexical path (tsift-cli), `content_fts_search_with_body` (tsift-index).
- **Live-verified** on the tsift repo: `tsift search "fts_match_query" --strategy lexical` returns `strategy: lexical`; with `TSIFT_FTS_SEARCH=1` it returns `strategy: fts` with correct line numbers/snippets on the actual matching lines. Default `tsift search` (exact/ripgrep) is unaffected.
- Release built + `cargo install`ed (local `tsift` refreshed). CI run `27864747385` was **in_progress** at closeout — closing from local evidence per the in-progress-CI rule.

**NEXT = Phase 4 cutover** (operator-scheduled, multi-session): flip the default to the FTS path, add a `files_in_path_scope` zone-map prune for path-filtered queries, retire the JSON `TokenIndex`; then Phase 5 ship (SPEC/VERSIONS). Say **"do #015t"** / **"do phase 4"** to continue.

### Re: do #015t phase 4 — FTS5 cutover shipped — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#015t] phase 4


You edited the queue head to `do [#015t] phase 4`, so I did the **Phase 4 cutover** (tsift `333a73c`, superproject pointer `f44e1ade9`).

**FTS5 `index.db` BM25 is now the DEFAULT lexical search path.** `tsift search --strategy lexical` (and the auto-resolved lexical default for multi-token queries) returns `strategy: "fts"`. The JSON `TokenIndex` is demoted to a fallback.

**How it stays sound** (the key design points):
- `run_sift_search` uses FTS only when `<root>/.tsift/index.db` exists **AND is fresh** (`index_db_is_fresh_for_fts`). The normal flow's `precheck` + autoindex guarantees that in the common case.
- **Freshness gate, not just existence** — a missing/corrupt index.db (empty placeholder) or a **stale** one (concurrent writer → autoindex degraded to read-only) falls back to the live `TokenIndex`. This caught a real regression in `make check`: degraded read-only search must serve **live** results, which the stale FTS index couldn't. The fallback fixes it.
- Ranking shifts substring-position → **BM25 by design**; the Phase 3 gate proved candidate coverage (FTS ⊇ TokenIndex), so no hit is silently dropped — only the order changes.
- Escape hatch: `TSIFT_FTS_SEARCH=0` (`0/false/no/off`) forces the legacy path during the transition.

**Verification:** `make check` green (exit 0); new + updated tests (default-fts / no-index fallback / falsy-escape-hatch parser, plus three integration tests updated for the cutover); SPEC (`specs/architecture.md`) + VERSIONS (BREAKING CHANGE) updated; release built + `cargo install`ed; **live-verified** — default `--strategy lexical` → `strategy: fts`, `TSIFT_FTS_SEARCH=0` → `strategy: lexical`.

**NEXT = Phase 4b cleanup** (agent-doable): retire the `TokenIndex` build/cache + `token-index.json` once the no-index path auto-indexes, add a `files_in_path_scope` path-prune, and thread the precheck freshness into `run_sift_search` to drop the redundant inspect walk. Then Phase 5 is the operator-gated crates.io release. Say **"do #015t"** / **"do phase 4b"** to continue.
<!-- agent:boundary:7ff87b79 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#015t] [operator-verify] FTS5 content index (plan: tasks/software/plan-tsift-fts5-content-index.md). Phases 0/1/2 + 3a + 3b + Phase 4 CUTOVER DONE + pushed (tsift 333a73c). FTS5 index.db BM25 is now the DEFAULT lexical path (run_sift_search -> sift::fts_search when root index.db exists AND is fresh); TokenIndex demoted to fallback (no-index / un-indexed / direct caller), escape hatch TSIFT_FTS_SEARCH=0. Freshness gate keeps a stale/corrupt index from serving stale results. SPEC + VERSIONS (BREAKING) updated. make check green, live-verified (default->fts, =0->lexical). NEXT = Phase 4b cleanup (agent-doable): retire TokenIndex build/cache + token-index.json once the no-index path auto-indexes; add files_in_path_scope path-prune; thread precheck freshness to drop the redundant inspect walk. Then Phase 5 operator-gated crates.io release. Say "do #015t" / "do phase 4b".
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#1yks] [operator] Finish tsift v0.1.72 publish: (1) bootstrap crates.io Trusted Publishing for the two NEW crates tsift-local-model + tsift-kg (browser/passkey 2FA — the prior 23 crates are configured, these two are not and a brand-new crate's first OIDC publish needs its config to exist; if crates.io rejects pre-config for a nonexistent name, do one manual 'cargo publish -p <crate>' to create it); (2) push the v0.1.72 tag (git tag v0.1.72 08d0f65 && git push origin v0.1.72) to trigger Release CI. Without step 1 the crates.io job fails again on the two new crates.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
