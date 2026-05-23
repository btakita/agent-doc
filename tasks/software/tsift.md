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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-210323.md`*

Compacted content:
- Archived 1 response topic(s): SQLite graph edge entities and benchmark
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve graph performance?
### Re: graph performance next steps — gpt-5

The top next step is `#gdbprepprof`.

The current bottleneck looks like shared preparation, not the graph read operations themselves. In the latest checked-in `edge-entity-bounded-2026-05-23-*` samples, the bounded graph operations are already in the micro/millisecond range (`real.sqlite.path_max_hops` around 88-93us, `real.sqlite.conflict_matrix` around 1.6-1.9ms, `real.sqlite.dispatch_trace` around 141us), while `real.refresh_phase.conflict_matrix_preparation` is still about 12s. So the priority is to split and cache `prepare_conflict_matrix_inputs` before spending more effort on engine replacement.

I added these backlog items in priority order:

1. `#gdbprepprof` — break down and cache conflict-matrix preparation inputs.
2. `#gdbprofilefull` — keep a full-projection performance profile beside the session-hinted 213-row run.
3. `#gdbhoptiers` — raise max-hop behavior through measured operation-specific tiers, not one global cap.
4. `#gdbfalkorreal` — keep FalkorDB gated behind backend-eval until a production adapter beats SQLite on full real workloads and operational constraints.

On the two direct questions: yes, max-hop caps can probably increase, but only with operation-specific defaults plus deep-chain/high-degree fixtures proving truncation and query-plan behavior. FalkorDB could help later, but I would not switch to it from the current evidence: the bounded run has FalkorDB faster on conflict-matrix, slower on `path_max_hops`, and the older full run had candidate backends far behind SQLite on real conflict-matrix cost.

Commands used for evidence: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `rg`, `sed`, and `jq` against the tsift spec/source/history fixture. No code changed and no test suite was run for this prompt-response cycle.
<!-- agent:boundary:48c7cb44 -->
<!-- /agent:exchange -->
###
<!--

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbsqlitepgq]
- do [#gdbedgeentity]
- Please run a benchmark on the performance improvements for this queue run
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbprepprof] [recommended] Profile and cache conflict-matrix preparation inputs: split prepare_conflict_matrix_inputs timing into session_review::compute, status/index gate, context-pack diff, staged diff, impact, exploration materialization, and graph-orchestration; reuse the packet by source/document/staged-diff watermark so backend-eval, conflict-matrix, and dispatch-trace do not spend about 12s before millisecond graph reads.
- [ ] [#gdbprofilefull] [recommended] Add a full-projection graph performance profile alongside session-hinted backend-eval: keep the bounded tasks/software/tsift.md target run, but also run and update an opt-in full agent-loop projection fixture so 213-row cached runs cannot hide million-row refresh/read regressions.
- [ ] [#gdbhoptiers] [recommended] Turn max-hop caps into operation-specific hop tiers: keep 1-hop direct/high-degree probes and the 64-hop deep-chain default, then add measured 128/256-hop fixtures with truncation warnings and query-plan checks before raising user-facing defaults.
- [ ] [#gdbfalkorreal] [recommended] Keep FalkorDB behind backend-eval and only promote it after a production adapter beats SQLite on full-projection conflict-matrix, evidence, dispatch-trace, path_max_hops, install portability, and lock behavior; current bounded numbers are promising for conflict-matrix but not enough for a backend switch.
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
