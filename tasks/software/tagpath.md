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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-022241.md`*

Compacted content:
- Archived 2 response topic(s): queue auto; do [#p5lic]. #spec-test-build-install-commit-push
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
❯ What are #next-steps for tagpath to assist with tsift and agent-doc?
### Re: next-steps — opus-4-7


**Top of backlog: p6tsi** (tsift adoption of the tagpath consumer contract). That's the single item that converts the Phase 5 platform work into actual user value — without it, all the indexing / watch / handle plumbing sits unused.

Six items added to the backlog, priority order:

| Rank | ID | Hook | Why |
|------|-----|-----|-----|
| 1 | p6tsi | Tsift adopts tagpath as symbol-graph source | Phase 5 shipped the contract; this is the sibling-repo work that consumes it. Highest leverage by far — every tagpath feature we built lights up the moment tsift wires in. |
| 2 | p6adf | Agent-doc finalize calls `tagpath lint --dialect agent-doc` | The lint shipped in 0.14.0 only fires if invoked manually. Wiring it into finalize closes the `archive=PATH` bug class permanently — the malformed directive never reaches a snapshot. |
| 3 | p6mcr | `tagpath mcp install` helper | The 9 MCP tools exist but every consumer has to hand-craft a config block. A scaffolder for Claude Desktop / Codex / OpenCode / Cursor drops setup friction to one command. |
| 4 | p6skl | Update `.claude/skills/tagpath/SKILL.md` | Skill is stale (v0.5.0, pre-Phase-4 surface). Claude Code can't recommend features it doesn't know about. Refresh covers index / watch / lint / MCP / sidecar. |
| 5 | p6ren | `tagpath rename` cross-convention refactor | The agent-loop integration vision ("intent-preserving refactoring") needs this. Index + handles already do the heavy lifting; rename is a thin writer on top. |
| 6 | p6meta [recommended] | agent-loop meta-index across submodules | Once 2+ siblings adopt tagpath, a top-level handle registry makes cross-repo navigation work. Wait until p6tsi proves the contract before building this. |

**Framing — what's shipped vs what's needed:** Phases 4 and 5 built the entire tagpath platform (index, MCP, WASM, dyn-grammar, handles, JSONL, sidecar, incremental update, watch, agent-doc lint dialect, wasm-pack publish, LICENSE). Phase 6 should be **adoption-driven**: each item is a sibling-repo or consumer-facing integration that proves the platform pays off. p6tsi first; everything else can run in parallel after.
<!-- agent:boundary:783fcf9e -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#p6tsi]
- do [#p6adf]
- do [#p6mcr]
- do [#p6skl]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#p6tsi] Tsift adoption of tagpath: in src/tsift, replace bespoke symbol extraction with the tagpath consumer contract — read .naming/index.json (or subscribe to tagpath watch), cite fam:/mem: handles in envelope refs, fall back to live scan when index is stale/missing. Sibling-repo work; tagpath only maintains the SPEC §15 contract.
- [ ] [#p6adf] Agent-doc finalize gate: in src/agent-doc, call tagpath lint --dialect agent-doc on the session document during preflight or pre-write, refuse to commit on lint errors. Closes the loop on the archive=PATH bug class — it never reaches a snapshot. Sibling-repo work referencing tagpath.
- [ ] [#p6mcr] tagpath mcp install helper: generate Claude Desktop / Codex / OpenCode / Cursor MCP config JSON blocks pointing at the local tagpath mcp binary. Drops integration friction so users get the 9 tools without hand-editing config files.
- [ ] [#p6skl] Update .claude/skills/tagpath/SKILL.md — currently references v0.5.0 and pre-Phase-4 commands. Cover index / watch / lint --dialect agent-doc / MCP server / sidecar perf semantics so Claude Code can recommend the new surfaces without rediscovery.
- [ ] [#p6ren] tagpath rename <old> <new>: cross-file, cross-convention identifier rename using the index. Walks every member of the matching family, rewrites in-place in each source convention (snake/camel/Pascal/kebab/UPPER/Ada). Foundation for the agent-loop intent-preserving refactor vision.
- [ ] [#p6meta] [recommended] agent-loop meta-index: aggregate per-submodule .naming/index.json files into a top-level handle registry so cross-repo searches resolve. New CLI: tagpath meta-index <workspace-root>. Useful once 2+ siblings adopt tagpath.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
