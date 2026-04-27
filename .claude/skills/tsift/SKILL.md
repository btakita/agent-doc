# tsift

Search a codebase using Sift (hybrid BM25 + vector search) via tsift CLI. Also provides AST symbol indexing, call-graph queries, batch editing, model-tier routing, and SQL introspection.

## TRIGGER

- User asks to search code, find symbols, or explore a codebase with `/tsift`
- User asks about call graphs, callers/callees of a function
- User asks to index a project for symbol search

## SKIP

- General grep/ripgrep usage (handled by rtk rewrite hook)
- File-level search (use Glob/Grep tools directly)
- Non-code search (use WebSearch)

## Prerequisites

Binary: `tsift` (installed via `cargo install --path src/tsift`)

## Commands

### Index a project

```bash
tsift index [path]                    # Index current dir or path
tsift index [path] --workspace        # Per-submodule indexing
tsift index [path] --submodule <name> # Index one submodule
```

### Search symbols

```bash
tsift search "<query>"                # Hybrid BM25 + vector search
tsift search "<query>" --strategy lexical   # Fast text-only
tsift search "<query>" --federated    # Search all federated submodules
tsift search "<query>" --scope <name> # Search within one submodule
```

### Call-graph queries

```bash
tsift graph <symbol>                  # Show callers AND callees
tsift graph <symbol> --callers        # Who calls this?
tsift graph <symbol> --callees        # What does this call?
tsift graph <symbol> --json           # Structured output
tsift graph <symbol> --scope <name>   # Restrict to submodule
```

### Other commands

```bash
tsift edit --file edits.json          # Batch file edits (atomic)
tsift route "<task description>"      # Classify → haiku/sonnet/opus
tsift rewrite "<command>"             # Rewrite grep/rg → tsift search
tsift sql <db> [-q "SQL"]             # SQLite introspection
```

## Runbooks

- [runbooks/search-strategies.md](runbooks/search-strategies.md) — when to use lexical vs hybrid vs vector
