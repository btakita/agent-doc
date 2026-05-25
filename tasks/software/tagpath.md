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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-092327.md`*

Compacted content:
- Archived 2 response topic(s): do [#p6tsifullscoped]; do [#p6tsifullscoped]
- Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c...
### Re: do [#tpauditscope] — opus-4-7

Shipped in tsift 0.1.61 (`6a3e795`, superproject pointer bumped). Test-only change locking in the per-submodule audit path that the 0.1.59 implementation already supported.

#### Test
`tests/exit_code.rs::audit_tagpath_scope_reports_per_submodule_walker_diff` builds an `alpha` + `beta` workspace:

- **alpha**: contains `alpha/__pycache__/lib.rs` (tsift indexes, tagpath SKIP_DIRS) and `alpha/lib.rs`. Has its own `.naming.toml` and `.naming/index.json` rooted at `src/alpha/`.
- **beta**: contains `beta/lib.rs` only. Has its own `.naming.toml` and `.naming/index.json` rooted at `src/beta/`.

Then exercises both scope paths:

- `tsift audit-tagpath --scope alpha --json` — asserts `scope: "alpha"`, asserts `__pycache__/lib.rs` lands in `tsift_only_files`, and asserts NO beta files leak into the alpha report.
- `tsift audit-tagpath --scope beta --json` — asserts `scope: "beta"` and empty `tsift_only_files` + `tagpath_only_files` (clean coverage).

The leak-prevention assertion is the load-bearing one: it locks in that `cmd_audit_tagpath` resolves the tagpath root from `scope.source_root` instead of falling back to the workspace root, and that `IndexDb` opens the scoped `.tsift/indexes/<scope>/index.db` rather than the workspace root.

#### Verification
- `cargo test --test exit_code audit_tagpath_scope_reports_per_submodule_walker_diff` — green
- `make check` — green
- `cargo install --path .` — `tsift 0.1.61` installed

#### Files
- `src/tsift/tests/exit_code.rs` — `audit_tagpath_scope_reports_per_submodule_walker_diff`
- `src/tsift/VERSIONS.md` — 0.1.61 entry
- `src/tsift/Cargo.toml`, `Cargo.lock` — version → 0.1.61
- Superproject `src/tsift` submodule pointer bumped

No source changes — `cmd_audit_tagpath` already handled the scoped path correctly when shipped in 0.1.59, so this turn locks that behavior in with deterministic test coverage.
<!-- agent:boundary:da2f362f -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#tpauditscope]~
- do [#tpauditpolicy]
- do [#commhxstrong]
- do [#p6tspub]
- do [#p6ren]
- do [#mixconv]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
