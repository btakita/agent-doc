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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-070330.md`*

Compacted content:
- Archived 1 response topic(s): do [#p6tsifullpath]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#p6tsifullexplain] — opus-4-7

Wired the tagpath consumer adapter into `tsift explain` and shipped as **tsift v0.1.52** (commit `0a71c45`, superproject pointer `015797d`).

#### What changed

- **CLI surface (src/tsift/src/main.rs):** `--no-tagpath` and `--tagpath-strict` flags added to `Commands::Explain`, plumbed into `cmd_explain_with_budget` as `tagpath_opts: TagpathSearchOpts`. Same flag spelling as `tsift search` / `tsift path`.
- **Index layer (src/tsift/src/index.rs):** `StoredSymbol` and `StoredEdge` gain a `tagpath_handle: Option<String>` field (serde-skipped when `None`). The shared index layer **never writes** the field — every `IndexDb` row constructor sets it to `None`. Only consumers (currently `cmd_explain`) populate it via the new annotation helpers.
- **Four annotation helpers (src/tsift/src/main.rs):**
  - `annotate_stored_symbols_with_tagpath` — definitions resolve directly from `(file, name)`.
  - `annotate_stored_edges_with_tagpath` with `EdgeSide::{Caller, Callee}` — caller rows annotate `(caller_file, caller_name)` directly; callee rows look up each `callee_name`'s definition via `db.symbol_info` first.
  - `resolve_community_member_handles` — looks up each member's definition file, returns parallel `Vec<Option<String>>` so `graph::Community` itself stays untouched.
  - `resolve_tagpath_handles_for_names` — shared name→file→handle batch lookup with dedup; used by the callee + community paths.
- **Annotation order:** `cmd_explain_with_budget` annotates against **absolute** paths before the optional `relativize_*` pass, so the adapter's `(path, name) → handle` resolution always sees real source paths.

#### Additive JSON change (recorded in VERSIONS.md 0.1.52)

`tsift explain --json` previously emitted `community` as `graph::Community` literally — `{id, members: ["alpha","beta"], modularity_contribution}`. Members are now objects: `{id, members: [{name, tagpath_handle?}, …], modularity_contribution}`. Existing consumers that read `member` as a string need a one-line update to read `member["name"]`. The standalone `tsift communities` command keeps the flat string-array shape until `#p6tsifullcommunities` lands.

Definitions/callers/callees gain the field inline on `StoredSymbol` / `StoredEdge` — serde-skipped when no handle, so default output is byte-identical for repos without `.naming.toml`.

#### Test coverage (src/tsift/tests/exit_code.rs)

- `explain_json_combines_definition_edges_and_community` — updated to read `member["name"]`, asserts every surface (definitions, callers, callees, community members) has no `tagpath_handle` field when no tagpath index is present.
- `explain_json_omits_handles_when_no_tagpath_flag_set` — builds a fresh tagpath index in the fixture, asserts `--no-tagpath` suppresses handles on all four surfaces.
- `explain_json_annotates_definitions_edges_and_community_when_index_is_fresh` — builds a fresh tagpath index, asserts every definition, caller, callee, and community member carries a `mem:` handle.

Reuses the `write_fresh_tagpath_index` helper introduced under `[#p6tsifullpath]`.

#### Verification

- `cargo test` ⇒ **855 passed** across 5 suites (exit_code 113, bin 687, conformance 31, perf_gate 21, scan_plan 4) — net +2 over the v0.1.51 baseline of 853 (3 new explain tests + 1 updated existing test).
- `cargo clippy --all-targets --no-deps -- -D warnings` ⇒ clean (2 pre-existing patch-not-used warnings only). Fixed one new `.into_iter()` redundancy clippy flagged in the community zip.
- `cargo build --release && cargo install --path .` ⇒ `tsift 0.1.52` live in `~/.cargo/bin/tsift`; `tsift explain --help` exposes the new flags.

#### Pending follow-up (queue advances next)

- `[#p6tsifullgraph]` — same template, but the backlog hint already notes `StoredEdge.tagpath_handle field if not already added by p6tsifullpath` ⇒ **already added by this commit**; `[#p6tsifullgraph]` only needs to wire `cmd_graph` to the existing annotation helpers and add CLI flags. Smaller blast radius than this turn.
- `[#p6tsifullcommunities]` — schema change: `graph::Community.members` likely promotes to `Vec<CommunityMember>` or gets a parallel handles field; same trade-off as `PathResult.path` faced under `[#p6tsifullpath]`.
- `[#p6tsifullfederated]` — most complex; adapter load must respect per-scope `project_root` across federated index DBs.
<!-- agent:boundary:5bcf718b -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#p6tsi]~
- ~do [#p6adf]~
- ~do [#p6mcr]~
- ~do [#p6adfstream]~
- ~do [#p6sklsym]~
- ~do [#p6lintbnd]~
- ~do [#p6sklsymagentdoc]~
- ~do [#p6sklsymcorky]~
- ~do [#p6adfmerge]~
- ~do [#p6adftestfix]~
- ~do [#p6sklsymadocrunbooks]~
- ~do [#p6adfcitagpath]~
- ~do [#p6tsifull]~
- ~do [#p6tsifullpath]~
- ~do [#p6tsifullexplain]~
- do [#p6tsifullgraph]
- do [#p6tsifullcommunities]
- do [#p6tsifullfederated]
- do [#p6tspub]
- do [#p6skl]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#p6tsifullgraph] Subitem of p6tsifull. Wire tagpath adapter into tsift graph command (src/tsift/src/main.rs ~line 1477). --callers / --callees output rows need tagpath_handle. Add StoredEdge tagpath_handle field if not already added by p6tsifullpath.
- [ ] [#p6tsifullcommunities] Subitem of p6tsifull. Wire tagpath adapter into tsift communities command (src/tsift/src/main.rs ~line 1515). Louvain community report struct needs a tagpath_handle field per member symbol. Smallest schema change after path.
- [ ] [#p6tsifullfederated] Subitem of p6tsifull. Wire tagpath adapter into tsift search --federated (workspace-scoped search across multiple scoped index.db files). Most complex of the five — federated search loops over scopes; adapter load must respect per-scope project_root. Defer until p6tsifullpath/explain/graph/communities establish the per-command template.
- [ ] [#p6tspub] Restore tsift crates.io publishability after p6tsi: tsift now depends on tagpath via path = ../tagpath. Replace with version = "0.17.1" (or whatever ships first) before any tsift release. Already gated by the sift-publish blocker so no immediate regression, but track here.
- [ ] [#p6ren] tagpath rename <old> <new>: cross-file, cross-convention identifier rename using the index. Walks every member of the matching family, rewrites in-place in each source convention (snake/camel/Pascal/kebab/UPPER/Ada). Foundation for the agent-loop intent-preserving refactor vision.
- [ ] [#p6meta] [recommended] agent-loop meta-index: aggregate per-submodule .naming/index.json files into a top-level handle registry so cross-repo searches resolve. New CLI: tagpath meta-index <workspace-root>. Useful once 2+ siblings adopt tagpath.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#p6tsifull] Wire the remaining tsift consumer surfaces to the tagpath adapter: graph, explain, communities, path, federated-search. The adapter (src/tsift/src/tagpath_adapter.rs) and annotate helper are already public; each command needs TagpathSearchOpts plumbing + a per-row annotation pass + tagpath_handle field on StoredSymbol / StoredEdge / community report structs. Mechanical follow-up to p6tsi which only wired tsift search.
- [/] [#p6adfmerge] Merge agent-doc feature branch 20260422/codex-agent-backend to main: the p6adf commit (845ddfd6) and prior p4*/p5* prep ride on this branch. agent-loop superproject already tracks feature-branch commits, but agent-doc main is behind and will need a sweep before any external consumer pulls.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
