---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
❯ re #bamc: can and should we build in the functionality into tsift?
### Re: #bamc — graphify evaluation — opus-4-6

**Decision: build the deterministic graph layer into tsift, keep graphify as optional semantic layer.**

tsift already owns AST extraction, call graph, symbol table, and incremental indexing. The natural extensions are community detection and graph queries — both deterministic, both feed from the existing SQLite graph.

#### Build into tsift (deterministic, AST-derived)

| Capability | Effort | Why |
|---|---|---|
| **Community detection (Louvain)** | ~200 lines Rust | Clusters call_edges + symbol refs into architectural subsystems. No new deps — Louvain is a simple iterative algorithm over adjacency. |
| **Graph queries** (`tsift path`, `tsift explain`) | ~150 lines | BFS/DFS traversal, shortest path between symbols. Extends existing `tsift graph` subcommand. |
| **Concept lint** (#cs4r) | Already planned | Cross-references markdown plain text against the symbol graph. |

#### Keep in graphify (LLM-derived, semantic)

| Capability | Why not tsift |
|---|---|
| LLM-based entity extraction | Needs API client, prompt orchestration, subagent coordination. graphify's core differentiator. |
| HTML visualization | Nice-to-have, not essential for CLI. `tsift graph --json` can feed any viz tool. |
| Semantic similarity (embeddings) | Requires vector index + embedding model. Overkill for structural code navigation. |
| Non-code inputs (papers, images, video) | Out of scope for a code tool. |

**Boundary rule:** tsift owns deterministic, AST-derived graph operations. graphify owns LLM-derived semantic operations. Interop: `tsift graph --json` exports to graphify-compatible format for merging.

Closing #bamc. Reclassified #jzm8 from "graphify integration" to tsift-native community detection. Added #cdet (Louvain) and #gpth (path/explain queries) to pending.
<!-- agent:boundary:703302d0 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#cdet] Louvain community detection over call graph — cluster related modules into architectural subsystems (~200 lines, no new deps)
- [ ] [#gpth] tsift path <A> <B> + tsift explain <symbol> — graph query subcommands extending existing tsift graph
- [ ] [#nkec] tsift audit: skill drift detection — scan installed skills, compare against manifest, flag stale/broken/missing
- [ ] [#865b] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#ay3j] tsift audit: usage tracking — analyze claude-mem/session history for skill invocation frequency, flag never-used skills
- [ ] [#t3pq] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#dt80] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
- [ ] [#cs4r] tsift lint: detect unannotated concepts in markdown — cross-reference plain text against known graph entities (symbols, headings, bold terms), suggest backtick/bold/link annotations
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#jzm8] tsift native: community detection (Louvain clustering) over call_edges + symbol references
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:pending -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
-->
