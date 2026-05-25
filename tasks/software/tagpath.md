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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-072548.md`*

Compacted content:
- Archived 1 response topic(s): do [#p6tsifullgraph]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#p6tsifullcommunities] — opus-4-7

Wired the tagpath consumer adapter into `tsift communities` and shipped as **tsift v0.1.54** (commit `1d29d0a`, superproject pointer `9faf6de`). Fourth of the five symbol-graph consumer surfaces — only `[#p6tsifullfederated]` remains.

#### Decision: typed `CommunityMember`, not parallel handles array

Last cycle's response flagged this as the open shape decision: promote `Community.members` to a typed `Vec<CommunityMember>` (breaking JSON change for `tsift communities`) or carry a parallel `tagpath_handles: Vec<Option<String>>` field (additive). Picked typed for two reasons:

1. **Precedent under `[#p6tsifullpath]`** — `PathResult.path` made the same trade-off (flat `Vec<String>` → typed `Vec<PathNode>`) with a one-line consumer migration. Applying the same pattern here keeps the consumer-side mental model consistent across `tsift path` and `tsift communities`.
2. **Removes JSON wrapping in `cmd_explain`** — v0.1.52's `cmd_explain` carried hand-written JSON wrapping for `community.members` so the explain response could emit `{name, tagpath_handle}` objects. Promoting the struct lets the wrapping go away entirely; `community` now serializes natively from `cmd_explain` with byte-identical output.

#### What changed

- **`graph::CommunityMember`** (src/tsift/src/graph.rs) — new public struct `{ name: String, tagpath_handle: Option<String> }`. `Community.members: Vec<String>` → `Vec<CommunityMember>`. `detect_communities` now builds members via `CommunityMember::new`.
- **`Commands::Communities`** (src/tsift/src/main.rs) — gains `--no-tagpath` / `--tagpath-strict`, plumbed into `cmd_communities` as `tagpath_opts: TagpathSearchOpts`.
- **`cmd_communities`** resolves the canonical project root, calls `annotate_communities_with_tagpath` on `result.communities` **before** the `min_size` / `--limit` filter+truncate pass so even filtered-out communities carry handles for any future per-community surface that re-reads `result.communities` (forward-compat win for `[#p6tsifullfederated]`).
- **`annotate_communities_with_tagpath`** (new) batches all member names across all communities into one `resolve_tagpath_handles_for_names` call (dedup'd), then back-fills handles. Replaces the older `resolve_community_member_handles` helper that synthesized parallel `Vec<Option<String>>` output for the now-removed `cmd_explain` JSON wrapping.
- **`cmd_explain_with_budget`** annotates `comm_result.communities` once before locating the focus community; downstream JSON / human / compact / tabular paths just serialize the struct directly.
- **`compact_members`** helper updated to take `&[graph::CommunityMember]` and project to names internally.
- **Human output:** unchanged member layout; nodes with a resolved handle now print as `name  [mem:…]` (matches the `path` / `explain` conventions).

#### JSON contract — BREAKING (recorded in VERSIONS.md 0.1.54)

`tsift communities --json` previously emitted each member as a bare string: `community.members: ["alpha", "beta"]`. It now emits objects: `community.members: [{name, tagpath_handle?}, …]`. `tagpath_handle` is serde-skipped when `None`, so default output for projects without `.naming.toml` differs from v0.1.53 only by the wrapping `{}` around each member.

`tsift explain --json` already had this object shape for `community.members` since v0.1.52 (via manual JSON wrapping); that wrapping is now gone — the struct serializes natively. The wire shape is unchanged.

#### Test coverage (src/tsift/tests/exit_code.rs)

- `communities_json_reports_disconnected_clusters` — updated to read `member["name"]`, asserts no handles when no tagpath index is present.
- `communities_json_omits_handles_when_no_tagpath_flag_set` — fresh tagpath index, asserts `--no-tagpath` suppresses every member handle.
- `communities_json_annotates_members_when_index_is_fresh` — fresh tagpath index, asserts every member in every community carries a `mem:` handle.

#### Verification

- `cargo test` ⇒ **860 passed** across 5 suites (+2 over v0.1.53). Counts per suite: exit_code 118, bin 686, conformance 31, perf_gate 21, scan_plan 4.
- `cargo clippy --all-targets --no-deps -- -D warnings` ⇒ clean (2 pre-existing patch-not-used warnings only). Fixed two clippy-flagged collapsible `if let … { if … }` blocks during this cycle.
- `cargo build --release && cargo install --path .` ⇒ `tsift 0.1.54` live in `~/.cargo/bin/tsift`; `tsift communities --help` exposes the new flags.

#### Pending follow-up (queue advances next)

- `[#p6tsifullfederated]` — last symbol-graph surface. Federated search loops over scopes; adapter `try_load` must run per-scope against each `project_root` (each scope owns its own `.naming/index.json`). Most complex of the five because there's no single project root.
- After that: `[#p6tspub]` (publish prep — restore tsift crates.io publishability by replacing `path = "../tagpath"` with `version = "..."`).
❯ Can tagpath handle mixed name conventions? Such as javascript that is mostly in camelCase but has some snake_case variable or function names?
### Re: Can tagpath handle mixed name conventions? — opus-4-7

Short answer: **yes for parse/search/extract; mostly yes for lint with one rough edge.** Tagpath's design always picks one convention per identifier and uses that as a routing key, so a file that mixes camelCase and snake_case is handled identifier-by-identifier, not file-by-file.

#### How it works in practice

1. **Detection is per-identifier, not per-file.** `parser::detect_convention(name)` in `src/parser/mod.rs:64` runs against each identifier individually:
   - Contains `_` (and is not all-uppercase, and not Ada_Case) → `SnakeCase`
   - Contains `-` → `KebabCase`
   - Starts uppercase → `PascalCase`
   - Otherwise → `CamelCase`

   So `getUserId` and `user_id` sitting side-by-side in the same `.js` file get classified independently — no global "file convention" lock-in.

2. **Canonical tags are convention-agnostic.** `parse()` first splits on `_` (and `__`), then applies camelCase splitting per segment (`src/parser/mod.rs:107` `tokenize`). The tag list is lowercased before any matching. Net effect:
   - `getUserId` → tags `[get, user, id]`
   - `get_user_id` → tags `[get, user, id]`
   - `GetUserId` → tags `[get, user, id]`
   - `get-user-id` → tags `[get, user, id]`
   - Mixed like `createContext_auth` → tags `[create, context, auth]`

   That means `tagpath search` and the cross-convention alias machinery treat all four as equivalent — the JS file with both styles is fully searchable from any one canonical query.

3. **AST-aware extraction sorts identifiers by context.** With `lang-javascript` enabled, `extract::extract_from_path_with_mode(path, true)` walks the tree-sitter AST and tags each identifier as `Function | Variable | Type | Constant | Import | Parameter | Field | Other`. So even within a mixed file, the extractor knows *which* identifier is a function and which is a variable — that's what makes per-context lint rules work.

#### Lint side — the rough edge

`tagpath lint` enforces per-context conventions from `.naming.toml`. The bundled `lang/javascript.toml` preset uses:

```toml
[contexts.function]   convention = "camelCase"
[contexts.variable]   convention = "camelCase"
[contexts.constant]   convention = "UPPER_SNAKE_CASE"
[contexts.class]      convention = "PascalCase"
```

So a JS file with `function getUserName()` (function, camelCase ✓), `const MAX_RETRIES = 3` (constant, UPPER_SNAKE_CASE ✓), and `let user_count = 0` (variable, snake_case ✗ — expected camelCase) lints **cleanly on the first two and flags only the third**. That's the right answer for "JS file that is mostly camelCase with a few snake_case stragglers."

**The rough edge:** identifiers that themselves mix conventions within one name (e.g. `createUserProfile_v2`) are classified by `detect_convention` as a single convention. Because the rule is "contains `_` → snake-ish family," `createUserProfile_v2` is detected as `SnakeCase` even though it reads as "camelCase + version suffix." Tag extraction still works correctly (`[create, user, profile, v2]`), but the lint check `actual == expected` compares whole-identifier conventions, so this would lint-fail in a `camelCase`-only context with the suggested fix being `create_user_profile_v2`. Pure single-convention identifiers in a mixed file lint correctly; only mixed-within-one-identifier names hit this edge.

#### Practical guidance

For a JS codebase that's "mostly camelCase but with some snake_case":
- **Parse / alias / search** — works out of the box. Same canonical tags, cross-convention matching is the default behavior.
- **Extract** — works out of the box with `lang-javascript` enabled.
- **Lint** — works out of the box with the bundled `javascript` preset; flags exactly the snake_case-where-it-shouldn't-be cases. If you want stricter or laxer enforcement (e.g. allow snake_case for legacy variables but not for new functions), narrow it via `.naming.toml` overrides on `[contexts.variable]` / `[contexts.function]` per context; you can also `extends = ["javascript"]` and override only the contexts that need different rules.

If the codebase has lots of mixed-within-one-identifier names (`createUserProfile_v2` style) and you don't want those flagged, the cleanest workaround today is to either rename them or scope-exclude their paths in `.naming.toml`. A "tolerate mixed within an identifier" lint mode does not exist yet — it would need a new lint policy variant that compares tag-equivalence rather than convention-equivalence.

#### Backlog impact

Worth tracking as a small follow-up alongside the existing tagpath surface: a `lint.allow_mixed_within_identifier` policy (or per-context `[contexts.variable] tolerate_mixed = true`) would close the only real gap above. Low risk, single-file change in `lint/mod.rs:79`, mostly the policy plumbing. Adding to the recommended slot in the backlog as `[#mixconv]`. Not blocking the existing `p6tsifull*` rollout; `[#p6tsifullfederated]` is still the natural next item for `/agent-doc` continuation.
<!-- agent:boundary:51d025ad -->
<!-- /agent:exchange -->
###
<!--
-->

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
- ~do [#p6tsifullgraph]~
- ~do [#p6tsifullcommunities]~
- do [#p6tsifullfederated]
- do [#p6tspub]
- do [#p6skl]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#mixconv] [recommended] tagpath lint policy that tolerates mixed conventions within a single identifier (e.g. `createUserProfile_v2`). Today `parser::detect_convention` classifies any identifier containing `_` as snake-family, so the camelCase-plus-suffix shape lint-fails in a camelCase-only context with `create_user_profile_v2` as the suggested fix. Add a per-context `tolerate_mixed = true` or top-level `lint.allow_mixed_within_identifier` policy that compares tag-equivalence rather than convention-equivalence. Single-file change in `src/lint/mod.rs:79`. Tests: extend `tests/lint.rs` with mixed-within-identifier cases.
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
