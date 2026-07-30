# tsift Internals (graph module, deps, dev, versioning, repo)

Extracted from the tsift spec/instructions. The primary agent entrypoint is [../SKILL.md](../SKILL.md).

## Graph Module (`src/graph.rs`)

Call-graph extraction via tree-sitter. Runs during `tsift index` and stores edges in `call_edges` table.

- `extract_call_sites(lang, source)` — parse source, find all function/method/macro calls
- `resolve_edges(symbols, call_sites)` — match call sites to enclosing functions (innermost wins)
- Supported: Rust (direct, method, scoped, macros), Python, TypeScript/TSX, JavaScript/JSX, Kotlin, GDScript (direct, `attribute_call` member calls, `base_call` super calls)
- Skipped: Zig, Bash, Markdown (no meaningful call patterns)
- `detect_communities(edges)` — Louvain phase 1 modularity optimization, returns communities sorted by size
- `shortest_path(edges, from, to)` — BFS over undirected call graph, returns path + hop count

**Query via CLI:**
```bash
tsift graph <symbol> --callers    # who calls this?
tsift graph <symbol> --callees    # what does this call?
tsift graph <symbol>              # both directions
tsift graph <symbol> --json       # structured output
tsift graph <symbol> --scope sub  # restrict to submodule
```

## Dependencies

- **sift** — upstream git dep (`github.com/rupurt/sift`). It is not currently published on crates.io under a compatible package name, so crates.io release automation must stay gated until that upstream publish story exists.
- **tagpath** — local path dep during development, with the published `0.6.0` version requirement retained so the dependency is already crates.io-compatible once the sift blocker is removed.
- **rusqlite** — `bundled` feature (no system SQLite needed).
- **tree-sitter** + per-language grammar crates — AST parsing for symbol extraction and call-graph.
- **clap**, **anyhow**, **serde**, **serde_json** — standard Rust CLI stack.

## Development

```bash
make check          # clippy + full test suite
cargo install --path .   # install to ~/.cargo/bin/
```

## Versioning

Canonical version source: [`Cargo.toml`](Cargo.toml) `package.version`. The installed binary exposes the same value via `tsift --version`.

Change history lives in [`VERSIONS.md`](VERSIONS.md). Even while tsift remains private, keep the Cargo package version and a matching `VERSIONS.md` entry in sync when the command surface or behavior changes.

If copied skill instructions lag behind the installed binary, treat this file, `VERSIONS.md`, and `tsift --help` as the current source of truth.
## Repo

Private: `github.com/btakita/tsift`. Submodule at `src/tsift` in agent-loop.
