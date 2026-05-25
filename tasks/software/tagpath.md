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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-004250.md`*

Compacted content:
- Archived 2 response topic(s): next-steps; queue auto
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
❯ preset #spec-test-build-install-commit-push
❯ - do [#p5sln]
❯ - do [#p5wat]
❯ - do [#p5mcs]
❯ - do [#p5wnp]
### Re: queue auto — do [#p5sln] — opus-4-7


**Done.** Agent-doc dialect for `tagpath lint` shipped in tagpath 0.14.0; submodule pointer bumped + pushed. The malformed-directive bug we hit mid-session (`agent:done archive PATH` without the `=`) now fails at lint time before reaching `finalize` — that's the canary the whole feature was built around. Side fix: re-strikethrough'd p5xfast in the queue (the prior cycle's queue consumer missed it).

**14 rules wired, each with passing positive + negative tests:**
1. `unknown-component`
2. `unclosed-component`
3. `orphan-close`
4. `duplicate-component`
5. **`malformed-attr`** — fires on `archive PATH` with a fix hint suggesting `archive=PATH`. The motivating bug.
6. `empty-attr-value`
7. `unknown-attr` — per-component allow-list (`agent:status/exchange/backlog/review/icebox/queue` → `patch`; `agent:done` → `archive`, `patch`)
8. `queue-mode-token` — bare `auto` / `manual` only; `mode=auto` rejected
9. `done-archive-missing-target` — fs-checks; also enforces `.done.md` suffix
10. `patch-marker-outside-cycle` (warning)
11. `backlog-id-collision`
12. `done-id-not-in-backlog` — fs-checks; honors `<!-- migrated -->` escape
13. `malformed-boundary` (bonus)
14. `unknown-patch-marker` (bonus)

Filesystem-dependent rules (#9, #12) are gated behind `--fs-checks` so the default lint stays fast and offline.

**Changes (tagpath submodule, cb874c8):**
- `src/lint/agent_doc.rs` (new) — hand-rolled HTML-comment parser + rule engine + structured `LintFinding`.
- `src/lint/mod.rs` — re-exports the new module.
- `src/main.rs` — `Lint` subcommand gains `--dialect {agent-doc,identifier,auto}`, `--fs-checks`, `--rule <id>` (repeatable). Auto-detect: a `.md` file containing `<!-- agent:exchange` opts into the agent-doc dialect; directory walks combine.
- `tests/test_lint_agent_doc.rs` (new, 13 integration tests) — including the explicit regression test for `agent:done archive PATH`.
- `SPEC.md` — new section 16 "Agent-doc dialect" (rules table, CLI flags, examples).
- `README.md` — `tagpath lint --dialect agent-doc tasks/` bullet.
- `Cargo.toml` — `0.13.0 → 0.14.0`; `Cargo.lock` refreshed.

**Verification:**
- `cargo fmt` + clippy clean on default and `dyn-grammar`.
- `cargo test` — **196/196 pass** across 13 suites (was 166 + 17 unit + 13 integration).
- `cargo test --features dyn-grammar` — **200/200 pass**.
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` — clean.
- `cargo build --release` + `cargo install --path . --force` — `~/.cargo/bin/tagpath` at 0.14.0.

**Smoke (release binary):**
- `tagpath lint --dialect agent-doc tasks/software/tagpath.md` → **0 findings**, exit 0.
- `tagpath lint --dialect agent-doc tasks/software/tsift.md` → **0 findings**, exit 0.
- Negative: a fixture with `<!-- agent:done archive tasks/x.done.md -->` → 2 `agent-doc/malformed-attr` errors with fix hints, exit 1. The motivating bug class is now caught at lint time.

**Commits:**
- tagpath `cb874c8`: `feat: agent-doc dialect for tagpath lint (p5sln)` → pushed.
- agent-loop `294c164`: `tagpath: bump submodule to 0.14.0 (p5sln)` → pushed.

Auto-queue advances next to `tagpath watch`.
<!-- agent:boundary:fb728c7c -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#p5tsi]~
- ~do [#p5inc]~
- ~do [#p5xfast]~
- ~do [#p5sln]~
- do [#p5wat]
- do [#p5mcs]
- do [#p5wnp]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#p5wat] tagpath watch: long-running watcher that emits index update events on file changes (newline-delimited JSON), suitable for tsift's incremental graph projection and agent-doc tag validators that need to react inside a cycle.
- [ ] [#p5mcs] MCP convention helpers for tsift and agent-doc: add tagpath mcp tools family_by_path, lint_session_doc, and index_handle so both upstreams can call tagpath without bespoke shell wrappers.
- [ ] [#p5wnp] [recommended] Publish the wasm-pack tarball to npm (@btakita/tagpath-wasm) so agent-doc front-end surfaces and tsift JS adapters can call parse / alias / normalize_query / search_over_rows in-browser without a native dep.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
