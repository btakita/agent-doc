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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260609-042037.md`*

Compacted content:
- Archived 1 response topic(s): next-steps - gpt-5
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:e8a90c44 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#kx6s]
- do [#qszc]
- do [#qs5v]
- do [#zkft]
- do [#58p8]
- do [#kn0d]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kx6s] Add a MemGraphRAG performance baseline fixture and gate covering memory query, memory project-graph, graph-db related, and semantic seeded neighborhood latency.
- [ ] [#qszc] Make memory candidate retrieval indexed with observed-at/create-time indexes plus FTS5 or a materialized token table, and rank a bounded candidate set instead of all memory events.
- [ ] [#qs5v] Materialize semantic vectors in a typed table/blob and add a GraphStore top-candidate query so semantic related does not scan and parse every semantic node property.
- [ ] [#zkft] Make memory graph projection incremental with memory watermarks/content hashes and explicit recent-first, oldest-first, and query-relevant read policies.
- [ ] [#58p8] Fold observed-at decay and memory node signals into shared GraphStore ranked_neighborhood scoring so memory and code prune through one graph-ranking path.
- [ ] [#kn0d] Push semantic seeded neighborhood caps into GraphStore/SQLite with SQL-side edge ranking limits and batched node lookup.
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
