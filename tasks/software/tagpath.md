---
agent_doc_session: 4602f467-ef2a-4094-a750-5eb533368988
agent: codex
claude_model: claude-opus-4-6
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

### Phase 4 — Index, ontology, dynamic loading, WASM, MCP

- [ ] `tagpath index` — persistent index (`.naming/index.json`) for instant queries
- [ ] Tag ontology — `.naming/tags/*.md` domain vocabulary (philosophy-style)
- [ ] Dynamic `.so` grammar loading — runtime extensibility via `libloading`
- [ ] WASM build — feature flag excluding tree-sitter, npm package via `wasm-pack`
- [ ] MCP tool integration — expose parse/search/lint to coding agents

### Design Decisions

- **Explicit imports only** — no implicit/magic config loading. `tagpath init --lang rust` generates full config; `extends = ["rust"]` tracks upstream.
- **Feature flags over monolith** — tree-sitter grammars are opt-in per language. `--no-default-features` gives regex-only.
- **Dynamic loading deferred** — feature flags cover immediate need. Runtime `.so` loading (Helix/Neovim style) for Phase 4.
- **WASM deferred** — core parser/search compile to WASM cleanly; tree-sitter stays native. Not a priority.

## Status

<!-- agent:status patch=replace -->
v0.6.0 published to crates.io
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260507-034315.md`*
❯ How can tagpath further assist tsift in saving tokens? #next-steps
### Re: tagpath token savings — gpt-5

Yes. `tagpath` can help `tsift` save more tokens in three concrete ways.

1. Top leverage: replace repeated raw identifier text with stable tag handles.
`tsift` already has compact output, preview budgets, and expansion handles. `tagpath` can shrink those further by turning repeated names like `get_user_profile`, `getUserProfile`, and `GetUserProfile` into one canonical tag handle plus one display alias. That matters most in `search`, `explain`, `context-pack`, and digest surfaces where the same symbol names repeat across sections.

2. Normalize queries before fallback search.
If an agent asks for `getUserProfile`, `user profile getter`, or `profile user get`, `tagpath` can canonicalize that intent into the same ordered tag query before `tsift` chooses exact search, symbol search, or BM25 fallback. That should reduce wasted broad searches and therefore reduce the size of the returned context.

3. Collapse result sets by tag family instead of listing each symbol separately.
When many hits differ only by convention, prefix, or namespace suffix, `tsift` can emit one family summary row first, with counts and an expand command. That preserves recall while spending tokens only when the caller actually drills in.

Recommended next steps are now in backlog priority order. Top item: `#tgfh`.
❯ do [#tgfh]. spec-test-build-install-commit-push
### Re: #tgfh preview tag aliases — gpt-5

Implemented in `src/tsift` and pushed on `main` as `2acd1b1`.

Changes:
- `tsift search` preview symbols now emit canonical `tag_alias` values derived from `tagpath` alongside the existing stable handles.
- `tsift explain` preview definitions/call edges now emit the same canonical aliases, and the human preview format surfaces them.
- `tsift context-pack` now carries compact symbol-ref objects for `next_context`, diff previews, and log symbol refs while keeping raw `touched_symbols` for compatibility.
- `SPEC.md` documents the new preview fields and handoff behavior.
- Regression coverage was added in `src/main.rs` and `tests/exit_code.rs`.

Verification:
- Passed: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo build`, `cargo install --path . --force`.
- Passed focused tests:
  - `cargo test tests::search_budget_report_truncates_symbol_preview_and_emits_stable_handle -- --exact`
  - `cargo test tests::explain_budget_report_limits_edges_and_members -- --exact`
  - `cargo test tests::session_review_next_context_budget_limits_lists -- --exact`
  - `cargo test tests::context_pack_diff_preview_limits_files_and_symbols -- --exact`
- `cargo test` is still red on pre-existing repo failures unrelated to this patch. Current failing tests in this environment:
  - `diff_digest::tests::diff_digest_cached_uses_index_snapshot_instead_of_working_tree`
  - `diff_digest::tests::diff_digest_revision_uses_commit_snapshots`
  - `tests::community_cmd_no_index_errors`
  - `tests::explain_cmd_no_index_errors`
  - `tests::graph_cmd_no_index_errors`
  - `tests::lint_finds_entities_from_project_root_index_db`
  - `tests::path_cmd_no_index_errors`
  - `tests::search_timeout_message_reports_missing_index_as_rebuild_needed`
  - `context_pack_json_composes_next_context_and_optional_digests`
<!-- agent:boundary:d5a40565 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#tgqn] tsift: normalize agent search queries through tagpath so cross-convention intent resolves before lexical fallback
- [ ] [#tgcl] tsift: cluster repeated symbol results by canonical tag family with one summary row + expand handle
<!-- /agent:backlog -->
