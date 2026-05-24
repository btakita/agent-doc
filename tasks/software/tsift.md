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
Completed: #gdbrefreshcache, #gdbtargetsnap, #gdbpathprobe, and #gdbpropscan shipped and pushed in tsift e8823da and agent-loop e8e6a8b0a; tsift CI run 26367244295 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-170246.md`*

Compacted content:
- Archived 2 response topic(s): gdbpropscan; post-closeout push
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve graph performance?
### Re: graph performance next steps — gpt-5

Top backlog item: `#gdbfullproj`.

I added four graph-performance follow-ups at the front of the backlog, in priority order:

1. `#gdbfullproj` — first get repeated `graph-db backend-eval --full-projection` samples into `metric-digest` and refresh the performance-history baseline. That gives us real evidence before changing defaults.
2. `#gdbpathcaps` — use the existing 64/128/256/512-hop backend-eval tiers to decide whether path caps should rise. The current implementation already has bounded SQLite frontier BFS and warning/probe coverage; raising caps should wait for repeated query-plan and regression proof.
3. `#gdbcmhot` — isolate the remaining full-projection hot/cold cache costs in conflict-matrix, evidence, and dispatch-trace preparation, then tighten shared graph snapshot reuse.
4. `#gdbbackendprod` — keep FalkorDB/kuzu behind the promotion gate until a native production adapter proves direct projection writes, freshness/parity, install cost, lock behavior, and full-projection performance against SQLite.

Short answer on the two implied options: increasing max-hop caps is plausible, but only after the measured path tiers pass the gate; FalkorDB could help later, but the current spec deliberately holds it until it beats SQLite on the full real/full-projection promotion surface, not just synthetic read probes.

Verification: ran `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, refreshed the `src/tsift` tsift index, and inspected the relevant graph-db/backend-eval/path surfaces. No code changed, so I did not run the test suite.
<!-- agent:boundary:bea74153 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbfullproj] [recommended] Run repeated full-projection graph-db backend-eval samples through metric-digest, update the performance-history baseline, and use the results as the promotion gate for graph performance work instead of relying on single-run or synthetic-only numbers.
- [ ] [#gdbpathcaps] [recommended] Decide whether to raise user-facing graph path max-hop defaults from measured data: compare 64/128/256/512-hop backend-eval tiers, inspect SQLite query-plan diagnostics, and add a regression gate before increasing caps.
- [ ] [#gdbcmhot] [recommended] Profile full-projection conflict-matrix, evidence, and dispatch-trace hot/cold cache paths, then reduce the remaining preparation hotspot with shared graph snapshot reuse and explicit cache-hit metric coverage.
- [ ] [#gdbbackendprod] [recommended] Prototype a native production graph backend path for FalkorDB or kuzu only after it writes provider-neutral projection rows directly, proves freshness/parity on real and full-projection datasets, and passes install, lock, evidence, conflict-matrix, dispatch-trace, and path gates against SQLite.
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
