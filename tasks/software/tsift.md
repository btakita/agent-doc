---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260607-061752.md`*

Compacted content:
- Archived 2 response topic(s): lazily-rs IPC for tsift/agent-doc; durable fallback in OS shm
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:cadee9d1 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#surrealbulk] SurrealDB projection performance (consolidates surrealbulk + surrealbulk-close + surreal-batch-bfs + surreal-fixture): replace per-row upsert loops with bulk/transactional row load + one-pass secondary-index rebuild; add batch frontier expansion; record SurrealDB samples in graph-db-performance-history.json fixture; re-run backend-eval benchmarks, update SPEC.md/BENCHMARKS.md table, measure refresh vs load separately.
- [/] [#e2e-install] E2E verify opencode-tsift install in a fresh opencode project: opencode plugin opencode-tsift && opencode then /tsift-status returns healthy (consolidates e2e-install + full-e2e-verify). Live-verify only; npm publish + OIDC auth already done (opencode-tsift@0.1.63 on npm).
- [/] [#7qky] [recommended] AGENT-DOC-SCOPED (drive from an agent-doc session, not tsift): re-measure Codex redundant runbook reads after the v0.34.0 retain-dont-reread nudge accumulates live Codex sessions; compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). If Codex rate does not drop, build a session-scoped content-dedup LEDGER in a shared LEAF crate (register(id,hash)/should_deliver->Full|Stub; exemptions via hash-change+compaction; session-id from harness). GATED on accumulating post-nudge Codex sessions.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output once quality gates pass; compare ranked vs unranked on real backlog/code queries (useful-hit density, duplicate-name precision, handle coverage, latency, emitted tokens). Also integrate the shipped #memgraphrag1 decay-weighted memory ranking so memory + code share one ranking over the unified graph.
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
