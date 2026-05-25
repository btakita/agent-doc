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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-084637.md`*

Compacted content:
- Archived 1 response topic(s): do [#p6tsifullfederated]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#tpstdiag] — opus-4-7

Wired in tsift 0.1.58 (`9148779` in tsift, superproject pointer bumped).

#### Fix
New helper `inject_tagpath_stale_into_json(value, stale, reason)` adds top-level `tagpath_index_stale: true` + `tagpath_stale_reason: <reason>` to any JSON `Object` when the tagpath adapter reported `stale=true` for any per-row annotation pass. Applied at every JSON emission site across the five commands:

- **`cmd_path`** — injects after the existing `tagpath_diag` capture.
- **`cmd_explain_with_budget`** — folds `def_diag`, `caller_diag`, `callee_diag`, and `comm_diag` into a combined `tagpath_stale` / `tagpath_stale_reason` pair; both budget-mode and non-budget JSON paths inject. The community-stale eprintln no longer skips the JSON path silently.
- **`cmd_graph`** — switches the local closure state to a `std::cell::RefCell<(bool, bool, Option<String>)>` tracker so the same diagnostic data is visible to all three JSON emit paths (callers-only, callees-only, combined) without violating the closure's mutable-borrow contract.
- **`cmd_communities`** — captures the `annotate_communities_with_tagpath` diagnostic and injects.
- **`cmd_search_with_budget`** — both budget-mode and plain-mode JSON paths convert the typed report to `serde_json::Value`, inject, and pass through `print_json_or_envelope`. Federated paths use the merged diagnostic from `federated_symbol_search` (closes the loop with `#p6tsifullfederated`).

The existing stderr `tagpath_index_stale: …` log line is preserved unchanged at every site. `--no-tagpath` suppresses both the stderr line and the new JSON fields because the diagnostic is gated through `tagpath_diag.stale && !tagpath_opts.no_tagpath`.

#### Test
`tests/exit_code.rs::json_surfaces_tagpath_stale_diagnostic_when_index_is_stale` — builds a fresh tagpath index, mutates `main.rs` to trigger `source_modified` staleness, then asserts the new fields appear on:

- `tsift path caller alpha --json`
- `tsift communities --json`
- `tsift graph alpha --json`
- `tsift explain alpha --json`
- `tsift search alpha --json`

…and that `--no-tagpath` suppresses the fields on the path command. Reason strings carry `source_modified` / `source_added` per the existing tagpath stale-reason vocabulary.

#### Verification
- `cargo test --test exit_code json_surfaces_tagpath_stale_diagnostic_when_index_is_stale` — green
- `make check` — green (full clippy + suite)
- `cargo install --path .` — installed `tsift 0.1.58` to `~/.cargo/bin/`

#### Files
- `src/tsift/src/main.rs` — `inject_tagpath_stale_into_json` helper; injection at 7 JSON emit sites across the five commands; `cmd_graph` closure restructured around `RefCell`
- `src/tsift/tests/exit_code.rs` — `json_surfaces_tagpath_stale_diagnostic_when_index_is_stale`
- `src/tsift/SPEC.md` — stale-diagnostic clause in §`tagpath_handle semantics`
- `src/tsift/VERSIONS.md` — 0.1.58 entry
- `src/tsift/Cargo.toml`, `Cargo.lock` — version → 0.1.58
- Superproject `src/tsift` submodule pointer bumped

Structured consumers (tsift envelope, agent-doc, dispatched job packets) can now branch on `response["tagpath_index_stale"]` instead of having to scrape stderr or rerun with `--tagpath-strict` to find out.
### Re: Add #next-steps to fix remaining lossy data — opus-4-7

Re-read the five loss modes from the prior turn and the current backlog to find what is still uncovered.

| Mode | Status before this turn |
|------|-------------------------|
| 1. Name-collision wrong-handle | Tracked as `[#commhx]` |
| 2. Cross-scope blind spot | Tracked as `[#p6tsifullfederated]` |
| 3. Index drift (recall, bounded) | Diagnosed via `tagpath_index_stale: true` on **stderr only** — consumers reading the JSON envelope never see it. Real gap. |
| 4. Walker divergence (tsift vs tagpath source walks) | **Not tracked anywhere.** Real gap. |
| 5. External references | By design, no work needed. |

Added two new items to close the remaining lossy surface, then reordered the backlog so the lossy-fix path comes first (correctness → recall → diagnostic → lint policy → feature work).

#### New backlog items

- `[#tpstdiag]` — Surface the tagpath stale-index signal into the structured `tsift --envelope` / `--json` output. Today `annotate_*_with_tagpath` only emits `tagpath_index_stale: true (reason=…)` on stderr; tsift / agent-doc consumers reading the JSON response cannot see the diagnostic and therefore cannot decide to re-run with `--tagpath-strict` or trigger a rebuild. Add a top-level `tagpath_index_stale: true` + `tagpath_stale_reason` field to the envelope when any of the helpers reported `stale=true`. Touches `cmd_search`, `cmd_path`, `cmd_explain`, `cmd_graph`, `cmd_communities`, and the budget-mode reports. Tests: extend `tests/exit_code.rs` with a stale-index fixture (mutate a source file after `write_fresh_tagpath_index`, assert the JSON carries the new field).

- `[#tpaudit]` — Reconcile tsift's tree-sitter source walk against tagpath's `.naming.toml` walk and surface the diff. Today silent recall loss happens when tagpath's `[exclude]` or `extends` skip files / languages that tsift indexes; the tsift symbols in those files get no handle even with a fresh tagpath index. New CLI: `tsift audit tagpath [--path .] [--scope <name>] [--json]` that reports (a) tsift symbols whose definition file is outside the tagpath source set, (b) tagpath families whose source files no longer exist in tsift's `index.db`, and (c) the policy hints (excluded paths, language coverage gaps). Tests: extend `tests/exit_code.rs` with a fixture where `.naming.toml` excludes a path tsift still indexes.

#### Reordered backlog (lossy-fix priority first)

Reorder via `--pending-reorder commhx,p6tsifullfederated,tpstdiag,tpaudit,mixconv,p6tspub,p6ren,p6meta` so the order is:

1. `[#commhx]` — Mode 1, correctness loss (wrong handle). Highest priority because it is the only mode that can emit incorrect data; the rest are conservative no-handle outcomes.
2. `[#p6tsifullfederated]` — Mode 2, recall loss across workspace scopes. Completes the symbol-graph triplet rollout, blocks meaningful workspace use.
3. `[#tpstdiag]` — Mode 3 diagnostic gap. Cheap to add and unblocks future automation (e.g. agent-doc retry/rebuild flow on stale).
4. `[#tpaudit]` — Mode 4, walker divergence audit. New surface; lower urgency than the other three because it is user-config drift, not a code bug.
5. `[#mixconv]` (recommended) — lint policy for mixed conventions within a single identifier. Quality-of-life, not lossy.
6. `[#p6tspub]` — administrative (restore crates.io publishability before any tsift release). Already gated by the sift-publish blocker, so not time-critical.
7. `[#p6ren]` — feature work (`tagpath rename`). Not lossy, not blocking.
8. `[#p6meta]` (recommended) — feature work (meta-index). Cross-repo nice-to-have.

#### Top of backlog

`[#commhx]`
<!-- agent:boundary:f9232d09 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#commhx]~
- ~do [#p6tsifullfederated]~
- ~do [#tpstdiag]~
- do [#tpaudit]
- do [#mixconv]
- do [#p6tspub]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tpaudit] Reconcile tsift tree-sitter source walk against tagpath `.naming.toml` walk and surface the diff. Today silent recall loss happens when tagpath `[exclude]` or `extends` skip files / languages that tsift indexes; the tsift symbols in those files get no handle even with a fresh tagpath index. New CLI: `tsift audit tagpath [--path .] [--scope <name>] [--json]` that reports (a) tsift symbols whose definition file is outside the tagpath source set, (b) tagpath families whose source files no longer exist in tsift index.db, and (c) the policy hints (excluded paths, language coverage gaps). Tests: extend `tests/exit_code.rs` with a fixture where `.naming.toml` excludes a path tsift still indexes.
- [ ] [#mixconv] [recommended] tagpath lint policy that tolerates mixed conventions within a single identifier (e.g. `createUserProfile_v2`). Today `parser::detect_convention` classifies any identifier containing `_` as snake-family, so the camelCase-plus-suffix shape lint-fails in a camelCase-only context with `create_user_profile_v2` as the suggested fix. Add a per-context `tolerate_mixed = true` or top-level `lint.allow_mixed_within_identifier` policy that compares tag-equivalence rather than convention-equivalence. Single-file change in `src/lint/mod.rs:79`. Tests: extend `tests/lint.rs` with mixed-within-identifier cases.
- [ ] [#p6tspub] Restore tsift crates.io publishability after p6tsi: tsift now depends on tagpath via path = ../tagpath. Replace with version = "0.17.1" (or whatever ships first) before any tsift release. Already gated by the sift-publish blocker so no immediate regression, but track here.
- [ ] [#p6ren] tagpath rename <old> <new>: cross-file, cross-convention identifier rename using the index. Walks every member of the matching family, rewrites in-place in each source convention (snake/camel/Pascal/kebab/UPPER/Ada). Foundation for the agent-loop intent-preserving refactor vision.
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
