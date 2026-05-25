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
<!-- agent:boundary:560204a3 -->
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
- complete [#gdbvacproof]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
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
