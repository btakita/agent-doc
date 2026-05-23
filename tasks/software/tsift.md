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
Completed: #gdbrefreshcache, #gdbtargetsnap, and #gdbpathprobe shipped in tsift c1c7668 and superproject f321eabef; GitHub Agent Doc CI is externally blocked before runner startup by the billing/spending-limit annotation.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-140200.md`*

Compacted content:
- Archived 2 response topic(s): graph DB refresh cache, scoped snapshots, and path probes; benchmark and IPC payload guard
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve sqlite graph performance?
### Re: SQLite graph performance next steps — gpt-5

Top backlog item: `#gdbsqlitehotpath`.

I added `#gdbsqlitehotpath` at the top of the backlog. The next performance work should stay on SQLite first, not FalkorDB/backend promotion yet. Current `backend-eval` says SQLite is still the correctness and portability baseline, and on the real agent-loop graph it is already faster for the read paths that matter most: `path_max_hops` was about 11.7 ms for SQLite while the current read-only candidate adapters were roughly 0.94-1.11 s, and `conflict_matrix` was about 522 ms for SQLite versus roughly 1.28-1.41 s for the candidates.

The remaining SQLite work is the write/refresh and traversal hot path: the same run spent about 25.7 s in SQLite refresh overall, with the largest SQLite-owned phases at `sqlite_delta_write` ~3.8 s, `sqlite_edge_staging` ~2.5 s, `sqlite_node_staging` ~0.96 s, and `sqlite_property_row_staging` ~0.57 s. Synthetic cases still show smaller but clear SQLite gaps for bounded path/conflict traversal, so the item asks for a three-sample baseline, then targeted refresh delta/staging and bounded traversal optimization backed by `metric-digest`.

I would keep `#gdbvacproof` as review/storage proof rather than the top perf item. `graph-db status` reports the projection current with ~946k live rows, ~413k retained tombstone rows, a ~1.09 GB file, no freelist bytes, and compaction `not_needed`; useful evidence, but not the main latency lever today. Preflight also warned that `#orchdog` is still a legacy gated item in `agent:backlog`, so `agent-doc migrate /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` remains housekeeping.

Commands/proof: ran `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `agent-doc orchestrate --from-exchange --plan`, `tsift status --json`, `tsift context-pack`, `tsift graph-db --json --path /home/brian/work/btakita/agent-loop status`, `tsift graph-db --json --path /home/brian/work/btakita/agent-loop evidence next-steps --depth 3 --limit 8`, and `tsift graph-db --json --path /home/brian/work/btakita/agent-loop backend-eval`. Changed paths: session document only. Verification: graph status/evidence/backend-eval all returned successfully; no full source test/build run because this cycle only updated the session response/backlog. Escalation: none.
<!-- agent:boundary:d3121683 -->
<!-- /agent:exchange -->
###
<!--

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshcache]
- do [#gdbtargetsnap]
- do [#gdbpathprobe]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbsqlitehotpath] [recommended] Improve SQLite graph hot paths before trying another graph backend: take a three-sample backend-eval baseline, profile real refresh write phases plus synthetic path/conflict cases, then optimize SQLite refresh delta/staging and bounded traversal until metric-digest shows no >15% regression and at least one measured win over the current fixture.
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
