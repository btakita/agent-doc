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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-043453.md`*

Compacted content:
- Archived 3 response topic(s): next-steps; queue auto; queue auto
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#p6sklsym] [#p6lintbnd] — opus-4-7


Both queue items shipped.

#### #p6sklsym — symlink co-installed skills

Replaced three identical agent-loop skill copies with relative symlinks to the submodule canonicals:

- `.claude/skills/tagpath/SKILL.md` → `../../../src/tagpath/.claude/skills/tagpath/SKILL.md`
- `.claude/skills/email/SKILL.md` → `../../../src/corky/.claude/skills/email/SKILL.md`
- `.claude/skills/linkedin/SKILL.md` → `../../../src/corky/.claude/skills/linkedin/SKILL.md`

Skipped two skills whose parent/child copies diverge — recorded as new follow-up backlog items (NOT completed this cycle):

- `agent-doc` parent (v0.34.0, freshly installed) vs submodule (v0.33.16) — new id `p6sklsymagentdoc`
- `corky` parent vs submodule (different command listing format) — new id `p6sklsymcorky`

#### #p6lintbnd — relax agent-doc/malformed-boundary

Investigated the conflict. The lint rule rejected anything with non-hex characters in the boundary id, but `agent_doc::new_boundary_id_with_summary` emits the form `hex:slug` (e.g. `a0cfeb34:boundary-fix`). The colon and slug dashes failed the all-hex check.

Chose to **relax the rule** rather than change the binary's format — the summary suffix is a real product feature that aids human scanning, and dropping it would lose information.

Updated `src/lint/agent_doc.rs` to accept `hex(:slug)?` where the slug is non-empty lowercase alphanumeric + dashes (matching what the slugifier produces). Added six unit tests: plain hex, hex:slug, non-hex prefix (rejects), empty id (rejects), uppercase slug (rejects), trailing-colon empty slug (rejects). Updated SPEC.md §16.3 rule description accordingly.

#### Verification

- `cargo test` — 234 passed across 15 suites
- `cargo clippy --all-targets -- -D warnings` — clean (fixed one `unnecessary_map_or` lint to `is_none_or`)
- `cargo build --release && cargo install --path .` — installed at `~/.cargo/bin/tagpath`
- Smoke test on the live doc: `tagpath lint tasks/software/tagpath.md --dialect agent-doc` → "No agent-doc tag violations found"
- Smoke test on synthetic hex:slug form → passes
- Smoke test on `zzz` non-hex form → fires as expected with updated message
- tagpath CI run kicked off

#### Commits

- `src/tagpath`: `a89ba7e feat(lint): accept <hex>:<slug> boundary form (p6lintbnd)` — pushed to main
- `agent-loop`: `38cfc05f0 skills: symlink tagpath/email/linkedin to submodule canonicals (p6sklsym)` — pushed to main (includes the src/tagpath pointer bump)

Note on versioning: tagpath Cargo.toml is at 0.18.0 locally but crates.io tops out at 0.10.0 and there's no v0.18.0 tag — the publish gap is independent of this change and not part of this preset.
<!-- agent:boundary:821612ff -->
<!-- /agent:exchange -->
###

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#p6tsi]~
- ~do [#p6adf]~
- ~do [#p6mcr]~
- ~do [#p6adfstream]~
- do [#p6sklsym]
- do [#p6lintbnd]
- do [#p6adfmerge]
- do [#p6tsifull]
- do [#p6tspub]
- do [#p6skl]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#p6sklsymagentdoc] Reconcile and symlink agent-doc SKILL.md: agent-loop/.claude/skills/agent-doc/SKILL.md (v0.34.0, freshly installed) diverges from src/agent-doc/.claude/skills/agent-doc/SKILL.md (v0.33.16). Copy the newer agent-loop content into the submodule, commit src/agent-doc, then symlink the agent-loop copy. After this, agent-doc skill install --harness claude will write through the symlink to the submodule — adjust the release flow to commit the submodule pointer as part of the install loop.
- [ ] [#p6sklsymcorky] Reconcile and symlink corky SKILL.md: agent-loop/.claude/skills/corky/SKILL.md and src/corky/.claude/skills/corky/SKILL.md diverge substantively (different command listing format — terse one-liners vs prose paragraphs). Pick a canonical version (submodule is the natural home), reconcile, then symlink the agent-loop copy. Same pattern applies to corky's sub-skills if they drift in the future.
- [ ] [#p6adfmerge] Merge agent-doc feature branch 20260422/codex-agent-backend to main: the p6adf commit (845ddfd6) and prior p4*/p5* prep ride on this branch. agent-loop superproject already tracks feature-branch commits, but agent-doc main is behind and will need a sweep before any external consumer pulls.
- [ ] [#p6tsifull] Wire the remaining tsift consumer surfaces to the tagpath adapter: graph, explain, communities, path, federated-search. The adapter (src/tsift/src/tagpath_adapter.rs) and annotate helper are already public; each command needs TagpathSearchOpts plumbing + a per-row annotation pass + tagpath_handle field on StoredSymbol / StoredEdge / community report structs. Mechanical follow-up to p6tsi which only wired tsift search.
- [ ] [#p6tspub] Restore tsift crates.io publishability after p6tsi: tsift now depends on tagpath via path = ../tagpath. Replace with version = "0.17.1" (or whatever ships first) before any tsift release. Already gated by the sift-publish blocker so no immediate regression, but track here.
- [ ] [#p6ren] tagpath rename <old> <new>: cross-file, cross-convention identifier rename using the index. Walks every member of the matching family, rewrites in-place in each source convention (snake/camel/Pascal/kebab/UPPER/Ada). Foundation for the agent-loop intent-preserving refactor vision.
- [ ] [#p6meta] [recommended] agent-loop meta-index: aggregate per-submodule .naming/index.json files into a top-level handle registry so cross-repo searches resolve. New CLI: tagpath meta-index <workspace-root>. Useful once 2+ siblings adopt tagpath.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
