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
queue_active: true
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260525-055028.md`*

Compacted content:
- Archived 10 response topic(s): #gdbfullreuse + #gdbscanproof + #gdbperfgate landed; review item #gdbcacheprove closed; #gdbgatecold packet in flight; 7 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: do [#cachelookupshift] — opus-4-7

Dispatched as a focused packet. Investigates why `preparation_cache_lookup` (~250 ms) never hits across consecutive `tsift graph-db backend-eval` runs on agent-loop — runs a pair, captures the watermark hash inputs, identifies the drifting input (likely a path slipping past `traversal_relative_path_is_generated_artifact` or a directory mtime entering the hash per the `#gdbgatecold` history), applies the minimal stable fix or stops with a "no fix — by design" verdict, then verifies cache-hit on a post-fix consecutive pair. Bumps to v0.1.51 if a fix lands. Hard regression-safety: `search_timeout_reports_reindex_when_index_turns_stale_during_worker_run` must stay green.

Awaiting the packet's proof block before `make check` + `cargo install --path .` + commit + push + finalize with `--done cachelookupshift`.
<!-- agent:boundary:f1876bae -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#cachelookupshift]
- do [#gdbvacconvex]
- complete [#gdbvacproof]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#cachelookupshift] On agent-loop the conflict-matrix preparation_cache_lookup (~250 ms) never hits across repeated tsift graph-db backend-eval runs because traversal source watermark hashes shift slightly each invocation. Identify the per-invocation hash input that's drifting (likely a generated artifact path slipping past traversal_relative_path_is_generated_artifact, or a directory mtime entering the hash) and either stabilize the hash or narrow the cache key so the prep cache actually hits on identical source state.
- [ ] [#gdbvacconvex] Deploy examples/convex-graph via npx convex dev against Brian's Convex account so tsift convex-sync --remote-snapshot --apply and tsift graph-db compact --prune-tombstones --confirmed-convex-reconciled can be exercised end-to-end (blocks the Convex half of #gdbvacproof).
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
