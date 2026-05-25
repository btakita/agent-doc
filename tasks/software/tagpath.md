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
v0.10.0 published to crates.io — Phase 4 complete (index, MCP, WASM, dyn-grammar)
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-012602.md`*

Compacted content:
- Archived 1 response topic(s): queue auto
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: queue auto — do [#p5wnp] — opus-4-7


**Done.** wasm-pack build pipeline shipped in tagpath 0.17.0; submodule pointer bumped + pushed. npm publish stays a manual gate per the session rule. One follow-up captured: missing root LICENSE file (p5lic) — easy fix, please address before you `npm publish`.

**What was delivered (build prep + verification, not publish):**

- `scripts/build-wasm.sh` — executable; runs `wasm-pack build` for three targets (bundler, nodejs, web), merges into one `pkg/`, generates `package.json` deterministically from `Cargo.toml`. Re-runnable.
- `pkg-smoke/smoke.mjs` — Node smoke test that imports the nodejs target and calls every binding: `parse`, `alias`, `prose`, `normalize_query`, `search_over_rows`. All 5 green; output: `smoke: all 5 bindings OK`.
- `.github/workflows/wasm-build.yml` — CI installs wasm-pack, runs the build script + smoke test, uploads `pkg/` as a workflow artifact. Proves the package keeps building without any publish gate.
- `.gitignore` — `pkg/`, `pkg-bundler/`, `pkg-nodejs/`, `pkg-web/`, `pkg-smoke/node_modules/` (generated artifacts).
- `SPEC.md` §13.5 — Publish flow subsection: build script, three targets, pkg layout, smoke test, manual npm publish gate.
- `README.md` — `npm install @btakita/tagpath-wasm` + Node + bundler usage snippets.

**pkg/ layout (total 692 KiB):**
```
pkg/
  package.json          # name=@btakita/tagpath-wasm, version=0.17.0, "exports" map
  README.md
  tagpath.d.ts          # merged type defs
  bundler/  ~248 KiB    # tagpath.js + tagpath_bg.{js,wasm} + .d.ts
  nodejs/   ~244 KiB    # CJS shim + package.json{type:commonjs} override
  web/      ~248 KiB    # ESM shim
```

**Build gotchas resolved (worth knowing):**
1. wasm-pack 0.15 errors on a second `--release` (release is default on stable).
2. wasm-pack misinterprets unrecognized args as cargo EXTRA_OPTIONS — keep its flags before the path arg.
3. `[lib].name = "tagpath"` produces `tagpath.{js,d.ts}` (not `tagpath_wasm.*`); script + smoke + SPEC updated.
4. nodejs target is CJS; merged `pkg/package.json` declares `type: module`. Fixed via a per-directory `pkg/nodejs/package.json` override.
5. `serde-wasm-bindgen` serializes Rust `BTreeMap` as JS `Map`, not plain object — smoke test reads via `Map.has()` with object fallback.

**Verification:**
- `cargo fmt` + clippy clean on default and `dyn-grammar`.
- `cargo test` — **213/213 pass**. `cargo test --features dyn-grammar` — **222/222 pass**.
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` — clean.
- `cargo build --release` + `cargo install --path . --force` — `~/.cargo/bin/tagpath` at 0.17.0.
- `./scripts/build-wasm.sh` — all three wasm-pack targets succeeded (each printed `📦 Your wasm pkg is ready to publish`).
- `node pkg-smoke/smoke.mjs` — exit 0, all 5 bindings verified.

**Commits:**
- tagpath `3717c88`: `feat: wasm-pack build for @btakita/tagpath-wasm (p5wnp)` → pushed.
- agent-loop `21c448e`: `tagpath: bump submodule to 0.17.0 (p5wnp)` → pushed.

**Manual gate (your step, when ready):**
```
cd src/tagpath && ./scripts/build-wasm.sh && node pkg-smoke/smoke.mjs
cd pkg && npm login && npm publish --access public   # scoped package needs --access public on first publish
```

**Follow-up captured:** p5lic — add a root `LICENSE-MIT` file. Both the wasm-pack tarball and any future crates.io publish currently ship without an actual license file even though `license = "MIT"` is set in `Cargo.toml`.

**Auto-queue drained.** Phase 5 prep complete: 7 cycles (p5tsi, p5inc, p5xfast, p5sln, p5wat, p5mcs, p5wnp), 7 minor releases (0.11.0 → 0.17.0), test baseline grew from 132 → 213 default / 222 with `dyn-grammar`, zero regressions.
<!-- agent:boundary:90df39c3 -->
<!-- /agent:exchange -->
###

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#p5lic] Add a root LICENSE-MIT file to the tagpath repo. Cargo.toml declares license = MIT but no LICENSE file exists, so the @btakita/tagpath-wasm npm tarball and crates.io published artifacts both ship without an actual license file. Trivial fix — paste the MIT text with btakita as the copyright holder.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
