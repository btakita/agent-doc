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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260530-041418.md`*

Compacted content:
- Archived 3 response topic(s): separate crates; next steps to split crates; do #tpcoreplan
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...

### Re: do #tpcoreextract — gpt-5

Completed `#tpcoreextract`.

Changed paths are in `src/tagpath`: created the `crates/tagpath-core` workspace crate at `0.12.0`, moved `parser`, `alias`, `family`, `prose`, `query`, and `compression` into it with their pure unit tests, and kept the root `tagpath` crate as the CLI/library facade through named re-exports from `tagpath_core`. Existing imports such as `tagpath::parser::parse`, `tagpath::alias::generate_aliases`, and `tagpath::compression::build_report` remain covered by facade tests.

I also updated `SPEC.md`, `README.md`, `VERSIONS.md`, `Cargo.lock`, `tests/test_core_split_plan.rs`, and `tagpath.iml`; removed the redundant root `license-file` metadata warning; committed and pushed `btakita/tagpath` at `00144ae` (`feat: extract tagpath core crate`); then committed and pushed the `agent-loop` submodule pointer at `4b3ad70b5`.

Verification passed: `cargo check`, `cargo test -p tagpath-core --no-default-features`, `cargo test`, `cargo test --workspace`, `cargo clippy`, `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm`, `cargo install --path . --force`, `cargo publish --dry-run --allow-dirty -p tagpath-core`, and root `make check` from `agent-loop`.

The facade publish dry-run was attempted with and without `--no-verify`, but Cargo stops before verification because `tagpath-core 0.12.0` is not yet present in the crates.io index. That matches the documented publish order: publish `tagpath-core` first, then rerun the `tagpath` facade dry-run/publish once the registry can resolve the core crate. Latest tagpath CI is green (`26674038403`), latest agent-doc CI is green (`26674156964`, `check` job only), and the root repo has no workflow named `CI`.

Escalation: none. Confidence: high. Next queue item remains `#tpcorefacade`.

### Re: do #tpcorefacade — gpt-5

Completed `#tpcorefacade`.

Changed paths:
- `src/tagpath/SPEC.md`
- `src/tagpath/VERSIONS.md`
- `src/tagpath/tests/lib_api.rs`
- `src/tagpath/tests/test_core_split_plan.rs`
- `src/tagpath/tests/test_facade.rs`
- `src/tagpath` submodule pointer in the superproject

What changed:
- Locked the root `tagpath` package as the compatibility facade: package name/version, `cdylib`/`rlib` crate type, binary name/path, `treesitter` binary gate, default feature set, and named optional feature wiring.
- Added facade API coverage for the old public `tagpath::...` paths that now re-export `tagpath-core` types.
- Added CLI facade regression tests for `parse`, `alias`, `family`, `prose`, and `normalize-query` through the `tagpath` binary.
- Updated the spec and changelog notes for the facade boundary.

Commits:
- `tagpath`: `06f853f test: lock tagpath facade compatibility`
- `agent-loop`: `a7714e617 chore: update tagpath submodule`

Verification:
- `cargo test --test lib_api --test test_core_split_plan --test test_facade` passed.
- `cargo test -p tagpath-core --no-default-features` passed.
- `cargo test` passed.
- `cargo test --workspace` passed.
- `cargo clippy` passed.
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` passed.
- `cargo install --path . --force` passed.
- `cargo publish --dry-run --allow-dirty -p tagpath-core` passed.
- `cargo publish --dry-run --allow-dirty -p tagpath` reached the expected publish-order blocker: crates.io does not yet have `tagpath-core 0.12.0`.
- Root `make check` passed.
- Tagpath GitHub Actions CI run `26674665718` passed.
- Root GitHub Actions workflow `Agent Doc` latest inspected run `26674487574` failed before runner startup: both `tmux-ci` and `check` jobs had zero steps and no logs, so this is classified as an external CI-start blocker rather than a code or tmux regression.

Escalation: none.

Next queue item remains `#tpcoreci`.

### Re: do #tpcoreci — gpt-5

Completed `#tpcoreci`.

Changed paths:
- `src/tagpath/.github/workflows/ci.yml`
- `src/tagpath/.github/workflows/release.yml`
- `src/tagpath/scripts/check-release.sh`
- `src/tagpath/tests/test_core_split_plan.rs`
- `src/tagpath/AGENTS.md`
- `src/tagpath/README.md`
- `src/tagpath/SPEC.md`
- `src/tagpath/VERSIONS.md`
- `src/tagpath` submodule pointer in the superproject

What changed:
- Expanded CI to run workspace clippy/tests, `tagpath-core` no-default tests, root facade library no-default tests, plain wasm target builds, and a crates.io publish-order smoke check.
- Added `scripts/check-release.sh`, which requires `tagpath-core` to package successfully and accepts the root `tagpath` dry-run only when it is blocked by the documented same-version `tagpath-core` crates.io ordering issue.
- Gated release artifact builds behind the same split-aware checks.
- Kept the existing wasm-pack workflow and added regression coverage proving the workflow/script/docs stay wired.
- Updated release documentation in AGENTS, README, SPEC, and VERSIONS.

Commits:
- `tagpath`: `8e27ce4 ci: add split release checks`
- `agent-loop`: `70ec5441a chore: update tagpath submodule`

Verification:
- `cargo test --test test_core_split_plan` passed.
- `scripts/check-release.sh` passed: `tagpath-core` dry-run passed; root `tagpath` dry-run reached the expected `tagpath-core` not-yet-published blocker.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo test --workspace` passed.
- `cargo test -p tagpath-core --no-default-features` passed.
- `cargo test -p tagpath --lib --no-default-features` passed.
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` passed.
- `cargo install --path . --force` passed.
- `scripts/build-wasm.sh` passed.
- `node pkg-smoke/smoke.mjs` passed.
- Tagpath GitHub Actions CI run `26674941830` passed with the expanded step list.
- Root `make check` was attempted and failed in the dirty `src/agent-doc` submodule on `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch`: the local uncommitted agent-doc code now records `abandoned` where the existing committed test expects `preflight_started`. I did not modify or commit that unrelated submodule state in this tagpath queue item.

Confidence: high for tagpath CI/release-check coverage; root-suite confidence is limited by the unrelated dirty agent-doc failure noted above.
Escalation: none.

Next queue item remains `#tpcoreadapters`.

### Re: do #tpcoreadapters — gpt-5
Completed `#tpcoreadapters` by recording the adapter-crate decision instead of adding more crates before `tagpath-core` has downstream mileage.

Changed paths:
- `src/tagpath/SPEC.md`
- `src/tagpath/README.md`
- `src/tagpath/VERSIONS.md`
- `src/tagpath/tests/test_core_split_plan.rs`
- root submodule pointer `src/tagpath`

What changed:
- Added SPEC section `18.7 Adapter crate decision`.
- Documented that no separate Rust crates named `tagpath-wasm`, `tagpath-mcp`, or `tagpath-project` are part of the first split.
- Kept `tagpath::wasm`, `tagpath::mcp`, and project/index/search/config surfaces in the root facade while `tagpath-core` stabilizes.
- Clarified that `@btakita/tagpath-wasm` remains the npm package produced by `scripts/build-wasm.sh`, not a Rust crate.
- Added regression coverage in `tests/test_core_split_plan.rs` so the adapter decision stays visible in SPEC, README, and release notes.

Commits:
- `src/tagpath`: `6a7131b docs: defer adapter crate split`
- root: `5bf334507 chore: update tagpath submodule`

Verification:
- `cargo test --test test_core_split_plan` passed after fixing a line-wrap-sensitive README assertion.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo test --workspace` passed.
- `cargo test -p tagpath-core --no-default-features` passed.
- `cargo test -p tagpath --lib --no-default-features` passed.
- `cargo build --target wasm32-unknown-unknown --no-default-features --features wasm` passed.
- `scripts/check-release.sh` passed; it accepted the expected facade dry-run blocker until `tagpath-core` is published to crates.io first.
- `scripts/build-wasm.sh` passed.
- `node pkg-smoke/smoke.mjs` passed.
- `cargo install --path . --force` passed.
- Tagpath GitHub Actions passed for `6a7131b`: `CI` run `26675167995`, `WASM build` run `26675167980`.

Root verification:
- `make check` still fails in unrelated dirty `src/agent-doc`, on `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch`: actual `abandoned`, expected `preflight_started`. I did not modify or stage `src/agent-doc`.
- Root has no workflow named `CI`.
- Latest root `Agent Doc` run `26675003816` is still an external CI-start blocker: both `check` and `tmux-ci` jobs failed with empty step lists.

Confidence: high for the tagpath adapter decision and submodule pointer. The remaining red evidence is outside this tagpath change.

Escalation: none.
❯ Should we split out the 3 additional crates?

### Re: adapter crates — gpt-5
No, not yet. I would keep the v0.12 split to `tagpath-core` plus the existing root `tagpath` facade.

The three proposed adapter crates are plausible later, but splitting them now would add release and compatibility surface before we have evidence that the core boundary is stable:

- `tagpath-wasm`: keep this as `tagpath::wasm` in the facade and publish `@btakita/tagpath-wasm` from `scripts/build-wasm.sh`. A Rust crate only makes sense if Rust-side wasm consumers need to depend on the adapter without pulling the facade package shape.
- `tagpath-mcp`: keep this in the facade because MCP currently couples JSON-RPC stdio, harness config install, project config, indexed search, lint, and filesystem behavior. Splitting it now would mostly move facade dependencies into a new crate without simplifying the API.
- `tagpath-project`: defer until project/index/search/config/ontology types settle. This might become the best second split, but only if downstream consumers need a stable project model without CLI/MCP/wasm packaging.

A good split trigger would be: `tagpath-core` 0.12.x is published and used downstream, an adapter has an independent consumer, the dependency reduction is measurable, and the adapter API can be versioned without re-export churn. If that happens, I would evaluate `tagpath-project` first, then `tagpath-mcp` on top of it, and leave `tagpath-wasm` as npm packaging unless Rust consumers specifically ask for a crate.

Changed paths: none.

Commands run:
- `tsift status`
- `agent-doc --version`
- `tsift status --fix`
- `tsift index --workspace .`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tagpath.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tagpath.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tagpath.md`

Verification: prompt-only decision; no code or tests changed.

Confidence: high.

Escalation: none.
<!-- agent:boundary:547f654c -->
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
