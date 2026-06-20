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
queue: start
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

### Re: do #015t phase 4b — freshness threading shipped — opus-4-8

> **Queue prompt:**
>
> :pushpin: [#015t]: do phase 4b


You queued `do phase 4b`, so I worked it. On inspection the three Phase 4b items had very different shapes, so I did the one that's genuinely safe mechanical cleanup and deferred the other two with rationale rather than forcing them.

**Done — item 3 (perf cleanup; tsift `475262b`, superproject `3ee95df15`):** threaded the precheck's freshness verdict into the search path as `fts_index_fresh: Option<bool>` (through `run_sift_search` / `run_search_with_timeout` / `__search-worker`), so the FTS path **no longer re-walks the tree with `inspect_read_only` on every search** — `cmd_search` already computed freshness in `precheck_search_indexes`. `Some(true)` → FTS without re-inspect; `Some(false)` (degraded read-only) → live `TokenIndex` fallback; `None` (direct callers) → inspect as before. The worker carries it as `--fts-index-fresh`.

**Deferred, with rationale (not punting effort — these aren't mechanical cleanup):**
- **Retire `TokenIndex`** — the Phase 4 freshness fallback *needs* the live-walk `TokenIndex` to serve **live** results in the degraded-read-only case (stale index held by a concurrent writer). Removing it requires an operator/design decision: keep `TokenIndex` for that path, or switch degraded-read-only to live `rg -F` exact. The redundant `token-index.json` *cache* is already effectively retired on the default path.
- **`files_in_path_scope` path-prune** — net-new capability (the FTS path searches the whole index, same as the old `TokenIndex`; `--path` sub-narrowing of file hits was never applied here), so it's an enhancement, not cleanup.

**Verification:** `make check` green (exit 0; fixed a clippy `too_many_arguments` with the codebase's standard `#[allow]`); new test `run_sift_search_honors_threaded_freshness_verdict` (Some(true)→fts, Some(false)→lexical); built + installed; live-verified the worker path still returns `strategy: fts`.

**NEXT:** Phase 4b's two deferred items need your call (keep/retire `TokenIndex` for degraded-read-only; whether the path-prune enhancement is worth it), and **Phase 5** is the operator-gated crates.io release. Say **"do #015t"** with a direction, or schedule the release.

### Re: would token-index.json be useful if the behavior is fixed, or purely a redundant vestige? — opus-4-8

> **Queue prompt:**
>
> :pushpin: Would token-index.json be useful if the behavior is fix or is it purely a redundant vestige?


> **Queue prompt:**
>
> :pushpin: Would token-index.json be useful if the behavior is fix[ed] or is it purely a redundant vestige?

**Short answer: purely a redundant vestige.** Fixing the invalidation behavior would make `token-index.json` *correct*, but not *useful* — because the reason to delete it (structural redundancy) is untouched by any behavior fix, and a correct freshness gate actually makes the other two problems *worse*.

Walk the three problems from the prior answer against the hypothetical "behavior is fixed":

1. **Never invalidates** — this is the *only* one a behavior fix addresses. Add an mtime/content-hash gate and the cache returns correct results. But "correct" here just means it now returns *what `.tsift/index.db` already returns*. Correctness is table stakes the real index already meets.
2. **All-or-nothing whole-tree rebuild** — a working freshness gate makes this strictly *worse*. A cache that actually invalidates means it rebuilds the entire `HashMap<token, Vec<path>>` inversion on *any* source change, while `.tsift/index.db` updates **incrementally**. You'd be paying full-rebuild cost on every edit to reproduce results the incremental FTS5 index already has for free.
3. **Redundant second index** — untouched by any behavior fix, and this is the fatal one. After the fix you still have two lexical indexes over the same corpus: FTS5/BM25-ranked, incrementally maintained, freshness-gated (`index.db`) **vs** a flat token→files HashMap with no ranking and whole-tree rebuilds. The fixed JSON is strictly dominated on every axis — it's never the better answer for any default-path query.

So as a **default-path search structure**: delete it. Fixing the behavior is polishing something you should remove.

**The one non-vestigial sliver is the engine, not the cache.** The degraded-read-only fallback (a concurrent writer holds a stale `index.db`) genuinely needs *live* results, and a fresh in-memory `TokenIndex::build` is a legitimate way to serve that. But two things follow:

- That path is **live by definition**, so it must NOT read a persisted `token-index.json` at all — caching a must-be-live path is wrong by construction. Even in its one defensible use case, the **JSON file is dead**; only the build engine has a possible role.
- And `rg -F` exact would serve that same fallback without maintaining a bespoke inversion at all.

**Bottom line / recommendation for #015t:**
- `token-index.json` (the persisted cache) → **delete unconditionally**, in every scenario. The behavior fix is not worth doing.
- `TokenIndex::build` (the engine) → keep *only* if you want an in-process degraded-read-only live fallback; otherwise replace that one call site with `rg -F` and drop the type entirely.

That collapses the two deferred phase-4b items into a single decision: **drop the cache; keep-or-replace the engine for degraded read-only.** Say **"do #015t"** with that direction (delete cache + which fallback you want) and I'll ship it.

### Re: implemented your recommendations — token-index.json persistence deleted — opus-4-8

> **Queue prompt:**
>
> :pushpin: Implement your recommendations.

**Done — shipped + pushed** (tsift `34bbbb1`, superproject pointer `8d6b392dd`, both on `origin/main`; lockstep `0.1.72 → 0.1.73`).

Implemented the high-value, low-risk half of my recommendation — **deleting the broken `token-index.json` cache** — and deliberately scoped *out* the risky protocol surgery:

- **Persistence deleted.** `load_or_build_token_index` is gone; the `TokenIndex` fallback now **always rebuilds the inversion in-memory** (`TokenIndex::build`). Removed `TokenIndex::save`/`load` (the JSON persistence API) and the `Serialize`/`Deserialize` derive — the type is in-memory only now. The cache that returned `token-index.json` whenever it merely existed (no mtime/content invalidation → stale-forever) no longer exists.
- **Engine kept** (my recommended default). `TokenIndex::build` stays as the degraded-read-only live fallback. The companion decision — keep it vs. replace that one call site with a live `rg -F` walk — is the **one operator call I left open** (it changes degraded-read-only behavior; not mechanical).
- **Scope I intentionally did NOT touch:** the generic `cache_dir` engine hook and the `__search-worker --cache-dir` subprocess protocol. That plumbing is a *separate concern* from the token-index cache (it's woven through the worker arg protocol and several tests); ripping it out is a riskier refactor for zero correctness gain. It's retained as a home for a future, *properly-invalidated* cache and no longer backs the token index — exactly the "no cache; build live" shape I recommended.

**Verification:**
- `make check` green — clippy `--all-targets -D warnings` clean; full suite passing (tsift-cli 397 + 190, tsift-search 15, etc.).
- New regression test `search_reflects_files_added_after_first_search` — a file created *after* the first search is now found (the old cache hid it); flipped the existing assert to prove **no** `token-index.json` is persisted.
- **Live-verified** the installed `0.1.73` binary: adding a second file mid-session surfaces it immediately, and no `token-index.json` is written anywhere.
- SPEC (`specs/architecture.md`) + VERSIONS updated; release built + `cargo install`ed (`tsift 0.1.73`).
- **CI:** run `27879975858` is *in progress* at finalize; local `make check` is the closeout evidence — will not block on it.

**NEXT (Phase 4b remainder — both need your call, not blocked work):**
1. **Degraded-read-only engine:** keep `TokenIndex::build` (current) or replace it with a live `rg -F` exact walk and delete the type. Default is "keep" — say the word to switch.
2. **`files_in_path_scope` path-prune:** captured as a `[recommended]` backlog item — a net-new *enhancement* (not cleanup); skip unless you want `--path` sub-narrowing of FTS hits.

Then **Phase 5** is the operator-gated crates.io release. Say **"do #015t"** with a direction (or "release") to continue.
<!-- agent:boundary:90ca0241 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: Implement your recommendations.~~
- do [#015t]
- :pushpin: Implement your recommendations.
- do [#ve5f]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#ve5f] [recommended] #015t Phase 4b enhancement: add a files_in_path_scope path-prune so --path sub-narrows FTS hits (net-new capability, not cleanup; implement only if the operator wants --path sub-narrowing).
- [ ] [#015t] [operator-verify] FTS5 content index (plan: tasks/software/plan-tsift-fts5-content-index.md). Phases 0/1/2 + 3a + 3b + Phase 4 cutover + Phase 4b item 3 (thread precheck freshness) DONE + pushed (tsift 475262b). FTS5 index.db BM25 is the DEFAULT lexical path; run_sift_search/run_search_with_timeout/__search-worker take an fts_index_fresh verdict so the FTS path no longer re-walks the tree on every search; degraded read-only routes to the live TokenIndex fallback. NEXT (Phase 4b remaining, optional): (a) retire TokenIndex build/cache — DEFERRED, the degraded-read-only fallback still needs the live-walk; needs an operator/design decision on degraded-read-only behavior (keep TokenIndex vs live rg -F); (b) files_in_path_scope zone-map path-prune — net-new enhancement, not cleanup. Then Phase 5 = operator-gated crates.io release. Say "do #015t" for (a)/(b) or to ship.
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
