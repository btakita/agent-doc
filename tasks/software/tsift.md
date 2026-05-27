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

Answered why the session-share tsift DB appeared stale. No source changes; refreshed indexes with tsift status --fix and confirmed session-share is now fresh while workspace remains stale only because the FDE-Project-Interview scope is missing.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-021317.md`*

Compacted content:
- Archived 3 response topic(s): graph + neighborhood next steps; stale index auto-update; session-share stale index
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:f276ace8 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gneighgate] Add a neighborhood performance/quality gate, then optimize `paged_neighborhood`: run repeated real + full-projection + synthetic high-degree/deep-chain `graph-db neighborhood` samples, collapse the current node-page and edge-page recursive walks into one bounded reachable-set path if it is hot, and lock SQLite query-plan diagnostics plus Convex parity.
- [ ] [#gsemrows] Make semantic rows and tagpath handles part of graph effectiveness: when `graph-db refresh` warns the summary cache is empty or `communities` reports `tagpath_state=missing`, surface a fail-closed/actionable readiness state, then run `summarize --extract` + refresh and prove neighborhood/evidence ranking improves.
- [ ] [#gnbrank] Add ranked neighborhood output: score neighbors by depth, edge kind, community co-membership, semantic relation, source-handle freshness, duplicate-name precision, and handle coverage; gate with community-search quality metrics before changing defaults.
- [ ] [#gtombops] Run and document the graph tombstone cleanup workflow on the tsift graph DB: Convex reconcile, refresh, compact, then record status/doctor/file-size/tombstone-scan deltas from the current 184,722 tombstone / 27,375 live-row state.
- [ ] [#gbenchneigh] Refresh the graph performance history with dedicated neighborhood rows so future graph-backend or hop-cap decisions compare `neighborhood`, `path_max_hops`, evidence, conflict-matrix, and dispatch-trace together instead of optimizing one surface blind.
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
