---
agent_doc_session: 4602f467-ef2a-4094-a750-5eb533368988
agent: codex
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-161445.md`*

Compacted content:
- Archived 1 response topic(s): do [#tpauditscope]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#tpauditpolicy] — gpt-5

Implemented and pushed `#tpauditpolicy`.

What changed:
- `tsift audit-tagpath --json` now emits `tsift_only_files_with_policy_hints`, with entries like `{ "file": "__pycache__/lib.rs", "hints": ["skip_dir:__pycache__"] }`.
- Policy hints now cover tagpath hard-coded skip directories (`.git`, `node_modules`, `target`, `__pycache__`, `.venv`, `vendor`) and tsift-indexed extensions that tagpath does not cover (`extension_unsupported`).
- The hint helper resolves `.naming.toml` through tagpath's `NamingConfig` extends chain and includes configured dynamic grammar extensions before treating an extension as unsupported.
- Human `audit-tagpath` output now prints the same hints beside tsift-only file symbol counts.
- `tests/exit_code.rs::audit_tagpath_reports_walker_diff` now covers both a skipped `__pycache__` file and a `.pyi` file that tsift indexes while tagpath skips.
- CI now checks out sibling `btakita/tagpath` at `../tagpath`, fixing the latest red CI failure where the standalone tsift checkout could not resolve the local tagpath path dependency.

Verification:
- `cargo test audit_tagpath_reports_walker_diff` passed.
- `make check` passed locally after the final edits.
- `cargo install --path .` passed.
- `gh run watch 26410346141 --exit-status` passed: both `clippy + test` and `live Convex graph acceptance` succeeded.
- tmux CI review: no `tmux`, `tmux-ci`, or `tmux-test` workflow/Makefile references exist in this repo.

Commits pushed:
- `btakita/tsift@a37937f` — `feat: surface audit-tagpath policy hints`
- `btakita/agent-loop@d6adbb3af` — `chore: update tsift audit policy hints`

I left the pre-existing unrelated dirty tsift files unstaged: `Cargo.toml`, `Cargo.lock`, `VERSIONS.md`, and `plans/gdbvacproof-evidence.md`.
### Re: do [#commhxstrong] — gpt-5

Implemented and pushed `#commhxstrong`.

What changed:
- `annotate_stored_edges_with_tagpath(..., EdgeSide::Callee)` now resolves callee handles per edge instead of sharing one `callee_name -> tagpath_handle` result across all rows.
- Callee resolution now prefers a candidate definition in the caller's own file, then a candidate whose file has overlapping Louvain community evidence with the caller file, then falls back to the previous first-resolving-handle behavior for no-context surfaces such as `tsift communities`.
- The no-context `resolve_tagpath_handles_for_names` helper remains the fallback path for community/path-style annotation.
- `SPEC.md` documents the per-edge callee-handle collision behavior.
- Added `graph_json_resolves_callee_handle_with_caller_file_name_collision`, covering two valid tagpath handles for the same `helper` name and asserting `graph src_caller --json` chooses the `src/main.rs` handle rather than the first sorted vendor handle.

Verification:
- `cargo test graph_json_resolves_callee_handle_with_caller_file_name_collision --test exit_code -- --nocapture` passed.
- `make check` passed after the final tree edits.
- `cargo install --path .` passed.
- `gh run watch 26411018042 --exit-status` passed: both `clippy + test` and `live Convex graph acceptance` succeeded.
- tmux CI review: no `tmux`, `tmux-ci`, or `tmux-test` workflow/Makefile references exist in this repo.

Commits pushed:
- `btakita/tsift@5b049cf` — `fix: resolve callee tagpath handle collisions`
- `btakita/agent-loop@24664c721` — `chore: update tsift callee handle resolution`

I left the pre-existing unrelated dirty tsift files unstaged: `Cargo.toml`, `Cargo.lock`, `VERSIONS.md`, and `plans/gdbvacproof-evidence.md`.
### Re: do [#p6tspub] — gpt-5

Implemented and pushed `#p6tspub`.

What changed:
- `tsift` no longer depends on sibling `../tagpath` for publish builds. `Cargo.toml` now uses the published crate dependency: `tagpath = { version = "0.11.1", default-features = false, features = [...] }`.
- `Cargo.lock` now records `tagpath v0.11.1` as a crates.io registry package with source/checksum, so packaging does not rely on the local sibling checkout.
- I used `0.11.1` rather than the old backlog placeholder `0.17.1`: the published docs.rs version page shows `v0.11.1 — 2026-05-25`, and the `v0.11.0` notes say local `0.11–0.18` versions were not published and were consolidated into the `0.11.0` crates.io release: https://docs.rs/crate/tagpath/0.11.1/source/VERSIONS.md

Verification:
- `cargo publish --dry-run --allow-dirty --manifest-path /home/brian/work/btakita/agent-loop/src/tsift/Cargo.toml` passed from `/tmp`, outside the agent-loop parent `.cargo/config.toml` patch scope. Cargo initially hit one transient crates.io 500 while downloading `tagpath v0.11.1`, retried, downloaded it, compiled the packaged crate, and aborted upload because dry run.
- `make check` passed locally.
- `cargo install --path .` passed.
- `gh run watch 26411462514 --exit-status` passed: both `clippy + test` and `live Convex graph acceptance` succeeded. CI had only non-blocking annotations for GitHub's Node.js 20 action deprecation and the live Convex URL note.
- tmux CI review: no `tmux`, `tmux-ci`, or `tmux-test` workflow/Makefile references exist in this repo.

Commits pushed:
- `btakita/tsift@8c5bff7` — `fix: use published tagpath dependency`
- `btakita/agent-loop@6d5c56010` — `chore: update tsift published tagpath dependency`

I left the pre-existing unrelated dirty tsift changes unstaged: the `0.1.62` version bump in `Cargo.toml`/`Cargo.lock`, `VERSIONS.md`, and `plans/gdbvacproof-evidence.md`.
### Re: do #p6ren — gpt-5

Implemented `tagpath rename <old> <new> [path]` as an index-backed family rename.

Code changes:
- Added `tagpath::rename` for loading or refreshing the index, finding the old family, rendering the new tags in each member's source convention, rewriting token-boundary-safe occurrences in-place, and rebuilding the index after a non-dry-run rename.
- Wired the CLI as `tagpath rename <OLD> <NEW> [<PATH>] [--dry-run] [--format text|json]`.
- Documented the command in `README.md` and `SPEC.md`.
- Added integration coverage for snake, camel, Pascal, kebab, upper-snake, Ada, refreshed-index behavior, and dry-run no-write behavior.

Verification:
- `cargo test --test test_rename`
- `cargo test`
- `cargo clippy`
- `cargo install --path .`
- `make check`
- GitHub Actions CI for `btakita/tagpath@3b34185` passed: run `26412025982`, job `check` in 56s. The only annotation was the existing Node 20 deprecation notice for `actions/checkout@v4`.

Commits:
- `btakita/tagpath@3b34185` pushed to `main`.
- Parent submodule pointer committed locally as `agent-loop@92a1afda8`; I will push the parent commit after this document finalize, per the agent-doc closeout rule.
### Re: do #mixconv — gpt-5

Implemented the mixed-convention lint policy as a top-level config option: `[lint] allow_mixed_within_identifier = true`.

Code changes:
- Added `LintConfig` to `.naming.toml` parsing and config inheritance.
- Updated lint convention checks so the mixed policy suppresses violations only when the identifier has a genuinely mixed surface, such as `createUserProfile_v2`, and parsing it under the expected convention produces the same tag list.
- Kept plain wrong-convention identifiers flagged, so `ParseName` still fails under a `snake_case` function rule even when the mixed policy is enabled.
- Documented the config surface in `README.md` and `SPEC.md`.
- Added regression tests for default failure, policy success, and the plain-wrong-convention guard.

Verification:
- `cargo test lint::tests::test_lint_`
- `cargo test`
- `cargo clippy`
- `cargo install --path .`
- `make check`
- GitHub Actions CI for `btakita/tagpath@d51aed6` passed: run `26412376372`, job `check` in 46s. The only annotation was the existing Node 20 deprecation notice for `actions/checkout@v4`.

Commits:
- `btakita/tagpath@d51aed6` pushed to `main`.
- Parent submodule pointer committed locally as `agent-loop@0ca2dadc8`; I will push the parent commit after this document finalize, per the agent-doc closeout rule.
<!-- agent:boundary:aac624bc -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#tpauditscope]~
- ~do [#tpauditpolicy]~
- ~do [#commhxstrong]~
- ~do [#p6tspub]~
- ~do [#p6ren]~
- ~do [#mixconv]~
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
