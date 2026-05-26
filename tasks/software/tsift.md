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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260526-030403.md`*

Compacted content:
- Archived 1 response topic(s): #next-steps graph performance
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:ccc44188 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#gperf]
- do [#ghop]
- do [#gfront]
- do [#gback]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gperf] Run fresh graph-db performance triage after the cache and tombstone-prune work: collect three clean backend-eval samples for real, full-projection, high-degree, and deep-chain workloads; refresh the performance history; use metric-digest to rank remaining hotspots before changing caps or backends.
- [ ] [#ghop] Add a hop-cap promotion gate before raising user-facing graph max-hop defaults: keep the 64-hop default until repeated real, full-projection, and synthetic deep-chain samples prove 128/256/512-hop tiers stay within the performance gate and produce useful rows.
- [ ] [#gfront] Optimize SQLite high-hop/frontier traversal if refreshed metrics show path/evidence latency is still hot: use indexed frontier expansion or recursive CTEs, avoid per-hop broad scans, emit cap diagnostics, and lock query-plan regressions for full-projection plus synthetic deep-chain fixtures.
- [ ] [#gback] [recommended] Spike a real optional graph backend adapter only after SQLite gate evidence is fresh: compare Kuzu and FalkorDB behind GraphStore with projection writes/load, lock semantics, install portability, and full parity; do not promote a read-only prototype unless it beats SQLite across every required workload.
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
