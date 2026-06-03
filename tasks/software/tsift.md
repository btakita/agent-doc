---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-150545.md`*

Compacted content:
- Archived 7 response topic(s): #gfrontierhot Performance Triage; #greadiness Actionable Readiness at Lookup Boundary; #rankdefault Ranked Neighborhood Comparison; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: SurrealDB Substrate Promotion — opencode

**Current state:** SurrealDB is a feature-gated spike (`backend-surrealdb`) using embedded SurrealKV with Rust-side BTreeMap indexes. The 2026-06-02 benchmark shows `promotion=hold` — real-workload total is **220x slower** (573 ms vs 2.6 ms) and refresh/load is **492x slower** (88.6 ms vs 0.18 ms). SurrealDB wins on full-projection refresh (82.7 ms vs 541.8 ms) and synthetic workloads (high-degree, deep-chain), but the real-workload penalty blocks promotion.

**Root causes for the real-workload gap:**

1. **Cold-start tax:** Every `SurrealdbGraphStore::from_rows_file_backed()` creates a fresh tokio runtime + opens SurrealKV. The 88.6 ms refresh is dominated by initialization, not queries. SQLite opens a file handle instantly.
2. **No delta refresh:** `replace_projection_rows()` does `DELETE ALL + INSERT ALL` even when 1 row changed. SQLite does row-hash diffing and writes only deltas.
3. **Per-store tokio runtime:** Each `SurrealdbGraphStore` owns its own `Runtime`. No pooling or reuse across backend-eval runs.
4. **Missing multi-process evidence:** The promotion gate (SPEC.md:304) explicitly requires "multi-process/read-only lock behavior evidence." No tests exist for concurrent SurrealKV access.

**Promotion path (prioritized backlog below):**

The critical unlock is **Priority 1: warm-start / store reuse**. If the 88.6 ms cold-start drops to near-zero on reuse, the real-workload total should collapse to the Rust-side BTreeMap index cost (microseconds), making SurrealDB competitive or faster than SQLite across all four required workloads. That single change likely resolves 4 of 5 failing gates simultaneously.

**Backlog items (priority order):**

1. `#surreal-warm` — Warm-start SurrealDB store: reuse `SurrealdbGraphStore` across backend-eval runs instead of recreating; measure real-workload median with persistent store. If cold-start was the bottleneck, this alone should move real total from ~573 ms to ~3-5 ms.
2. `#surreal-delta` — Delta projection refresh: implement `replace_projection_rows_delta()` with row-hash diffing, matching SQLite's `replace_projection_with_version()` pattern. Only upsert changed rows and delete removed rows instead of full `DELETE + INSERT`.
3. `#surreal-multiproc` — Multi-process lock behavior: write a conformance test that opens N concurrent reader processes + 1 writer against the same SurrealKV path; document lock contention, stale-read behavior, and crash recovery. Required by the SPEC.md:304 promotion gate.
4. `#surreal-rt-pool` — Shared tokio runtime: accept an external `Runtime` reference or use a lazy static instead of per-store `Runtime::new()`. Reduces memory and init cost for multi-store scenarios.
5. `#surrealbulk-close` — Close `#surrealbulk` review item: the bulk transaction path is implemented (`replace_projection_rows`), but the review item is still `[/]`. Re-run benchmarks, update SPEC.md benchmark table, mark done.

**Top of backlog:** `#surreal-warm`
<!-- agent:boundary:138e974f -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- do [#libmod4]
- do [#libmod5]
- do [#opencode-read]
- do [#opencode-grep]
- do [#codex-read]
- do [#libmod3]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#surreal-warm] Warm-start SurrealDB store: reuse SurrealdbGraphStore across backend-eval runs instead of recreating; measure real-workload median with persistent store
- [ ] [#surreal-delta] Delta projection refresh: implement replace_projection_rows_delta() with row-hash diffing matching SQLite's replace_projection_with_version() pattern
- [ ] [#surreal-multiproc] Multi-process lock behavior: write conformance test with N concurrent reader processes + 1 writer against same SurrealKV path; document lock contention, stale-read behavior, and crash recovery (SPEC.md:304 promotion gate)
- [ ] [#surreal-rt-pool] Shared tokio runtime: accept external Runtime reference or use lazy static instead of per-store Runtime::new()
- [ ] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [ ] [#libmod4] Phase 4: Extract Convex (~3,000 lines), Graph DB core (~5,000+) from lib.rs
- [ ] [#libmod5] Phase 5: Extract Context Pack (~1,700), Conflict Matrix (~2,500), Search Budget (~800), Session Review (~500) from lib.rs
- [ ] [#opencode-read] OpenCode: replace Read tool with tsift AST API (tsift --envelope source-read) for source code reading
- [ ] [#opencode-grep] OpenCode: replace grep/rg with tsift search for content search
- [ ] [#codex-read] Codex: adopt tsift AST API for source reading instead of raw file reads
- [ ] [#libmod3] Phase 3: Extract Semantic Edit (~4,000 lines), Traversal/Exploration (~1,300) from lib.rs
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
