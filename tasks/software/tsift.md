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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-144729.md`*

Compacted content:
- Archived 2 response topic(s): #kgnv Graphify-style traversal; #fidx fresh index gate
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Please clone and assess https://github.com/colbymchenry/codegraph. Are there any patterns & tech in this project that are useful for tsift?
### Re: codegraph assessment — gpt-5

I cloned `https://github.com/colbymchenry/codegraph` into `/tmp/codegraph-assess.JOSqXU/codegraph` and assessed commit `79b9601` against local tsift commit `3aebc7e`.

Short answer: yes, there are useful patterns, but they should be ported selectively. CodeGraph is an MCP-first TypeScript knowledge-graph product; tsift is already stronger on binary-owned agent workflows, freshness gates, bounded handles, and SQLite lock recovery. I would borrow product and graph-query patterns, not the runtime architecture wholesale.

Most useful:

- **Adaptive exploration packets.** CodeGraph's `codegraph_explore` scores entry-point symbols, groups related nodes by file, emits relationship maps, reads clustered line-numbered source windows, and scales output budgets by project size. This maps cleanly onto tsift `context-pack`, `traverse`, `source-read`, and `workflow search`; tsift already has stable handles and envelopes, so the missing piece is better relevance clustering and file-section packing.
- **Framework/route nodes.** CodeGraph models web routes as first-class `route` nodes linked to handlers through `references` edges across several frameworks. This is directly useful for tsift because real agent questions often start at endpoints, UI routes, commands, or workflow names instead of pure function names. I would start narrow with Rust, TypeScript, and Python route extractors.
- **Affected-test / impact analysis.** CodeGraph has `affected` and symbol impact surfaces for changed files. tsift already has search, graph, digests, and call edges; a `tsift affected`-style command could turn changed files into likely tests, affected symbols, and closeout commands for agent-doc/CI workflows.
- **Concise MCP/tool-selection instructions.** CodeGraph emits server instructions telling agents when to use search, context, callers, impact, node, files, and status. tsift's equivalent should bias agents toward existing safe primitives: `search`, `explain`, `source-read`, `context-pack`, `session-review`, and `workflow search`.
- **Evaluation harness shape.** CodeGraph's evaluation cases assert that realistic exploration prompts surface expected symbols, not just that parsers produce nodes. That pattern is worth adding to tsift's DCI/search-quality fixtures: query -> expected symbol/file/handle coverage.

Less useful for tsift:

- The Node/TypeScript runtime, `web-tree-sitter`/WASM packaging, and optional `better-sqlite3` fallback do not fit tsift as well as the current Rust CLI plus feature-gated tree-sitter grammars.
- CodeGraph's database concurrency story is weaker than tsift's current WAL, sidecar writer lock, and snapshot fallback model. Keep tsift's lock/freshness design.
- Always-on file watching is only partly transferable. For agent-doc cycles, tsift's explicit fresh-index gate is safer than assuming a watcher caught up.
- FTS5 over symbol metadata is worth noting, but I would not prioritize it until `sift` plus exact `rg -F` leaves a demonstrated retrieval gap.

I added three recommended backlog items for the parts that look actionable: adaptive exploration packets, framework/route graph nodes, and affected-test/impact analysis.
<!-- agent:boundary:417d8778 -->
<!-- /agent:exchange -->
###
<!--
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#7k0p] [recommended] Borrow CodeGraph-style adaptive exploration packets for tsift context/traverse surfaces: project-size-aware budgets, relationship maps, line-numbered clustered source windows, and no-reread guidance.
- [ ] [#15vh] [recommended] Add framework/route nodes to the tsift graph, starting with narrow Rust/TypeScript/Python route extractors that emit route nodes and handler reference edges.
- [ ] [#72v1] [recommended] Add a tsift affected-tests/impact query over changed files, imports, and call edges so closeout and CI loops can choose likely test targets without broad manual scanning.
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
