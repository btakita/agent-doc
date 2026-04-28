---
description: "Code-search and code-navigation skill for tsift. TRIGGER: user invokes /tsift or explicitly asks to search/explore a codebase with tsift. VERSION CHECK: compare installed tsift binary against tsift-version before relying on this copied skill text."
user-invocable: true
argument-hint: "[query or symbol]"
tsift-version: "0.1.0"
---

# tsift

Search a codebase using Sift (hybrid BM25 + vector search) via tsift CLI. Also provides AST symbol indexing, call-graph queries, batch editing, model-tier routing, and SQL introspection.

## Version Check

- Run `tsift --version` and compare it to `tsift-version` in this file's frontmatter.
- If the installed binary is newer, treat this skill text as stale. Refresh from `src/tsift/CLAUDE.md` and `src/tsift/VERSIONS.md`, and prefer `tsift --help` / subcommand help for the live command surface.
- Keep `tsift-version` aligned with `src/tsift/Cargo.toml`.

## Freshness Check

- Run `tsift status` at session start before relying on `search`, `explain`, or `graph`.
- If `tsift status` reports `index: stale` or `use: (none — run tsift index first)`, run `tsift index .` before continuing.
- If `tsift search` times out with `The index may be stale`, treat that as a required reindex. This is common in resumed tmux sessions with an old `.tsift/index.db`.
- After `tsift index .`, retry the original tsift command. Reserve `--timeout 0` for cases where you intentionally want an uncapped long-running search on a fresh index.

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
