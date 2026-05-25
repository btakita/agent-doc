---
agent_doc_session: 4602f467-ef2a-4094-a750-5eb533368988
agent: claude
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
queue_active: true
---


## Project Summary

**tagpath** — Tag Path naming system. Parse, lint, and search tag-based identifiers across languages.

- **Repo:** https://github.com/btakita/tagpath (submodule in agent-loop, symlinked from `~/work/btakita/tagpath`)
- **Crate:** [`tagpath` v0.5.0](https://crates.io/crates/tagpath)
- **Config format:** TOML (`.naming.toml`)
- **39 language presets**, **14 tree-sitter grammars** (Rust, Python, JS, TS, TSX, Go, C, C++, Java, Ruby, PHP, C#, Swift, Kotlin)
- **6 conventions:** snake_case, camelCase, PascalCase, kebab-case, UPPER_SNAKE_CASE, Ada_Case
- **Docs:** README.md, AGENTS.md (symlinked as CLAUDE.md), SPEC.md
- **CI:** GitHub Actions (clippy + test on push, 5-target release on tag)
- **Skill:** `/tagpath` Claude Code skill at `.claude/skills/tagpath/SKILL.md`

### Key Concepts

- Every identifier is a **path** through an ordered sequence of **tags**
- `personName` (camelCase) = `person_name` (snake_case) = `PersonName` (PascalCase) = `Person_Name` (Ada_Case) → canonical `[person, name]`
- `__` separates namespace dimensions: `auth0__user__validate` → 3 dimensions
- Role detection: `create_*` (factory), `use_*` (hook), `set_*` (setter), `is_*` (predicate)
- Shape detection: `*_a` (array), `*_r` (record), `*_m` (map), `*$` (signal)
- Per-context convention mapping in `.naming.toml`
- Composable configs via `extends` (language presets, convention presets, relative paths)

### Architecture

```
src/
  main.rs              CLI entrypoint (clap)
  parser/mod.rs        Convention detection, tokenization, role/shape detection
  config/mod.rs        .naming.toml schema, loading, extends resolution
  lint/mod.rs          Lint engine — validates identifiers against config rules
  extract/mod.rs       Identifier extraction (regex + tree-sitter)
  search/mod.rs        Cross-convention semantic search
  treesitter/mod.rs    Tree-sitter grammar loading and AST walking
  alias/mod.rs         Cross-convention alias generation
  prose/mod.rs         Human-readable prose descriptions
  graph/mod.rs         Tag co-occurrence graph (petgraph)
lang/                  Language presets (39 languages, TOML)
presets/               Convention presets (immutable-tag.toml)
```

### Agent Integration Vision

- **MCP tool** — expose `parse`, `search`, `lint` to coding agents
- **Intent-preserving refactoring** — rename a concept across all files/conventions
- **Cross-language symbol resolution** — shared semantic index for polyglot projects
- **Convention enforcement** — `.naming.toml` tells agents exact conventions per context
- **Tag ontology** — `.naming/tags/*.md` domain vocabulary per project

## Phases

### Phase 1 — Parse, detect, semantic equivalence, CLI ✅ (v0.1.0)

- [x] Config schema (`.naming.toml` with `extends`)
- [x] Name parser — convention detection, tokenization, tag decomposition
- [x] `tagpath parse` — CLI command with text/JSON output
- [x] `tagpath init` — generate config from language/convention preset
- [x] Language presets — 39 languages
- [x] Convention presets — `immutable-tag.toml`
- [x] 12 tests, YAML → TOML migration

### Phase 2 — tree-sitter, lint, extract, composability ✅ (v0.2.0–v0.4.0)

- [x] `tagpath extract` — regex-based + AST-aware identifier extraction
- [x] `tagpath search` — cross-language semantic tag search
- [x] tree-sitter integration — 14 grammars behind `lang-*` feature flags
- [x] `tagpath lint` — validates identifiers against `.naming.toml` context rules
- [x] Config composability — `extends` resolution (left-to-right, shallow merge)
- [x] Cross-language matching tests proven
- [x] GitHub Actions CI + release workflows
- [x] Ada_Case convention + Convention Display fix (v0.4.0)
- [x] 51 tests, clippy clean

### Phase 3 — Graph, alias, prose, skill ✅ (v0.5.0)

- [x] `tagpath alias` — convention translation (`snake_case ↔ camelCase ↔ PascalCase`)
- [x] `tagpath prose` — translate between tags and natural language
- [x] `tagpath graph` — build tag relationship graph from codebase (petgraph)
- [x] `/tagpath` Claude Code skill — installed in both repos
- [x] 72 tests, clippy clean, published to crates.io

### Phase 4 — Index, ontology, dynamic loading, WASM, MCP ✅ (v0.7.0–v0.10.0)

- [x] `tagpath index` — persistent index (`.naming/index.json`) for instant queries (v0.7.0)
- [x] Tag ontology — `.naming/tags/*.md` domain vocabulary (`tagpath ontology` already shipped)
- [x] Dynamic `.so` grammar loading — runtime extensibility via `libloading`, opt-in `dyn-grammar` feature (v0.10.0)
- [x] WASM build — `wasm` feature gating tree-sitter behind `cfg(not(target_arch = "wasm32"))`, wasm-bindgen surface (v0.9.0)
- [x] MCP tool integration — stdio JSON-RPC 2.0 server exposing parse / normalize_query / lint / search / ontology_lookup / indexed_project_query (v0.8.0)

### Design Decisions

- **Explicit imports only** — no implicit/magic config loading. `tagpath init --lang rust` generates full config; `extends = ["rust"]` tracks upstream.
- **Feature flags over monolith** — tree-sitter grammars are opt-in per language. `--no-default-features` gives regex-only.
- **Dynamic loading deferred** — feature flags cover immediate need. Runtime `.so` loading (Helix/Neovim style) for Phase 4.
- **WASM deferred** — core parser/search compile to WASM cleanly; tree-sitter stays native. Not a priority.

## Status

<!-- agent:status patch=replace -->
v0.10.0 published to crates.io — Phase 4 complete (index, MCP, WASM, dyn-grammar)
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-090022.md`*

Compacted content:
- Archived 2 response topic(s): do [#tpstdiag]; Add #next-steps to fix remaining lossy data
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#tpaudit] — opus-4-7

Shipped in tsift 0.1.59 (`1551a72` in tsift, superproject pointer bumped).

#### Surface
New top-level command: `tsift audit-tagpath [--path .] [--scope <name>] [--json]`.

Reconciles the tsift symbol index against the tagpath `.naming/index.json` source set and reports the diff so operators can see exactly where `tagpath_handle` recall is at risk. JSON shape:

```
{
  "project_root": "...",
  "scope": null | "<scope>",
  "tagpath_state": "fresh" | "stale" | "stale_unreadable" | "missing",
  "tsift_file_count": <usize>,
  "tagpath_file_count": <usize>,
  "tsift_only_files": [<rel_path>, ...],
  "tagpath_only_files": [<rel_path>, ...],
  "tsift_only_symbol_count": <usize>,
  "tsift_only_files_with_symbols": [{"file": ..., "symbols": ...}, ...]
}
```

When `tagpath_state == "stale"`, the binary still loads the on-disk snapshot so the audit produces useful output, marks the response, and injects the standard `tagpath_index_stale: true` + `tagpath_stale_reason: stale_snapshot_loaded` pair (the same field contract `#tpstdiag` shipped for the other commands). When the tagpath index is missing entirely, the audit reports `tagpath_state: "missing"` and `tagpath_only_files: []`.

#### Implementation
`cmd_audit_tagpath` in `src/main.rs`:

1. Resolves the project root, optionally narrowing to a workspace scope via `config::Config::resolve_submodule`.
2. Opens `IndexDb` read-only at the resolved scope's `.tsift/indexes/<scope>/index.db` (or workspace root `.tsift/index.db`).
3. Normalizes tsift's stored absolute paths to relative-to-tagpath-root for set comparison while remembering the absolute form so `db.symbols_for_file` can still query it.
4. Builds the set of tagpath sources from `adapter.index.sources` (already relative).
5. Diffs both sets and counts per-file tsift symbols for each tsift-only file.

#### Test
`tests/exit_code.rs::audit_tagpath_reports_walker_diff` — uses a `__pycache__/lib.rs` + `main.rs` fixture. Tagpath's hard-coded `SKIP_DIRS` skips `__pycache__/`, but tsift's `ignore`-based walker indexes it. The test runs `tsift index` then `tsift audit-tagpath --json`, asserts `tagpath_state == "fresh"`, that `__pycache__/lib.rs` lands in `tsift_only_files`, and that the per-file symbol count surface shows `cached_helper`'s file with `symbols >= 1`.

#### Verification
- `cargo test --test exit_code audit_tagpath_reports_walker_diff` — green
- `make check` — green (full clippy + suite)
- `cargo install --path .` — installed `tsift 0.1.59` to `~/.cargo/bin/`

#### Files
- `src/tsift/src/main.rs` — `AuditTagpath` clap variant + dispatch + `cmd_audit_tagpath`
- `src/tsift/tests/exit_code.rs` — `audit_tagpath_reports_walker_diff`
- `src/tsift/VERSIONS.md` — 0.1.59 entry
- `src/tsift/CLAUDE.md` — command table row for `tsift audit-tagpath`
- `src/tsift/Cargo.toml`, `Cargo.lock` — version → 0.1.59
- Superproject `src/tsift` submodule pointer bumped

Out of scope for this turn (would be follow-ups if needed): policy-hint surfacing (excluded paths, language coverage gaps), `--scope` test coverage (the implementation routes through `resolve_submodule` but the test exercises only the workspace-root path). Both can land later if operators ask for them.
❯ Add #next-steps for follow-ups
### Re: Add #next-steps for follow-ups — opus-4-7

Added four follow-up items derived from the four tracked items closed this session (`#commhx`, `#p6tsifullfederated`, `#tpstdiag`, `#tpaudit`), reordered the backlog to put the silent-bug follow-up first and demote the existing `[recommended]` items.

**Top of backlog: `#p6tsifullscoped`**

#### New items (in priority order)

1. **`#p6tsifullscoped`** (HIGH — silent bug, same pattern as just-shipped `#p6tsifullfederated`)
   `tsift search --scope <name>` and inferred-scope paths still annotate against the workspace root. Scoped searches in workspaces where the submodule (not the workspace root) owns the `.naming.toml` return zero handles. Fix is mechanical: route `&sift_path` into `annotate_hits_with_tagpath` in `cmd_search_with_budget`.

2. **`#tpauditscope`** (MEDIUM — locks in scope path for the just-shipped audit feature)
   `tsift audit-tagpath --scope <name>` exists and routes correctly, but the test suite only exercises the workspace-root path. Add a multi-submodule fixture so the scoped path can't silently regress.

3. **`#tpauditpolicy`** (MEDIUM — completes the audit feature)
   The audit reports WHAT diverges but not WHY. Add `tsift_only_files_with_policy_hints` JSON field that surfaces tagpath `SKIP_DIRS` matches and language-coverage gaps so operators can act on the diff with intent.

4. **`#commhxstrong`** (MEDIUM-LOW — stronger variant of `#commhx`)
   Cheap fix in 0.1.56 returns the first row with a valid handle. Stronger fix uses Louvain community + `caller_file` evidence to pick the row that matches the actual call edge. Only worth doing if name collisions across files with valid handles produce visible wrong-handle bugs in practice.

#### Reordered queue

After this turn the backlog ordering is:

1. `#p6tsifullscoped`
2. `#tpauditscope`
3. `#tpauditpolicy`
4. `#commhxstrong`
5. `#p6tspub`
6. `#p6ren`
7. `#mixconv` (existing `[recommended]` — demoted below the new follow-ups)
8. `#p6meta` (existing `[recommended]`)

The queue auto-prompts will pick up `#p6tsifullscoped` next.
<!-- agent:boundary:f2f8b93f -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#commhx]~
- ~do [#p6tsifullfederated]~
- ~do [#tpstdiag]~
- ~do [#tpaudit]~
- do [#mixconv]
- do [#p6tspub]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#p6tsifullscoped] `tsift search --scope <name>` and inferred-scope search paths still annotate against the workspace root instead of the scope's `source_root`. Same silent-handle-loss pattern as the federated bug `#p6tsifullfederated` (closed in 0.1.57). When a submodule has its own `.naming.toml` / `.naming/index.json` but the workspace root has none, scoped searches return zero `tagpath_handle` even though the data is right there. Fix: in `cmd_search_with_budget`, pass `&sift_path` (already populated with `scope.source_root` for scoped paths) into `annotate_hits_with_tagpath` instead of `&root`. Tests: extend `tests/exit_code.rs` with a single-submodule fixture where the workspace root has no `.naming.toml` but the submodule does; assert `tsift search ... --scope <name> --json` returns hits with `tagpath_handle` set.
- [ ] [#tpauditscope] `tsift audit-tagpath --scope <name>` test coverage. The implementation routes through `config::Config::resolve_submodule` and opens the scoped `index.db` + tagpath at `scope.source_root`, but the `audit_tagpath_reports_walker_diff` test only exercises the workspace-root path. Add a fixture with `alpha` + `beta` submodules where one submodule's tagpath skips a path tsift indexes; assert the JSON `scope: "<name>"` field is set and only that submodule's files appear in `tsift_only_files`.
- [ ] [#tpauditpolicy] Policy-hint surfacing for `tsift audit-tagpath`. Today the audit reports WHAT diverges (tsift-only / tagpath-only files) but not WHY. Parse the resolved `NamingConfig` (including `extends` chain) and surface: (a) hard-coded `SKIP_DIRS` hits per tsift-only file (the obvious explanation when files live under `__pycache__/`, `vendor/`, `target/`, `node_modules/`, `.venv/`, `.git/`), (b) language coverage gaps (extension not in tagpath's `SOURCE_EXTENSIONS` but tsift's walker accepted it). New JSON fields: `tsift_only_files_with_policy_hints: [{file, hints: ["skip_dir:__pycache__", "extension_unsupported"]}]`. Operators can then decide whether to broaden tagpath, narrow tsift, or accept the gap with intent.
- [ ] [#commhxstrong] Stronger fix for `#commhx` name-collision wrong-handle (closed cheaply in 0.1.56 by iterating `symbol_info` rows). Today when two files define the same symbol AND both have valid tagpath handles, the resolver returns the FIRST file's handle, which may not match the actual call edge's resolved file. Stronger fix: when resolving a callee handle for a caller row, prefer the `symbol_info` row whose file lives in the same Louvain community as the caller (use `caller_file` evidence). Falls back to current first-handle-wins behavior when no community context is available (e.g. `tsift communities` itself). Touches `annotate_stored_edges_with_tagpath::EdgeSide::Callee` and the per-row `resolve_tagpath_handles_for_names` helper. Tests: extend the name-collision fixture from 0.1.56 with two valid tagpath handles (both files indexed by tagpath); assert the caller's callee handle matches the file in the caller's community.
- [ ] [#p6tspub] Restore tsift crates.io publishability after p6tsi: tsift now depends on tagpath via path = ../tagpath. Replace with version = "0.17.1" (or whatever ships first) before any tsift release. Already gated by the sift-publish blocker so no immediate regression, but track here.
- [ ] [#p6ren] tagpath rename <old> <new>: cross-file, cross-convention identifier rename using the index. Walks every member of the matching family, rewrites in-place in each source convention (snake/camel/Pascal/kebab/UPPER/Ada). Foundation for the agent-loop intent-preserving refactor vision.
- [ ] [#mixconv] [recommended] tagpath lint policy that tolerates mixed conventions within a single identifier (e.g. `createUserProfile_v2`). Today `parser::detect_convention` classifies any identifier containing `_` as snake-family, so the camelCase-plus-suffix shape lint-fails in a camelCase-only context with `create_user_profile_v2` as the suggested fix. Add a per-context `tolerate_mixed = true` or top-level `lint.allow_mixed_within_identifier` policy that compares tag-equivalence rather than convention-equivalence. Single-file change in `src/lint/mod.rs:79`. Tests: extend `tests/lint.rs` with mixed-within-identifier cases.
- [ ] [#p6meta] [recommended] agent-loop meta-index: aggregate per-submodule .naming/index.json files into a top-level handle registry so cross-repo searches resolve. New CLI: tagpath meta-index <workspace-root>. Useful once 2+ siblings adopt tagpath.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#p6adfmerge] Merge agent-doc feature branch 20260422/codex-agent-backend to main: the p6adf commit (845ddfd6) and prior p4*/p5* prep ride on this branch. agent-loop superproject already tracks feature-branch commits, but agent-doc main is behind and will need a sweep before any external consumer pulls.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
