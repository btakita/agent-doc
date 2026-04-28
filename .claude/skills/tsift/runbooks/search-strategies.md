# Search Strategies

## When to use each strategy

| Strategy | Flag | Speed | Use when |
|----------|------|-------|----------|
| **lexical** | `--strategy lexical` | Instant | Exact symbol names, known identifiers, grep-like queries |
| **hybrid** | `--strategy hybrid` (default) | Fast | General code search — combines BM25 text matching with vector similarity |
| **vector** | `--strategy vector` | Slower | Conceptual/semantic queries ("error handling pattern", "authentication flow") |

## Index required

All strategies require `tsift index` to have been run first. The index lives at `.tsift/index.db`.

## Stale-session recovery

- Long-lived tmux sessions can outlive the current `.tsift/index.db`.
- If `tsift status` reports a stale index, run `tsift index .` before searching.
- If `tsift search` times out with `The index may be stale`, reindex with `tsift index .` and retry the same search.
- Use `--timeout 0` only when you want to allow a long search on a fresh index.

## Workspace search

For monorepos with submodules:

- `--federated` searches all submodules with `shared` tier (excludes `private`/`isolated`)
- `--scope <name>` searches a specific submodule regardless of tier
- Without flags, searches the flat `.tsift/index.db` (root project only)

## First-run note

`hybrid` and `vector` strategies download an embedding model on first use. Use `lexical` for instant results without model download.
