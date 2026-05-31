---
agent_doc_session: 4602f467-ef2a-4094-a750-5eb533368988
agent: opencode
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
queue_active: false
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
v0.12.0 published to crates.io — tagpath-core split release complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260530-183916.md`*

Compacted content:
- Archived 5 response topic(s): do #tpcoreextract; do #tpcorefacade; do #tpcoreci; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
❯ publish 0.12.0

### Re: publish 0.12.0 — gpt-5

Published `tagpath` `0.12.0`.

- Published `tagpath-core 0.12.0` and `tagpath 0.12.0` to crates.io: https://crates.io/crates/tagpath-core and https://crates.io/crates/tagpath
- Tagged and pushed `v0.12.0` in `btakita/tagpath`; GitHub release is at https://github.com/btakita/tagpath/releases/tag/v0.12.0
- Updated `VERSIONS.md` from `Unreleased` to `2026-05-30` in tagpath commit `682fcba`.
- Committed the agent-loop superproject submodule pointer update as `eb15c8aa4`.

Verification:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p tagpath-core --no-default-features`
- `cargo test -p tagpath --lib --no-default-features`
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm`
- `scripts/check-release.sh` before and after publishing `tagpath-core`
- `cargo install --path . --force`
- `cargo search tagpath --limit 3` and `cargo search tagpath-core --limit 3` both show `0.12.0`
- GitHub `tagpath` CI run `26692232890` passed.
- GitHub `tagpath` Release run `26692233759` passed.
- Root `make check` passed.

Root CI note: the latest agent-loop `Agent Doc` workflow run `26691990789` failed before any job steps were allocated for both `check` and `tmux-ci`, so I’m recording it as an external CI-start failure rather than a code or tmux regression. I did not run `make tmux-ci` because there was no runner-started tmux failure to reproduce.

Changed paths:

- `src/tagpath/VERSIONS.md` in the tagpath repo
- `src/tagpath` submodule pointer in agent-loop
- this session document via `agent-doc finalize`

Escalation: no approval prompts were needed; the publish, tag, and push operations were authorized by the document prompt `publish 0.12.0`. Existing unrelated agent-loop worktree changes were left untouched.

Confidence: high.
❯ Is the latest version published? If not, publish.

### Re: publish 0.12.1 — glm-5.1

Published `tagpath` `0.12.1`.

- Published `tagpath-core 0.12.1` and `tagpath 0.12.1` to crates.io
- Tagged and pushed `v0.12.1` in `btakita/tagpath`; GitHub release at https://github.com/btakita/tagpath/releases/tag/v0.12.1
- Updated `VERSIONS.md` with the v0.12.1 changelog entry
- Updated test expectations in `test_core_split_plan.rs` for the new version
- Committed agent-loop superproject submodule pointer update as `59086ccb7`

Three unreleased commits were on top of `v0.12.0`: new lint rules for bare `queue`/`priority`/`auto` attributes on backlog/icebox/queue markers, plus the `agent-doc/invalid-attr-value` warning rule.

Verification:

- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo test --workspace` — 284 tests pass
- `cargo test -p tagpath-core --no-default-features` pass
- `cargo test -p tagpath --lib --no-default-features` pass (77 tests)
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` success
- `scripts/check-release.sh` passed after `tagpath-core` publish
- `cargo search tagpath --limit 3` and `cargo search tagpath-core --limit 3` both show `0.12.1`

Changed paths:

- `Cargo.toml`, `crates/tagpath-core/Cargo.toml`, `tests/test_core_split_plan.rs`, `VERSIONS.md`, `SPEC.md` in the tagpath repo
- `src/tagpath` submodule pointer in agent-loop
- this session document via `agent-doc finalize`

Escalation: none. Publish and push were authorized by the document prompt.

Confidence: high.
<!-- agent:boundary:55853765 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#p6adfmerge] Merge agent-doc feature branch 20260422/codex-agent-backend to main: the p6adf commit (845ddfd6) and prior p4*/p5* prep ride on this branch. agent-loop superproject already tracks feature-branch commits, but agent-doc main is behind and will need a sweep before any external consumer pulls.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
