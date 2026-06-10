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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260610-185850.md`*

Compacted content:
- Archived 1 response topic(s): next-steps closeout repair
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:7c910a53 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- [#hccr2]
- [#hsmart2]
- [#hcache2]
- [#hlearn2]
- [#hmcp2]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#cmmcp-eval] [recommended] Evaluate DeusData/codebase-memory-mcp (https://github.com/DeusData/codebase-memory-mcp) as a tsift idea-source — NOT a replacement. Pure-C zero-dep single binary, 159 tree-sitter grammars, SQLite knowledge graph, Cypher-like read-only query language + BM25 + regex + bundled-embedding vector search, Hybrid-LSP type resolution (Py/TS/Go/PHP/C#/C/C++), team-shareable compressed graph artifact (.codebase-memory/graph.db.zst), 3D graph viz, PreToolUse grep/glob augmentation hook, 14 MCP tools. Port-worthy ideas to assess: (1) Cypher-like graph query language as a first-class agent interface vs our graph --callers/--callees; (2) committed/shareable compressed graph artifact so teammates+CI skip re-indexing; (3) pure-C zero-toolchain single-binary install; (4) grep/glob result augmentation. MIT, 3.1k stars, active. tsift keeps the edge on agent-doc integration (rewrite hooks, digest-runner, session-review, diff-digest, envelope budgets/scale_guard).
- [ ] [#hccr2] [recommended] Implement reversible retrieval handles for truncated tsift search/source/context-pack/digest output, storing omitted originals in .tsift/cache with query-scoped retrieve commands.
- [ ] [#hsmart2] [recommended] Add adaptive sampling for large JSON/list outputs in tsift envelopes and digests, preserving errors, first/last rows, anomalies, and query-relevant rows before budget truncation.
- [ ] [#hcache2] [recommended] Add stable-prefix and prompt-cache layout checks to session-review/context-pack output, separating stable instructions/evidence from volatile dates, paths, watermarks, and run ids.
- [ ] [#hlearn2] [recommended] Add failure-learning mining over session-review/log-digest/test-digest history, producing marker-owned AGENTS or tsift-memory corrections for repeated command/path/search failures.
- [ ] [#hmcp2] [recommended] Add first-party tsift MCP tools for compress/retrieve/stats-style workflows, reusing existing envelope, digest, and token-savings metrics rather than requiring a proxy.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
