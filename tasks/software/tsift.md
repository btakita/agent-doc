---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Rust CLI skeleton pushed.
<!-- /agent:status -->

## Spec: AST-Aware Code RAG

### Goal

Extend tsift with tree-sitter AST parsing, dependency graph tracking, and per-submodule isolation to enable token-efficient code retrieval at the function/symbol level rather than file level.

### Architecture

```
tsift (CLI + MCP plugin)
├── sift (BM25 + vector — existing)
├── tsift-graph (new crate)
│   ├── tree-sitter parsing (function/type/trait extraction)
│   ├── dependency graph (call/import relationships)
│   └── graph-ranked retrieval (PageRank-style symbol ranking)
└── rusqlite (storage — existing)
```

### Per-Submodule Isolation

Each git submodule gets its own index. Isolation tiers control federation (cross-submodule queries):

| Tier | Behavior | Examples |
|------|----------|----------|
| **Isolated** | Never federated, strict boundary | session-share, monsterrodholders-dev |
| **Private** | Never federated | mail, resume |
| **Shared** | Federated by default | agent-doc, corky, ctx-core-dev |

Config: `.tsift/config.toml` in workspace root.

```toml
[defaults]
federation = true

[overrides.session-share]
federation = false
isolated = true

[overrides.mail]
federation = false
```

### Storage Layout

```
.tsift/
  indexes/
    agent-doc/
      symbols.db      # SQLite: function signatures, types, locations
      embeddings.lance # LanceDB: vector embeddings of signatures
      deps.json        # call graph + import graph
      meta.json        # last indexed commit, language stats
    corky/
      ...
  config.toml
```

### New Subcommands

```bash
tsift index --ast <path>        # tree-sitter AST extraction → symbols.db
tsift graph <path>              # build dependency graph → deps.json
tsift graph --callers <symbol>  # who calls this function?
tsift graph --callees <symbol>  # what does this function call?
tsift search <query>            # gains AST-aware ranking when index exists
tsift search --scope <submod>   # restrict to one submodule's index
```

### Key Design Decision: Graph > Vector for Code

Aider's repo-map research showed graph-ranked retrieval (PageRank over call/import references) outperforms pure vector similarity for code. The approach: extract symbols via tree-sitter, rank by reference count (centrality), embed only top-ranked. This gives best token efficiency.

### Output Contract

Retrieval returns function-level results, not file-level:
- Function signature + file:line location
- 1-hop dependencies (callers/callees)
- 50-200 tokens per result vs. 2000+ for full-file reads

### Development Phases

1. Add `tree-sitter` + `tree-sitter-rust` deps
2. Implement `tsift index --ast` for single directory (extract function signatures + locations)
3. Store in SQLite `symbols` table
4. Wire into `tsift search` — AST index ranking first, BM25 fallback
5. Add `tsift-graph` crate — call graph extraction + traversal
6. Per-submodule config + isolation tiers

### What NOT to build

- Visualization (Mermaid, HTML) — leave to graphify
- Full LSP-level type inference — diminishing returns
- Embedding model hosting — use external API or lightweight local model (all-MiniLM-L6-v2)

## Exchange

<!-- agent:exchange patch=append -->

<!-- agent:boundary:f1f24d99 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#ast1] Add tree-sitter + tree-sitter-rust deps to Cargo.toml
- [ ] [#ast2] Implement `tsift index --ast` — extract function signatures/types/traits from a directory
- [ ] [#ast3] SQLite `symbols` table schema — name, kind, signature, file, line, parent_module
- [ ] [#ast4] Wire AST index into `tsift search` — rank by symbol match first, BM25 fallback
- [ ] [#grp1] Create `tsift-graph` crate — call graph extraction via tree-sitter
- [ ] [#grp2] `tsift graph --callers/--callees` subcommands
- [ ] [#iso1] Per-submodule config (.tsift/config.toml) + isolation tiers
- [ ] [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#nkec] tsift audit: skill drift detection — scan installed skills, compare against manifest, flag stale/broken/missing
- [ ] [#865b] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#ay3j] tsift audit: usage tracking — analyze claude-mem/session history for skill invocation frequency, flag never-used skills
- [ ] [#t3pq] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#dt80] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
<!-- /agent:pending -->
