---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
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
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-185101.md`*

Compacted content:
- Archived 3 response topic(s): tsift graph navigation queue; Convex graph abstraction; Convex graph projection backend
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps for graph extensions on Convex and sqlite?
### Re: graph extension next steps — gpt-5

Top backlog item after this cycle: `#gctx`.

The next graph work should start by moving the agent-facing graph surfaces onto the substrate boundary. The substrate and Convex row adapter exist, but `traverse` and `context-pack` still rebuild graph nodes from `index.db` tables and markdown at query time. That keeps SQLite-only traversal working, but it means Convex cannot become an equivalent graph read model yet.

I added three prioritized backlog items:

1. `#gctx` — make `GraphStore` the shared query path for `traverse` and `context-pack`, with SQLite materializing file/symbol/route/session/backlog/source-handle graph records first and Convex proving parity through the same path.
2. `#cxsync` — add the concrete Convex projection sync surface around the existing adapter: schema/index contract, chunked idempotent upserts, tombstone/delete propagation, dry-run output, retries, and diagnostics.
3. `#gfresh` — add projection freshness/parity checks so SQLite remains the local correctness store and Convex-backed reads fail closed when the projection is stale or incomplete.

I would keep the existing `#98qd` immediately after those. It becomes the consumer-facing agent-doc orchestration/context-shaping step once the storage/query substrate is ready.
<!-- agent:boundary:1c12fe05 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gctx] [recommended] Route tsift traverse and context-pack exploration through the provider-neutral GraphStore, materializing files, symbols, routes, sessions, backlog items, and source handles into SQLite graph_nodes/graph_edges first and proving the same query path against the Convex adapter.
- [ ] [#cxsync] [recommended] Add a concrete Convex projection sync surface for nodes/edges: schema/index documentation or generated functions, chunked idempotent upserts, tombstone/delete propagation, dry-run JSON, retry/partial-failure diagnostics, and mock-client regression tests.
- [ ] [#gfresh] [recommended] Add SQLite-to-Convex projection freshness and parity checks: projection version/content hash metadata, stale/missing node and edge diagnostics, and fail-closed traverse/context-pack behavior when Convex trails the local SQLite correctness store.
- [ ] [#98qd] Support agent-doc context shaping/orchestration: add tsift-backed backlog coverage for job packets, source handles, and bounded worker context.
- [ ] [#atvp] Unblock crates.io publication for tsift by replacing the upstream git-only sift dependency, publishing a compatible upstream crate, or vendoring a small stable search adapter; rerun cargo publish --dry-run before enabling TSIFT_ENABLE_CRATES_PUBLISH.
- [ ] [#g6kf] Confirm the v0.1.42 GitHub release completes after the macOS x86_64 Actions runner leaves the queue; if it remains stuck, remove or replace that matrix target and retag a fixed release.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
