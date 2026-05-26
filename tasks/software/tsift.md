---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Answered why the session-share tsift DB appeared stale. No source changes; refreshed indexes with tsift status --fix and confirmed session-share is now fresh while workspace remains stale only because the FDE-Project-Interview scope is missing.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260526-165359.md`*

Compacted content:
- Archived 1 response topic(s): #f14h automatic graph refresh
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to make the graph + neighborhoods faster + more effective?
### Re: graph + neighborhood next steps — gpt-5

Top backlog item: `#gneighgate`.

I added five follow-up items in priority order. The short version is: do not start with FalkorDB or higher hop defaults. The current highest-leverage work is to measure `graph-db neighborhood` directly, then optimize the SQLite neighborhood query shape if the new gate proves it is hot.

Why that is first: `SqliteGraphStore::paged_neighborhood` currently runs one recursive walk to page reachable nodes, then `neighborhood_edges_for_page` runs another recursive walk to recover edges for that page. That is correct and bounded, but it is the most obvious neighborhood-specific place where large-depth or high-degree queries can do duplicate traversal work. The next item should add repeated real/full-projection/synthetic neighborhood metrics, then collapse that into one bounded reachable-set path if the measurements justify it.

The effectiveness work is separate: local `graph-db refresh` warned that the summary cache is empty, so semantic rows are unavailable, and `tsift communities --json` reported `tagpath_state=missing` with no annotated communities. Until semantic rows and handles are present, neighborhoods can be fast but still feel shallow. That is why `#gsemrows` and `#gnbrank` are next: make missing semantic/tagpath readiness explicit, then rank neighborhood output by depth, edge kind, community membership, semantic relation, handle freshness, duplicate-name precision, and coverage.

Operationally, the local graph DB also needs cleanup: after refresh, `graph-db doctor` was healthy but warned about `184,722` tombstones versus `27,375` live rows. That is not the first neighborhood-query fix, but it is real graph maintenance work and belongs in the backlog as `#gtombops`.

On hop caps: keep the user-facing default at 64 until the existing hop-cap promotion gate has repeated real, full-projection, and synthetic deep-chain evidence. On FalkorDB or other backends: keep them behind backend-eval promotion gates until SQLite's current neighborhood and semantic-readiness surfaces are measured and stable.

Proof for this cycle:

- Changed paths: session document/backlog only, through `agent-doc finalize`; no tsift source files changed.
- Commands: `tsift status --fix`, source inspection with `tsift --envelope search/source-read`, `tsift graph-db --json --path . status`, `tsift graph-db --json --path . refresh`, `tsift graph-db --json --path . doctor`, `tsift communities --json`.
- Verification: no code tests were run because this was a planning/backlog response with no code changes. Graph DB status/refresh/doctor succeeded locally; doctor reported only tombstone-retention/compaction warnings.
- Confidence: medium-high for backlog ordering; low confidence in any specific latency win until `#gneighgate` adds repeated neighborhood metrics.
- Escalation: none.
❯ Does tsift auto-update the index if the index is stale?
### Re: stale index auto-update — gpt-5

Yes, for the main indexed read paths. `tsift search` auto-updates by default: it does a freshness precheck, and if the resolved local/scoped index is stale or missing, it incrementally rebuilds it before running the normal indexed search. `--autoindex` still exists, but it is just the explicit form of the default behavior. `--no-autoindex` disables that and fails fast when an existing index is stale.

The read-only graph commands share that freshness chain too: `graph`, `communities`, `path`, and `explain` resolve the local/scoped target, refresh missing or stale indexes before opening `index.db`, then read through the resilient read-only path. If another tsift writer already holds `index.lock`, they skip the refresh and continue against the existing read-only snapshot when one exists, with a warning that results may lag.

`tsift status` is slightly different. Plain `status` reports stale state and recommendations; it can auto-create missing workspace-scope indexes when the source root exists, but it does not generally refresh every stale index unless you run `tsift status --fix`. In this workspace, `status --fix` refreshed the existing scopes but still reported workspace stale because the configured `FDE-Project-Interview` scope is missing on disk, so there is no index it can build for that scope.

Also, exact live-file search (`tsift search --exact`) does not need the index in the same way, so stale-index auto-refresh is mainly about indexed search/graph queries.

Proof for this cycle:

- Changed paths: session document only, through `agent-doc finalize`.
- Commands: `agent-doc preflight`, `agent-doc plan`, `tsift status`, `tsift status --fix`, `tsift status` in `src/tsift`, `rg`/`sed` over `SPEC.md` and `src/main.rs`, and `tsift --envelope search` for stale-index behavior.
- Verification: no code tests were run because this was an explanatory response with no source changes.
- Confidence: high; behavior is documented in `SPEC.md` and reflected in `src/main.rs` search/status handling.
- Escalation: none.
❯ Why was the tsift db in session-share ~5 days stale?
### Re: session-share stale index — gpt-5

It was stale because tsift indexing is demand-driven, not a background daemon. A scope can sit at its last indexed state until one of these happens: a command targets that scope and autoindexes it, `tsift status --fix` runs, or someone explicitly runs `tsift index` / `tsift index --workspace`.

So the likely chain is:

1. `session-share` had been indexed during an older workspace-wide index pass.
2. Agent-doc/session-share work kept changing tracked markdown files afterward.
3. No tsift search/graph command targeted `session-share` for a while, so nothing forced that scope to refresh.
4. Plain `tsift status` reported the stale state; it does not generally refresh all stale scopes unless `--fix` is used.

For the current state, `tsift index --check --submodule session-share --json` reported exactly one stale tracked file: `tasks/20260526-orbit-arch.md`. The index DB was refreshed at `2026-05-26 15:57:26 -0400`, then that file changed at `2026-05-26 15:59:51 -0400` via `agent-doc(20260526-orbit-arch)`, so it immediately became stale again. `src/session-share` also currently has local edits to `tasks/.gitignore` and `tasks/20260526-orbit-arch.md`.

After running `tsift status --fix`, `session-share` is fresh again. The overall workspace still says `index: stale` because the configured `FDE-Project-Interview` scope is missing on disk, so tsift cannot build its missing index. That is separate from `session-share`.

Net: nothing mysterious happened to the SQLite DB; it was old because no demand-triggered refresh hit that scope, and active session-share document edits made it stale again shortly after refresh.

Proof for this cycle:

- Changed paths: session document only, through `agent-doc finalize`.
- Commands: `agent-doc preflight`, `agent-doc plan`, `tsift status`, `tsift status --json`, `tsift index --check --submodule session-share --json`, `git -C src/session-share status --short`, `stat`, `sqlite3` inspection of the session-share index DB, `git -C src/session-share log`, and `tsift status --fix`.
- Verification: no code tests were run because this was an explanatory response with no source changes. `tsift status --fix` completed and refreshed `session-share`; workspace staleness remains only from the missing configured scope.
- Confidence: high for the current stale cause; medium for the historical “~5 days” cause because the old DB timestamp was overwritten by the refresh, but the demand-driven behavior explains how that age accumulates.
- Escalation: none.
<!-- agent:boundary:5db4d7c9 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gneighgate] Add a neighborhood performance/quality gate, then optimize `paged_neighborhood`: run repeated real + full-projection + synthetic high-degree/deep-chain `graph-db neighborhood` samples, collapse the current node-page and edge-page recursive walks into one bounded reachable-set path if it is hot, and lock SQLite query-plan diagnostics plus Convex parity.
- [ ] [#gsemrows] Make semantic rows and tagpath handles part of graph effectiveness: when `graph-db refresh` warns the summary cache is empty or `communities` reports `tagpath_state=missing`, surface a fail-closed/actionable readiness state, then run `summarize --extract` + refresh and prove neighborhood/evidence ranking improves.
- [ ] [#gnbrank] Add ranked neighborhood output: score neighbors by depth, edge kind, community co-membership, semantic relation, source-handle freshness, duplicate-name precision, and handle coverage; gate with community-search quality metrics before changing defaults.
- [ ] [#gtombops] Run and document the graph tombstone cleanup workflow on the tsift graph DB: Convex reconcile, refresh, compact, then record status/doctor/file-size/tombstone-scan deltas from the current 184,722 tombstone / 27,375 live-row state.
- [ ] [#gbenchneigh] Refresh the graph performance history with dedicated neighborhood rows so future graph-backend or hop-cap decisions compare `neighborhood`, `path_max_hops`, evidence, conflict-matrix, and dispatch-trace together instead of optimizing one surface blind.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
