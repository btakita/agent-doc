---
agent_doc_session: tsift-v0.1
agent: opencode
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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260601-032539.md`*

Compacted content:
- Archived 6 response topic(s): #terse-health-early-exit; #libsql-edges-between-pushdown; #compact-community-serialization; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Would tsift benefit from lazily-rs for caching and create lazy-load graphs?
test content


### Re: lazily-rs for tsift caching and lazy-load graphs — glm-5.1

Yes, tsift would benefit from lazily-rs in specific areas, though the fit is narrower than the agent-doc integration (which already has a plan at `tasks/agent-doc/plans/lazily-rs-integration.md`).

#### Where lazily-rs helps tsift

**1. Graph neighborhood expansion** — `ranked_neighborhood()` and `neighborhood()` currently BFS-eagerly to depth N, loading all nodes/edges up front. A lazily-rs Slot per depth layer would defer each hop until the consumer requests it, cutting I/O for shallow queries on large graphs.

**2. Call-graph edge resolution** — `resolve_edges()` resolves all call sites eagerly per file. Wrapping per-symbol resolution in Slots keyed by `(file, symbol, content_hash)` with Cell-backed file-mtime invalidation would avoid re-resolving unchanged symbols across `tsift graph <symbol> --callees` calls.

**3. Thread-local inspection cache replacement** — `InspectScopeGuard` is a hand-rolled dedup cache for `IndexDb::inspect_read_only`. lazily-rs's built-in dependency tracking + auto-invalidation could replace it with less boilerplate.

**4. Summary cache enrichment** — Summary rows could be lazily loaded as Slots keyed by `(file, content_hash)`, computing on first access and auto-invalidating when the file changes (Cell tracks content hash). This replaces the "check if cached, else compute" pattern scattered across summary lookups.

#### Where lazily-rs is NOT a good fit

- **SQLite graph store** (`SqliteGraphStore`) — tsift already has mature SQLite-backed CRUD with WAL concurrency. lazily-rs is single-threaded (`RefCell`-based), so it cannot replace the storage layer.
- **Community detection** (Louvain) — requires full edge-set access; lazy evaluation doesn't help the algorithm itself (though Slot-wrapping the *result* for incremental invalidation on edge changes would).
- **Search token index** — inverted index construction is batch-oriented, not demand-driven.

#### Concrete integration path

```
Cell: content_hash(file)                    — invalidated on file change
 |--
 |-- Slot: symbol_edges(file, content_hash)  — resolve_edges, cached per-file
 |   |--
 |   +-- Slot: symbol_callees(symbol, symbol_edges) -- per-symbol callees
 |
 |-- Slot: neighborhood(depth, center, graph_store) -- lazy BFS layer
 |   |--
 |   +-- Slot: ranked_neighborhood(center, scoring) -- ranked expansion
 |
 +-- Slot: summary(file, content_hash)       -- LLM summary, computed once
```

This is lighter scope than the agent-doc integration (which wires config/frontmatter/SSH resolution). Worth a separate plan if you want to proceed.
What are #next-steps to integrate lazily-rs into tsift?
<!-- agent:boundary:62bc70c2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
- What are #next-steps to integrate lazily-rs into tsift?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
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
