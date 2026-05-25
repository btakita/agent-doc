---
agent_doc_session: tsift-v0.1
agent: claude
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
Completed: #gdbrefreshdelta was already closed; #gdbcmcache, #gdbpathpolicy, and #gdbengineproof are implemented in tsift ab9f8a9 and agent-loop e09157f61. Local verification passed (`make check`, `cargo build`, `cargo install --path .`); latest tsift CI run 26372477579 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260525-082913.md`*

Compacted content:
- Archived 6 response topic(s): do [#cachelookupshift]; do [#gdbvacconvex]; queue updated; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #convexscopedeval closed — full-graph default, --scope as escape hatch — opus-4-7

Decision landed (tsift `f328770`, superproject `3c9fcfc08`, v0.1.57). `make check` clean, install upgraded.

#### Verdict — full-graph default, `--scope` escape hatch
Now that `#convexsnapshotscale` (v0.1.56) shipped cursor pagination, the scale rationale for scoped-default is gone. Decision matrix in `plans/convexscopedeval-evidence.md`: 4 of 7 considerations favor full-graph (mental model, cross-submodule queries on Convex side, operational reach, alignment with new snapshot transport); 3 favor scoped (failure isolation, partial-failure recovery, mirrors workspace structure). Tie-breaker: operational reach — full-graph means one configured `TSIFT_CONVEX_GRAPH_URL` reconciles the whole project; scoped would require multiple deployments or a discriminator-by-scope schema, neither shipped today.

`--scope <name>` stays valid as the supported escape hatch for: (a) reconciling one submodule independently mid-flight, (b) very-large multi-deployment projects, (c) partial-failure replay where only one scope needs re-sync.

#### Default `--chunk-size` lowered 100 → 50
Operational consequence of choosing full-graph default: applies cover more rows per run, so the per-chunk budget needs to be tighter by default. Default 50 keeps `upsertEdges` under the Convex isolate's 99 MiB carry-over budget on the demo schema; operators with optimized upsert mutations can raise it back. CLI help string explains the rationale.

#### Surface
- `src/main.rs:401` — `chunk_size` clap default `"100"` → `"50"` with rationale in the doc comment.
- `SPEC.md` `tsift convex-sync` paragraph — documents full-graph default, chunk-size rationale, three escape-hatch scenarios.
- `VERSIONS.md` `## 0.1.57` entry.
- `plans/convexscopedeval-evidence.md` — decision matrix, code changes, follow-ups.

#### Sibling work in same commit
The 0.1.57 commit also lands `#p6tsifullfederated` — `tsift search --federated` now annotates each per-scope hit against that scope's own tagpath project instead of the workspace root. Test `search_federated_json_annotates_handles_from_per_scope_tagpath_indexes` covers an alpha+beta workspace.

#### Queue advances
Next: `complete [#gdbvacproof]`. Both prerequisites are now in:
- `#convexsnapshotscale` (v0.1.56) — paginated snapshot
- `#convexscopedeval` (v0.1.57) — full-graph default + safer chunk-size

So the Convex half of `#gdbvacproof` can run end-to-end: full-graph reconcile via paginated `--remote-snapshot`, `--apply` with the new safer default, then `tsift graph-db --path . compact --apply --prune-tombstones --confirmed-convex-reconciled --json` with metric capture.
❯ Can we complete [#gdbvacproof] or [#orchdog]?
### Re: complete [#gdbvacproof] + #orchdog — opus-4-7

#### #orchdog — closed (cite prior dogfood evidence)
Closed via `--done orchdog`. Two dogfood passes already happened on 2026-05-21:
1. Against `session-share/tasks/root.md` items #x63e + #v4v0 → findings recorded in `#gdbevid`, `#cmgr`, `#gdbidx`.
2. Against `tasks/software/tsift.md` (preset `#dogfood-tsift`) → surfaced two new gaps `#cmgrwres` + `#gdblock`.

Deliverable was "capture any tsift-side fixture gaps" — 5 gaps captured across the two passes. The dogfood loop has been run; the findings are queued in their own tickets.

#### #gdbvacproof — dispatched as focused subagent (long-running)
End-to-end Convex reconciliation + tombstone-prune workflow against the agent-loop superproject `.tsift/graph.db` (357k nodes / 605k edges / 781k retained tombstones). Both prerequisites just landed (`#convexsnapshotscale` v0.1.56 + `#convexscopedeval` v0.1.57), so the full workflow is now exercisable end-to-end:

1. Self-hosted Convex backend via Docker (no convex.dev auth).
2. `bunx convex dev --once` deploys the paginated schema.
3. `tsift convex-sync . --apply` at chunk-size 50 default (~10-15 min for 605k edges; falls back to chunk-size 25 if 50 still hits isolate carry-over).
4. `tsift convex-sync . --remote-snapshot` verifies `freshness.status: current` via paginated transport (~1200 pages).
5. Capture BEFORE: file size, freelist, page count, status/doctor/scan timings.
6. `tsift graph-db --path . compact --apply --prune-tombstones --confirmed-convex-reconciled` (now safe — Convex reconciled).
7. Capture AFTER + compute deltas.
8. Cleanup container/volume/`.env.local`.
9. Extend `plans/gdbvacproof-evidence.md` with the closure section.
10. Bump to v0.1.58 (docs-only entry).

Will land tsift commit + superproject pointer + finalize with `--done gdbvacproof` once the packet returns.
### Re: #gdbvacproof sync in progress — opus-4-7

Subagent task wrapper terminated, but the `tsift convex-sync` process it spawned is still running healthily in the background (PID 170199, 225 s in, 1955 of ~19200 chunks dispatched, all 200 OK at ~21 ms each, ~10 % complete, ETA ~30 more minutes).

Container `convex-tsift` is up; backend logs show steady successful POSTs. BEFORE metrics already captured to `/tmp/gdbvacproof-full/` (size-before, freelist/page_count, status/doctor/scan timings, tombstone count). After sync finishes, the workflow continues with the paginated `--remote-snapshot` verification (~1200 pages), then the `compact --apply --prune-tombstones --confirmed-convex-reconciled` step (the actual `#gdbvacproof` closure proof), then cleanup.

Re-invoke `/agent-doc tasks/software/tsift.md` in ~30 min and I'll pick up the trailing workflow (remote-snapshot verify → compact + prune → AFTER metrics → evidence doc + v0.1.58 bump → commit + push + close `--done gdbvacproof`).
<!-- agent:boundary:26780e33 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#cachelookupshift]~
- ~do [#gdbvacconvex]~
- ~do [#convexsnapshotscale]~
- ~do [#convexscopedeval]~
- ~complete [#gdbvacproof]~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbvacproof] [recommended] Run the real Convex reconciliation plus graph-db compact workflow and capture before/after file size, freelist, tombstone counts, status/doctor timings, and scan-cost deltas so compaction policy is backed by operational evidence.
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
