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

OpenCode npm package path code-complete and pushed. #gopencode-install and #gopencode gated on npm registry auth to publish opencode-tsift. 5 of 7 backlog items completed: integration tests, permission docs, transcript verify, smoke session, README expansion.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-234100.md`*

Compacted content:
- Archived 3 response topic(s): local-e2e-verify; tsift + tokensave integration; libsql backend implementation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Can algorithms and pipelines from tokensave be used in tsift? Would it be beneficial?
### Re: tokensave algorithms in tsift — glm-5.1

**Yes, selectively — but not wholesale.**

#### High-value integration paths

| tokensave capability | tsift benefit | Integration approach |
|---|---|---|
| **34-language tree-sitter extraction** | tsift covers ~10 languages; tokensave covers 34 | Delegate extraction to tokensave for uncovered languages via GraphStore adapter |
| **Code-health analytics** (health scores, god class, dead code, circular deps, coupling, DSM, Lakos levelization) | tsift has no equivalent; high-value for conflict-matrix and dispatch-trace | Wrap tokensave tools inside tsift digest pipeline with budget-aware envelopes |
| **Call-graph impact analysis** (`impact`, `affected`, `hotspots`) | tsift has BFS path/neighborhood but lacks topological impact scoring | Pipe into tsift `conflict-matrix` gate for smarter parallel dispatch |
| **Shared libSQL storage** | `LibsqlGraphStore` already uses same libSQL backend | Shared `.db` schema could reduce re-indexing between both tools |

#### What NOT to absorb

- **48 MCP tools + edit primitives** — months of work for marginal gain; tsift delegates edits to the harness
- **Multi-branch indexing** — tsift projection/versioning model is architecturally different
- **Cost tracking + TUI monitor** — no tsift equivalent needed; tokensave owns this
- **FTS5 semantic search** — tsift uses graph queries, not full-text search

#### Concrete next step

1. Implement a `tokensave` backend adapter in `substrate.rs` behind `backend-eval` feature flag
2. Read tokensave `.tokensave/tokensave.db` directly via `LibsqlGraphStore::open()`
3. Benchmark through `perf_gate.rs` against SQLite

This lets tsift query tokensave pre-built graph without re-indexing — highest-ROI integration with minimal new code.
❯ What are #next-steps to reverse-engineer tokensave to directly implement in tsift?
### Re: reverse-engineering tokensave next steps — glm-5.1

Based on deep analysis of tokensave's architecture, here are concrete next steps ordered by ROI.

#### Phase 1: Read tokensave's existing DB (low effort, high value)

**Step 1.1 — tokensave DB reader adapter**
- Implement `TokensaveGraphStore` in `src/tokensave_backend.rs` behind `backend-tokensave` feature
- Read `.tokensave/tokensave.db` via `LibsqlGraphStore::open()` — already works since we share libSQL
- Map tokensave's `nodes`/`edges` schema to tsift's `GraphNode`/`GraphEdge` types
- tokensave schema: `nodes(id TEXT PK, kind, name, qualified_name, file_path, start_line, ...)` with `edges(source, target, kind, line)`
- tsift schema: `graph_nodes(id, kind, label, file_path, ...)` with `graph_edges(from_id, to_id, kind, ...)`
- Write a thin schema-mapping layer in the adapter

**Step 1.2 — FTS5 search integration**
- tokensave has `nodes_fts` virtual table for semantic search over name/qualified_name/docstring/signature
- Add `search_fts()` method to tsift's `GraphStore` trait (or as extension trait)
- Wrap in tsift's envelope system for budget-aware truncation

#### Phase 2: Port graph algorithms (medium effort, high value)

**Step 2.1 — Iterative Tarjan SCC** (`~200 LOC`)
- Port from tokensave's `graph/scc.rs` — already iterative (no recursion), clean algorithm
- Used for: cycle detection, acyclicity health score, dependency depth (Lakos levelization)
- Add to tsift's `substrate.rs` as methods on `GraphStore` trait

**Step 2.2 — Composite health score** (`~400 LOC`)
- Port `graph/health.rs`: geometric mean of 5 dimensions (acyclicity, depth, equality, redundancy, modularity)
- Each dimension is independent — port incrementally
- Integrate with tsift's `conflict-matrix` gate: use health scores to gate dispatch risk

**Step 2.3 — Dead code detection** (`~150 LOC`)
- Port from `graph/queries.rs`: zero incoming edges of "real" kinds, exclude test/pub markers
- Uses temp tables for efficient test-marker pre-join
- Add as `detect_dead_code()` on `GraphStore`

**Step 2.4 — Coupling / fan-in / fan-out** (`~100 LOC`)
- File-level dependency adjacency from `calls`/`uses` edges
- Add as `compute_coupling()` returning structured results

#### Phase 3: Port extraction pipeline (high effort, medium value)

**Step 3.1 — LanguageExtractor trait** (`~300 LOC framework`)
- Port `extraction/mod.rs` trait + `LanguageRegistry`
- tsift already has tree-sitter extraction in `graph.rs` for ~10 languages
- Unify tsift's existing extractors under the new trait

**Step 3.2 — Complexity counting** (`~200 LOC`)
- Port `extraction/complexity.rs`: generic cyclomatic complexity framework
- Maps tree-sitter node types to branches/loops/returns/nesting/unsafe/assertions
- Per-language `ComplexityConfig` structs

**Step 3.3 — Multi-strategy reference resolution** (`~500 LOC`)
- Port `resolution/resolver.rs`: qualified name match → exact match → scored disambiguation
- Scoring heuristics: same file (+100), directory proximity (+5/shared), same language (+50), pub (+10)
- Blocklist for common names (Result, Option, new, from, etc.)
- Uses rayon for parallel resolution

#### Phase 4: Advanced features (optional, low priority)

| Feature | LOC est. | Value for tsift |
|---|---|---|
| DSM (Design Structure Matrix) | ~300 | Low — nice visualization, not core |
| Gini coefficient | ~100 | Low — part of health score |
| Redundancy detection (AST/CFG/shingles) | ~600 | Medium — duplicate code detection |
| Edit primitives (str_replace, etc.) | ~400 | Skip — tsift delegates to harness |
| ONNX embeddings / vector search | ~500 | Skip — out of scope |
| Multi-branch indexing | ~400 | Skip — tsift has projection versioning |

#### Recommended order

1. **Step 1.1** — tokensave DB reader (1-2 days)
2. **Step 2.1** — Tarjan SCC (1 day)
3. **Step 2.2** — Health score (2-3 days)
4. **Step 2.3** — Dead code detection (1 day)
5. **Step 2.4** — Coupling analysis (1 day)
6. **Phase 3** — Extraction pipeline (5-7 days, if needed for language expansion)

**Total Phase 1-2: ~1-2 weeks for core graph algorithms. Phase 3 adds another week.**
❯ Should we architect separate rust crates in tsift?
### Re: separate crates architecture — glm-5.1

**Yes, but not yet.** The current monolith is manageable but approaching the point where decomposition pays off.

#### Current state

- **71K LOC** in a single crate, 28 source files
- `main.rs` alone is **39K LOC** (command routing — the bulk)
- `substrate.rs` is **4.8K LOC** (GraphStore trait + 3 backends)
- `graph.rs` is **1.4K LOC** (tree-sitter extraction)

#### When to split

Split when you hit one of these triggers:
1. **You're adding tokensave ports** (Phase 2-3 from previous response) — Tarjan SCC, health scores, extraction pipeline are distinct domains
2. **Compile times exceed 60s** — currently single-crate means full recompilation on any change
3. **Feature flags get complex** — `backend-libsql`, `backend-eval`, future `backend-tokensave` are backend concerns

#### Proposed crate structure (when you split)

```
tsift/
  crates/
    tsift-core/          # GraphStore trait, GraphNode, GraphEdge, GraphProjection types
                         # ~1.5K LOC (substrate.rs types + trait only)

    tsift-sqlite/        # SqliteGraphStore (default backend)
                         # ~3K LOC from substrate.rs

    tsift-libsql/        # LibsqlGraphStore (feature-gated)
                         # ~700 LOC (libsql_backend.rs)

    tsift-graph/         # tree-sitter extraction, LanguageExtractor, complexity
                         # ~2K LOC (graph.rs + lang/)

    tsift-algorithms/    # Tarjan SCC, health scores, dead code, coupling, BFS/DFS
                         # NEW — ports from tokensave Phase 2

    tsift-extraction/    # Reference resolution, scoring heuristics
                         # NEW — port from tokensave Phase 3

    tsift-cli/           # main.rs command routing, clap definitions
                         # ~39K LOC — depends on all other crates

  Cargo.toml             # workspace root
```

#### Why NOT split now

- The monolith is still coherent — all 28 files relate to one tool
- Splitting adds workspace overhead, version coordination, dependency management
- The tokensave ports haven't started yet — crate boundaries should reflect the final architecture, not the current one
- **Recommendation**: start the tokensave Phase 1 (DB reader adapter), then split as part of Phase 2 when you have concrete code to organize
❯ Would splitting separate crates improve agentic maintenance?
### Re: crates and agentic maintenance — glm-5.1

**Yes — specifically for context window management and parallel agent work.**

#### How crates help agents

**1. Smaller context windows per task**
- An agent working on `tsift-algorithms` (Tarjan SCC, health scores) reads ~2K LOC, not the full 71K
- Fewer tokens consumed = more room for reasoning and tests
- `tsift-core` (~1.5K LOC) becomes the stable API surface agents can reference without loading backends

**2. Parallel agent dispatch**
- With workspace crates, two agents can safely work on `tsift-libsql` and `tsift-algorithms` simultaneously without merge conflicts
- Each crate has its own `Cargo.toml`, test scope, and build boundary
- tsift's own `conflict-matrix` gate would work better with crate-level isolation

**3. Targeted test runs**
- `cargo test -p tsift-algorithms` instead of rebuilding the entire monolith
- Faster feedback loops for agents — test cycle goes from ~30s to ~5s per crate

**4. Dependency isolation**
- `tsift-libsql` depends on `libsql` + `tokio` — only compiled when needed
- `tsift-graph` depends on `tree-sitter` grammars — heavy, but isolated
- `tsift-core` has zero heavy deps — fast to compile, stable API

#### How crates hurt agents

- **Cross-crate refactors** require touching multiple `Cargo.toml` files — more tool calls
- **Version coordination** — workspace helps, but pub API changes ripple
- **Build complexity** — agent needs to understand workspace structure
- **Smaller pain at current scale** — 71K LOC is large but still one cognitive unit

#### Net assessment

| Factor | Monolith | Workspace crates |
|---|---|---|
| Agent context per task | 71K LOC (wasteful) | 1-5K LOC (focused) |
| Parallel agent safety | Low (shared files) | High (crate isolation) |
| Test cycle time | ~30s full | ~5s per crate |
| Refactor overhead | Low | Medium |
| Onboarding complexity | Low | Medium |

**The split pays off when you start the tokensave Phase 2 ports.** The new algorithms crate gives agents a clean 2K LOC target instead of editing the 4.8K LOC `substrate.rs`. Recommend splitting as the first step of Phase 2, not before.
Add #next-steps for extracting crates interleaved with #next-steps of tokensave reverse engineer port.
### Re: interleaved crate extraction + tokensave port — glm-5.1

Combined roadmap where each crate extraction enables the next tokensave port.

#### Step 1: Extract `tsift-core` crate
- **Crate work**: Pull `GraphStore` trait, `GraphNode`, `GraphEdge`, `GraphProjection`, `GraphTombstone` types out of `substrate.rs` into `crates/tsift-core/src/lib.rs`
- **~1.5K LOC** — pure types + trait, zero heavy deps
- **tsift port progress**: None yet — this is foundation
- **Verification**: `cargo test -p tsift-core` compiles and passes

#### Step 2: Extract `tsift-sqlite` + `tsift-libsql` crates
- **Crate work**: Move `SqliteGraphStore` into `crates/tsift-sqlite/`, move `libsql_backend.rs` into `crates/tsift-libsql/`
- Both depend on `tsift-core` for the trait
- **tsift port progress**: None — but now backends are pluggable crates
- **Verification**: `cargo test -p tsift-sqlite && cargo test -p tsift-libsql --features backend-libsql`

#### Step 3: tokensave DB reader adapter (Phase 1)
- **Crate work**: Add `crates/tsift-tokensave/` — new crate, depends on `tsift-core` + `tsift-libsql`
- **tsift port**: Implement `TokensaveGraphStore` that reads `.tokensave/tokensave.db`
  - Schema mapping: tokensave `nodes(id, kind, name, qualified_name, file_path, ...)` → tsift `GraphNode`
  - Schema mapping: tokensave `edges(source, target, kind, line)` → tsift `GraphEdge`
  - FTS5 search: expose `nodes_fts` virtual table through tsift's envelope system
- **~800 LOC** — thin adapter over libsql
- **Verification**: Read a real tokensave DB, query nodes/edges, run FTS5 search

#### Step 4: Extract `tsift-graph` crate + port extraction pipeline
- **Crate work**: Move `graph.rs` + `lang/` into `crates/tsift-graph/`
- **tsift port**: Port from tokensave:
  - `LanguageExtractor` trait (~300 LOC) — unify tsift's existing extractors
  - `LanguageRegistry` (~100 LOC) — extension-based dispatch
  - `ComplexityConfig` + generic `count_complexity()` (~200 LOC) — cyclomatic framework
- **Verification**: Existing tree-sitter tests pass under new trait

#### Step 5: Add `tsift-algorithms` crate + port graph algorithms
- **Crate work**: New `crates/tsift-algorithms/`, depends on `tsift-core`
- **tsift port**: Port from tokensave:
  - Iterative Tarjan SCC (~200 LOC) from `graph/scc.rs`
  - Composite health score (~400 LOC) from `graph/health.rs` — geometric mean of 5 dimensions
  - Dead code detection (~150 LOC) from `graph/queries.rs`
  - Coupling / fan-in / fan-out (~100 LOC)
- **Verification**: Run against tokensave DB via `tsift-tokensave` adapter

#### Step 6: Add `tsift-resolution` crate + port reference resolution
- **Crate work**: New `crates/tsift-resolution/`, depends on `tsift-core` + `tsift-graph`
- **tsift port**: Port from tokensave:
  - Multi-strategy resolver (~300 LOC) — qualified → exact → scored
  - Scoring heuristics (~100 LOC) — file proximity, language, visibility
  - Blocklist (~50 LOC) — common names (Result, Option, new, from)
- **Verification**: Resolve references in a real project, compare hit rate

#### Step 7: Collapse CLI into `tsift-cli` crate
- **Crate work**: Move `main.rs` (39K LOC) into `crates/tsift-cli/`, depends on all other crates
- **tsift port**: Wire new crates into CLI commands (`tsift health`, `tsift dead-code`, `tsift coupling`)
- **Verification**: Full `cargo test` across workspace, `make check` passes

#### Summary

| Step | Crate work | tokensave port | LOC estimate |
|---|---|---|---|
| 1 | Extract `tsift-core` | — | ~1.5K moved |
| 2 | Extract `tsift-sqlite` + `tsift-libsql` | — | ~3.7K moved |
| 3 | Add `tsift-tokensave` | DB reader adapter | ~800 new |
| 4 | Extract `tsift-graph` | LanguageExtractor + complexity | ~600 new + ~1.4K moved |
| 5 | Add `tsift-algorithms` | Tarjan SCC, health, dead code, coupling | ~850 new |
| 6 | Add `tsift-resolution` | Multi-strategy resolver | ~450 new |
| 7 | Extract `tsift-cli` | Wire new commands | ~39K moved |

**Steps 1-2**: 2-3 days (pure refactoring, no new code)
**Steps 3-6**: 1-2 weeks (new tokensave ports, interleaved with crate creation)
**Step 7**: 1 day (final CLI wiring)
**Total**: ~3 weeks
#next-steps were not added to the backlog. #agent-doc-bug. Add #next-steps to the backlog.
### Re: backlog update — glm-5.1

Acknowledged. Adding the 7-step interleaved roadmap to the backlog now. Bug reported to `tasks/agent-doc/agent-doc-bugs2.md`.
<!-- agent:boundary:0be07837 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#local-e2e-verify]~
- ~[#npm-auth-setup]~
- ~[#npm-publish-verify]~
- ~[#full-e2e-verify]~
- #next-steps were not added to the backlog. #agent-doc-bug. Add #next-steps to the backlog.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#step-1-core] Extract `tsift-core` crate — GraphStore trait, GraphNode, GraphEdge, GraphProjection types (~1.5K LOC from substrate.rs)
- [ ] [#step-2-backends] Extract `tsift-sqlite` + `tsift-libsql` crates from substrate.rs + libsql_backend.rs (~3.7K LOC)
- [ ] [#step-3-tokensave-reader] Add `tsift-tokensave` crate — DB reader adapter for .tokensave/tokensave.db with schema mapping + FTS5 search (~800 LOC)
- [ ] [#step-4-graph] Extract `tsift-graph` crate + port LanguageExtractor trait, LanguageRegistry, complexity counting from tokensave (~600 new + ~1.4K moved)
- [ ] [#step-5-algorithms] Add `tsift-algorithms` crate — port iterative Tarjan SCC, composite health score, dead code detection, coupling analysis from tokensave (~850 LOC)
- [ ] [#step-6-resolution] Add `tsift-resolution` crate — port multi-strategy reference resolver, scoring heuristics, blocklist from tokensave (~450 LOC)
- [ ] [#step-7-cli] Extract `tsift-cli` crate — move main.rs (39K LOC), wire new crates into CLI commands (tsift health, tsift dead-code, tsift coupling)
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
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
