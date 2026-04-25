---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  "#spec-test-commit-push": update spec + tests. commit + push
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Running preflight on the tsift session doc and exploring the current codebase in parallel.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Good — preflight confirms only a boundary artifact changed. Now let me read the key files before implementing `#gpth`.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
`graph.rs` already has `shortest_path()` and `PathResult`. Now let me check the IndexDb API and SPEC.md.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Now I have everything I need. Let me implement `#gpth` — `tsift path` + `tsift explain`.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Now I'll implement `#gpth`. The plan:
- `tsift path <from> <to>` — BFS shortest path using `graph::shortest_path()` already in graph.rs
- `tsift explain <symbol>` — structured callers+callees overview for LLM consumption
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
The subcommands are already there. Let me check if the command functions and dispatch exist.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Dispatch is wired but `cmd_path` and `cmd_explain` don't exist yet. I'll add them after `cmd_communities`.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Now I'll add the two command functions. Let me insert them right before the `#[cfg(test)]` block.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Good — the functions exist. Let me verify their content.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Both functions are already implemented with community membership shown in `explain`. Now let me check existing tests and build.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Tests exist. Let me run the build.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
153 tests pass. Now let me check git status in the tsift submodule and update SPEC.md.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
SPEC.md is already updated. Let me look at the new tests for path and explain in main.rs.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
All tests pass (153). Now build and commit.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
Now persist the response.
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
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

synchronous orchestra
preset #spec-test-commit-push
- do #cdet
- do #gpth

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #cdet
❯ Running preflight and exploring the tsift codebase in parallel.
#cdet is done. `tsift communities` ships Louvain community detection over the call graph — 357 lines across 4 files, 8 new tests (139 total), committed to `btakita/tsift@400c199` and pushed.

❯ (preset #spec-test-commit-push)
update spec + tests. commit + push
do #gpth
The response is committed. Now fix the pending using `--pending-done`.
<!-- agent:boundary:c4bffeea -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] [#cdet] Louvain community detection over call graph — cluster related modules into architectural subsystems (~200 lines, no new deps)
- [x] [#gpth] tsift path <A> <B> + tsift explain <symbol> — graph query subcommands extending existing tsift graph
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
