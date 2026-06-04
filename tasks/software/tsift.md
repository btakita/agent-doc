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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-023641.md`*

Compacted content:
- Archived 1 response topic(s): surreal-skip-load
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #astnav-autoindex-oc auto-reindex on plugin load — glm-5.1

> **Queue prompt:**
>
> Please benchmark surrealdb vs sqlite


Implemented auto-reindex for OpenCode in `packages/opencode-tsift/src/index.js`:

- On plugin load and `installation.updated` hook, `ensureIndexFresh()` runs `tsift status --json` to check index state
- If `index.state` is `"stale"` or `"missing"`, it runs `tsift status --fix` to reindex
- Errors are logged via `ctx.client.app.log` but never block plugin load
- Gracefully handles missing `tsift` binary (no-op)

**Files changed:**
- `packages/opencode-tsift/src/index.js` — added `ensureIndexFresh()` with freshness check + auto-reindex
- `packages/opencode-tsift/test/install.test.js` — 3 new tests (fresh-skip, missing-tsift, hook-return)
- `SPEC.md` — documented auto-reindex behavior

**Verification:** `npm test` — 8/8 pass (3 new + 5 existing)
What are the results of the surrealdb vs sqlite benchmark?

### Re: surrealdb vs sqlite benchmark results — glm-5.1

#### Most recent benchmark (2026-06-02, from SPEC.md)

Three backend-eval --candidate surrealdb --full-projection cache-hit samples. Promotion decision: hold.

| Workload | SQLite (median) | SurrealDB (median) | Verdict |
|---|---|---|---|
| Real | 2.6 ms | 573.2 ms | SurrealDB ~220x slower |
| Full-projection | 544.2 ms | 535.0 ms | Roughly competitive |
| High-degree | 11.0 ms | 7.4 ms | SurrealDB faster |
| Deep-chain | 12.0 ms | 4.3 ms | SurrealDB ~3x faster |
| Refresh/load (full-proj) | 541.8 ms | 82.7 ms | SurrealDB 6.5x faster |
| Real refresh/load | 0.18 ms | 88.6 ms | SurrealDB 492x slower |

#### Why promotion is held

1. Real workload blocked: SQLite 2.6 ms vs SurrealDB 573.2 ms. The real (session-scoped) workload is the hot path for agent-doc dispatch, and SurrealDB per-store tokio runtime + SurrealKV open cost dominates at small graph sizes.
2. Operation-level gates fail: edge lookup, edge-property scan, incident edges, neighborhood, and path tiers all regress on the real workload even when aggregate full-projection and synthetic totals look competitive.
3. Refresh/load is a bright spot: SurrealDB batch projection load (82.7 ms vs SQLite 541.8 ms on full-projection) shows it can win on bulk write/load, but the real workload does not benefit because the graph is small and already cached.

#### Fixture gap

fixtures/graph-db-performance-history.json has no SurrealDB entries yet (only SQLite, DuckDB, FalkorDB, Kuzu, Ladybug). Review item #surreal-fixture tracks recording SurrealDB samples into the fixture.

#### SQLite baseline from fixture (full-projection, 952k rows)

edge_lookup 15.8 ms, incident_edges 0.34 ms, path_max_hops 0.065 ms, conflict_matrix 817 ms, total 22.96 s (refresh-dominated).
<!-- agent:boundary:3adc24b4 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ehj2] #surreal-skip-load-test Add regression test for deferred index load
- [ ] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
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
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
