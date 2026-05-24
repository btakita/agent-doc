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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-185732.md`*

Compacted content:
- Archived 2 response topic(s): graph performance next steps; graph performance backlog
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to imrove graph performance?
### Re: graph performance next steps — gpt-5

Top backlog item: `#gdbcacheprove`.

I added three graph-performance follow-ups at the front of the backlog, in priority order:

1. `#gdbcacheprove` — prove the post-fix full-projection cache-hit behavior on a real `agent-loop` run. The refreshed history from the last cycle captured three useful full-projection samples, but those samples also exposed the pre-fix miss pattern: `full_projection.cache_lookup` missed each time because runtime artifacts could perturb the source watermark. This should be validated against the fixed binary before treating cache-hit numbers as the new baseline.
2. `#gdbcachecompact` — reduce cache artifact footprint. The full-projection cache is useful, but the observed cache artifacts were large enough that storage size and cleanup policy are now part of the performance story.
3. `#gdbrowreuse` — once cache hits are stable, profile row construction/loading and remove avoidable clone/serde work across SQLite and prototype backend reports. Current sample 3 still shows large source/projection costs (`full_projection.source_graph_build` about 18.6s, `full_projection.projection_rows` about 3.2s), so the next hotspot should be measured after cache churn is eliminated.

I did not add another FalkorDB or max-hop item here because those gates were just closed into explicit hold/measurement criteria. The next useful work is to make the full-projection evidence stable and cheap enough that backend and path-cap decisions are based on clean repeated samples.

Verification: ran `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, refreshed the workspace tsift index with `tsift status --fix`, inspected the current full-projection metric history, and searched the backend-eval/cache/conflict-matrix spec surfaces. No code changed in this cycle, so I did not run the test suite.

<!-- agent:boundary:b69cb833 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbcacheprove] [recommended] Run a post-fix real graph-db backend-eval --full-projection cold/cache pair on agent-loop, prove .agent-doc runtime markdown no longer changes the source watermark, prune stale pre-fix .tsift/backend-eval-cache miss artifacts, and refresh fixtures/graph-db-performance-history.json with a cache-hit sample.
- [ ] [#gdbcachecompact] [recommended] Reduce full-projection backend-eval cache footprint: replace the large JSON projection cache artifacts with a compact row store or cleanup policy, then gate on cache-hit load time, disk bytes, and metric-digest history size.
- [ ] [#gdbrowreuse] [recommended] Profile full-projection provider-neutral row construction and candidate row-store loading after cache hits are stable, then remove avoidable clone/serde work across SQLite and prototype backend reports.
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
