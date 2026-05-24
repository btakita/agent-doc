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
v0.6.2 published to crates.io
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260524-222233.md`*

Compacted content:
- Archived 1 response topic(s): agent:queue auto halt
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: queue auto — do [#p4wm] — opus-4-7


**Done.** WASM packaging shipped in tagpath 0.9.0; submodule pointer bumped + pushed.

**Changes (tagpath submodule, d30c630):**
- `Cargo.toml` — version `0.8.0 → 0.9.0`; `[lib] crate-type = ["cdylib", "rlib"]`; `[[bin]] required-features = ["treesitter"]`; new `wasm` feature; optional deps `wasm-bindgen`, `serde-wasm-bindgen`, `js-sys`.
- `src/lib.rs` — `extract`, `graph`, `index`, `lint`, `mcp`, `ontology`, `search`, `treesitter` all gated `#[cfg(not(target_arch = "wasm32"))]`. New `pub mod wasm` gated for wasm.
- `src/wasm/mod.rs` (new) — `parse`, `alias`, `prose`, `normalize_query`, `search_over_rows`. The row-search path uses `query::normalize_query` so multi-word queries decompose correctly. No filesystem access from WASM by design — callers pass extracted rows.
- `tests/test_wasm_surface.rs` (new) — 8 native tests covering parse / alias / prose / normalize_query / search predicate shapes (the wasm wrappers call the same native paths).
- `SPEC.md` — new section 13 "WASM" (feature flag, API surface, no-fs rule, wasm-pack command).
- `README.md` — WASM build section + JS snippet.

**Verification:**
- `rustup target add wasm32-unknown-unknown` (new toolchain target).
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` — compiles clean (66 crates).
- `cargo fmt` clean.
- `cargo clippy --all-targets --all-features -- -D warnings` — clean (same 2 unrelated workspace `patch` advisories).
- `cargo test` — **132/132 pass** across 8 suites (124 prior + 8 new wasm-surface).
- `cargo build --release` + `cargo install --path .` — `~/.cargo/bin/tagpath` replaced 0.8.0 → 0.9.0.

**Commits:**
- tagpath `d30c630`: `feat: add tagpath wasm packaging (p4wm)` → pushed `btakita/tagpath:main`.
- agent-loop `77b5a6d`: `tagpath: bump submodule to 0.9.0 (p4wm)` → pushed `btakita/agent-loop:main`.

**Punted (intentional):**
- `wasm-pack` is not installed on this host. The native `cargo build --target wasm32` proves the surface compiles, and `tests/test_wasm_surface.rs` exercises the underlying logic, but no JS-side smoke test ran. Easy follow-up: install `wasm-pack` and run `wasm-pack build --target nodejs --no-default-features --features wasm`.
- `alias` map keys use `Convention::Display` form (`camelCase`, `kebab-case`) rather than the serde-renamed snake form inside `ParsedName`. Documented in SPEC section 13.2 — downstream tweak if normalization is wanted.
- No crates.io publish, no npm publish — those land with the release-hygiene queue item.

Auto-queue advances next to dynamic grammar loading.
### Re: queue auto — do [#p4wm] — opus-4-7


**Done.** WASM packaging shipped in tagpath 0.9.0; submodule pointer bumped + pushed.

**Side fix in this cycle:** the `&lt;!-- agent:done archive ... --&gt;` directive was missing the `=` and quoting that the parser requires, which blocked the prior finalize. Corrected to `archive="tasks/software/tagpath.done.md"`.

**Changes (tagpath submodule, d30c630):**
- `Cargo.toml` — version `0.8.0 → 0.9.0`; `[lib] crate-type = ["cdylib", "rlib"]`; `[[bin]] required-features = ["treesitter"]`; new `wasm` feature; optional deps `wasm-bindgen`, `serde-wasm-bindgen`, `js-sys`.
- `src/lib.rs` — `extract`, `graph`, `index`, `lint`, `mcp`, `ontology`, `search`, `treesitter` all gated `#[cfg(not(target_arch = "wasm32"))]`. New `pub mod wasm` gated for wasm.
- `src/wasm/mod.rs` (new) — `parse`, `alias`, `prose`, `normalize_query`, `search_over_rows`. The row-search path uses `query::normalize_query` so multi-word queries decompose correctly. No filesystem access from WASM by design — callers pass extracted rows.
- `tests/test_wasm_surface.rs` (new) — 8 native tests covering parse / alias / prose / normalize_query / search predicate shapes.
- `SPEC.md` — new section 13 "WASM" (feature flag, API surface, no-fs rule, wasm-pack command).
- `README.md` — WASM build section + JS snippet.

**Verification:**
- `rustup target add wasm32-unknown-unknown`.
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` — compiles clean (66 crates).
- `cargo fmt` clean.
- `cargo clippy --all-targets --all-features -- -D warnings` — clean.
- `cargo test` — **132/132 pass** across 8 suites (124 prior + 8 new wasm-surface).
- `cargo build --release` + `cargo install --path .` — `~/.cargo/bin/tagpath` replaced 0.8.0 → 0.9.0.

**Commits:**
- tagpath `d30c630`: `feat: add tagpath wasm packaging (p4wm)` → pushed `btakita/tagpath:main`.
- agent-loop `77b5a6d`: `tagpath: bump submodule to 0.9.0 (p4wm)` → pushed `btakita/agent-loop:main`.

**Punted (intentional):**
- `wasm-pack` is not installed on this host. The cross-compile proves the surface builds and `tests/test_wasm_surface.rs` exercises the underlying logic, but no JS-side smoke test ran. Follow-up: install `wasm-pack` and run `wasm-pack build --target nodejs --no-default-features --features wasm`.
- `alias` map keys use `Convention::Display` form (`camelCase`, `kebab-case`) rather than the serde-renamed snake form inside `ParsedName`. Documented in SPEC 13.2 — downstream normalization is opt-in.
- No crates.io / npm publish — those land with the release-hygiene queue item.

Auto-queue advances next to dynamic grammar loading.
### Re: queue auto — do [#p4wm] — opus-4-7


**Done.** WASM packaging shipped in tagpath 0.9.0; submodule pointer bumped + pushed.

**Side fixes this cycle:**
- `&lt;!-- agent:done archive ... --&gt;` directive was missing the `=` + quoting the parser requires → corrected to `archive="tasks/software/tagpath.done.md"`.
- The referenced `tasks/software/tagpath.done.md` did not yet exist → created with the standard header so the archive directive resolves.

**Changes (tagpath submodule, d30c630):**
- `Cargo.toml` — version `0.8.0 → 0.9.0`; `[lib] crate-type = ["cdylib", "rlib"]`; `[[bin]] required-features = ["treesitter"]`; new `wasm` feature; optional deps `wasm-bindgen`, `serde-wasm-bindgen`, `js-sys`.
- `src/lib.rs` — `extract`, `graph`, `index`, `lint`, `mcp`, `ontology`, `search`, `treesitter` all gated `#[cfg(not(target_arch = "wasm32"))]`. New `pub mod wasm` gated for wasm.
- `src/wasm/mod.rs` (new) — `parse`, `alias`, `prose`, `normalize_query`, `search_over_rows`. Row search delegates to `query::normalize_query` so multi-word queries decompose. No filesystem access from WASM — callers pass extracted rows.
- `tests/test_wasm_surface.rs` (new) — 8 native tests covering parse / alias / prose / normalize_query / search predicate shapes.
- `SPEC.md` — new section 13 "WASM" (feature flag, API surface, no-fs rule, wasm-pack command).
- `README.md` — WASM build section + JS snippet.

**Verification:**
- `rustup target add wasm32-unknown-unknown`.
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` — clean (66 crates).
- `cargo fmt` clean. `cargo clippy --all-targets --all-features -- -D warnings` — clean.
- `cargo test` — **132/132 pass** across 8 suites.
- `cargo build --release` + `cargo install --path .` — `~/.cargo/bin/tagpath` replaced 0.8.0 → 0.9.0.

**Commits:**
- tagpath `d30c630`: `feat: add tagpath wasm packaging (p4wm)` → pushed `btakita/tagpath:main`.
- agent-loop `77b5a6d`: `tagpath: bump submodule to 0.9.0 (p4wm)` → pushed `btakita/agent-loop:main`.

**Punted (intentional):**
- `wasm-pack` is not installed on this host. Cross-compile + native surface tests pass; no JS-side smoke ran. Follow-up: install `wasm-pack` and run `wasm-pack build --target nodejs --no-default-features --features wasm`.
- `alias` map keys use `Convention::Display` form (`camelCase`, `kebab-case`). Documented in SPEC 13.2.
- No crates.io / npm publish — those land with the release-hygiene queue item.

Auto-queue advances next to dynamic grammar loading.
### Re: queue auto — do [#p4wm] — opus-4-7


**Done.** WASM packaging shipped in tagpath 0.9.0; submodule pointer bumped + pushed.

**Side fixes this cycle (archive directive plumbing):**
- The `agent:done archive ...` comment was missing the `=` the parser needs (and the value-quoting style used elsewhere in the repo is unquoted) — corrected to `archive=tasks/software/tagpath.done.md` to match `tasks/software/tsift.md` / `tasks/software/skill-harness.md`.
- The archive target file did not yet exist — created `tasks/software/tagpath.done.md` with the standard `# Agent Doc Completed Work` header so the directive resolves.

**Changes (tagpath submodule, d30c630):**
- `Cargo.toml` — version `0.8.0 → 0.9.0`; `[lib] crate-type = ["cdylib", "rlib"]`; `[[bin]] required-features = ["treesitter"]`; new `wasm` feature; optional deps `wasm-bindgen`, `serde-wasm-bindgen`, `js-sys`.
- `src/lib.rs` — `extract`, `graph`, `index`, `lint`, `mcp`, `ontology`, `search`, `treesitter` all gated `#[cfg(not(target_arch = "wasm32"))]`. New `pub mod wasm` gated for wasm.
- `src/wasm/mod.rs` (new) — `parse`, `alias`, `prose`, `normalize_query`, `search_over_rows`. Row search delegates to `query::normalize_query`. No filesystem access from WASM — callers pass extracted rows.
- `tests/test_wasm_surface.rs` (new) — 8 native tests covering parse / alias / prose / normalize_query / search predicate shapes.
- `SPEC.md` — new section 13 "WASM"; `README.md` — WASM build section + JS snippet.

**Verification:** `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` clean (66 crates). `cargo fmt` + `cargo clippy --all-targets --all-features -- -D warnings` clean. `cargo test` **132/132 pass** across 8 suites. `cargo build --release` + `cargo install --path .` → `~/.cargo/bin/tagpath` replaced 0.8.0 → 0.9.0.

**Commits:**
- tagpath `d30c630`: `feat: add tagpath wasm packaging (p4wm)` → pushed.
- agent-loop `77b5a6d`: `tagpath: bump submodule to 0.9.0 (p4wm)` → pushed.

**Punted:** `wasm-pack` not on host — no JS smoke ran; `alias` keys use `Convention::Display` form (SPEC 13.2). No crates.io / npm publish — release-hygiene queue item owns those.

Auto-queue advances next to dynamic grammar loading.
<!-- agent:boundary:0296afe5 -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#p4ix]~
- ~do [#p4mp]~
- ~do [#p4wm]~
- do [#p4dy]
- do [#p4rl]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#p4dy] Add dynamic grammar loading behind an opt-in native feature: use `libloading`, register runtime tree-sitter grammars from configured directories, and cover version or symbol mismatch errors.
- [ ] [#p4rl] Finish Phase 4 release hygiene: update the Phase 4 checklist to mark the shipped ontology command complete, sync README/SPEC examples, run full verification, publish the next crate, and update this session status.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->
- 2026-05-09 [#ctxp] Add `context-pack` to `fixtures/tsift-token-savings.json` with raw refs, tagpath family rows, ontology_ref handles, and minimum savings thresholds; mirror the new case in tagpath + tsift fixture tests.
- 2026-05-09 [#cmpa] Add a tagpath library/CLI compression-report helper that accepts raw symbol rows and emits grouped family previews plus byte/token savings metrics for tsift.
- 2026-05-09 [#qont] Add query/ontology compactness fixture cases that cover normalize-query input and `.naming/tags` ontology_refs so tsift can keep context-pack refs as handles instead of prose.
- 2026-05-24 [#p4ix] Implement `tagpath index`: write `.naming/index.json` with schema version, config fingerprint, source hashes or mtimes, extracted identifier families, ontology refs, and CLI `index`/`index --check`; add tests for build, stale invalidation, and indexed search.
- 2026-05-24 [#p4mp] Add MCP integration on top of the index: expose parse, normalize-query, lint, search, ontology lookup, and indexed project query tools with JSON schemas plus local smoke tests.
<!-- /agent:done -->
