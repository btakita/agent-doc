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
Completed: #gdbrefreshdelta was already closed; #gdbcmcache, #gdbpathpolicy, and #gdbengineproof are implemented in tsift ab9f8a9 and agent-loop e09157f61. Local verification passed (`make check`, `cargo build`, `cargo install --path .`); latest tsift CI run 26372477579 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-221237.md`*

Compacted content:
- Archived 8 response topic(s): graph performance next steps; #gdbrefreshdelta; recovery closeout; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to make the graph faster?
### Re: graph performance next steps — gpt-5

I added four follow-up items to the backlog in priority order. Top backlog item: `#gdbprephot`.

The next graph-speed work should start from the remaining measured hotspots, not from a blind max-hop increase or a backend swap. The last closed cycle made the default queue path much faster, but it still left conflict-matrix preparation as the obvious hot area, and it explicitly called out that cold/full-projection proof still needs repeated samples. The gated review items `#gdbcacheprove` and `#gdbvacproof` are still evidence gates; the new backlog items are the implementation-oriented follow-ups after or alongside that proof.

Priority order:

1. `#gdbprephot` - reduce the remaining conflict-matrix preparation cost first.
2. `#gdbfullreuse` - make full-projection cold/cache behavior the hard performance gate.
3. `#gdbscanproof` - prove whether SQLite scan/index plans are still the limiting factor before adding engine work.
4. `#gdbperfgate` - make the measurement loop repeatable enough to block regressions and backend promotion mistakes.
<!-- agent:boundary:6d2a7c7e -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbprephot] [recommended] Profile and reduce the remaining conflict-matrix preparation hotspots after the cache fixes: measure session_review_compute, context_pack_diff, impact, and dispatch-trace reuse on repeated real/full-projection runs, then add shared packet reuse or narrower digest paths with metric-digest gates so the cached queue path drops below the current multi-second prep cost without trusting stale ownership.
- [ ] [#gdbfullreuse] [recommended] Make full-projection cache-hit performance the next hard gate: run at least three tsift graph-db --json backend-eval --full-projection cold/cache samples, split source-graph-build, projection-row construction, cache load, compression, and SQLite write phases, then remove clone/serde/cache artifact churn only where those samples show a stable bottleneck.
- [ ] [#gdbscanproof] [recommended] Re-check SQLite scan plans on the current graph schema before adding another backend: capture EXPLAIN/cardinality evidence for edge_property_scan, incident_edges, kind/neighborhood property filters, and path frontier probes on real plus synthetic high-degree/deep-chain data; add indexes/materialized rows only for proven scan-dominant cases.
- [ ] [#gdbperfgate] [recommended] Turn graph performance tuning into a repeatable release gate: store metric-digest summaries for default, full-projection, high-degree, and deep-chain runs, require three-sample regression evidence, and keep FalkorDB/Kuzu promotion blocked until they beat the stabilized SQLite gate including projection writes and lock behavior.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbcacheprove] [recommended] Run a post-fix real graph-db backend-eval --full-projection cold/cache pair on agent-loop, prove .agent-doc runtime markdown no longer changes the source watermark, prune stale pre-fix .tsift/backend-eval-cache miss artifacts, and refresh fixtures/graph-db-performance-history.json with a cache-hit sample.
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
