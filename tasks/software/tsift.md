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
Completed: #gsqlwrite reduces repeated SQLite graph refresh property-row work by staging and mutating materialized node/edge properties only for new or row-hash changed owners. Full-projection backend-eval now emits and requires the requested SQLite write phase metrics. Local verification passed (`cargo test -q sqlite_projection_refresh_handles_bulk_row_replacement`, `cargo test -q graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`, `cargo build`, `cargo install --path .`, `make check`); tsift CI run 26435208332 is green. The tsift submodule commit `7534c13` is pushed, and the root pointer commit `5a88f58ee` is in the current root history.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260526-153847.md`*

Compacted content:
- Archived 5 response topic(s): #next-steps for SQLite graph + tagpath community search; #gprojrows; closeout repair; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:798b9214 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gprojrows]~
- ~do [#gsqlwrite]~
- do [#gcommcache]
- do [#gcommtagctx]
- do [#gcommgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gcommcache] [recommended] Cache and bound tagpath-annotated community detection: key Louvain results by index graph watermark, scope, and tagpath freshness, avoid re-running db.all_edges plus full in-memory Louvain for unchanged graphs, and add JSON diagnostics for cache hit, edge count, iterations, and tagpath state.
- [ ] [#gcommtagctx] [recommended] Improve community search quality by carrying file/ref context into CommunityMember and resolving tagpath handles with community or edge evidence instead of name-only fallback; cover duplicate-name, skipped-directory, scoped, and federated fixtures with handle coverage and ambiguous-member diagnostics.
- [ ] [#gcommgate] [recommended] Add a community search quality and performance gate to metric-digest and fixtures: track communities runtime, handle coverage, stale/no-tagpath behavior, duplicate-name precision, and top-community stability across real and synthetic multi-module workloads.
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
