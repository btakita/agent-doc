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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260524-232950.md`*

Compacted content:
- Archived 1 response topic(s): queue auto
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
❯ What are #next-steps for tagpath to support tsift and agent-doc?
### Re: next-steps — opus-4-7


**Top of backlog: p5tsi** (tsift integration via `.naming/index.json` as a first-class symbol-graph adapter).

Six items added to the backlog in priority order — the first three matter most because they unblock real tsift and agent-doc workflows; the bottom three are enablers / nice-to-haves.

| Rank | ID | Hook | Why now |
|------|-----|-----|---------|
| 1 | p5tsi | tsift integration: `.naming/index.json` becomes tsift's symbol-graph adapter | Tsift currently re-extracts symbols. Tagpath 0.7.0+ has a richer family/ontology index. Wiring tsift envelopes to cite tagpath family handles gives stable refs and removes duplicated extraction. |
| 2 | p5inc | Incremental indexing (`tagpath index --update`) | Full rebuilds block any "freshness-gated" caller. Tsift refuses stale envelopes; agent-doc cycles need sub-second index repair. Today the index is rebuild-only. |
| 3 | p5sln | Session-doc tag lint | This session hit the `agent:done archive PATH` bug because the directive parser is hand-rolled. Tagpath lint already knows tag families — a dialect that validates agent-doc components closes the loop and would have failed-closed earlier. |
| 4 | p5wat | `tagpath watch` | Long-running watcher emitting NDJSON index events. Foundation for live tsift projection and agent-doc editor-side validation. |
| 5 | p5mcs | MCP helpers `family_by_path`, `lint_session_doc`, `index_handle` | Lets tsift and agent-doc speak to tagpath over the stdio MCP server shipped in 0.8.0 without bespoke wrappers. |
| 6 | p5wnp [recommended] | Publish wasm-pack tarball to npm | Wasm surface shipped in 0.9.0 but no npm artifact. Required before any JS-side agent-doc / tsift surface can pick it up. |

**Phase 5 framing:** items 1–3 are a coherent "indexing-as-a-platform" arc. Combined deliverable: a tagpath-backed symbol/tag graph that tsift consumes for citations and agent-doc consumes for tag validation. Recommend grouping under the `#spec-test-build-install-commit-push` preset once you're ready to execute.
<!-- agent:boundary:3693a094 -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#p5tsi]~
- ~do [#p5inc]~
- do [#p5sln]
- do [#p5wat]
- do [#p5mcs]
- do [#p5wnp]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#p5xfast] Hit the 10x no-op perf bar for tagpath index --update on large repos: today on a 1000-file synthetic tree no-op runs at ~4.7x vs full rebuild and one-file-edit at ~1.4x; ceiling is full JSON (de)serialize of the on-disk index. Options: stream-parse only the sources array of the existing index, or add a binary sidecar cache. Either is scope beyond p5inc — track here so tsift's polling cadence isn't blocked.
- [ ] [#p5sln] Session-document tag lint: extend tagpath lint with an agent-doc dialect that validates agent:exchange / agent:backlog / agent:done / patch:* tags, catches malformed directives like the agent:done archive=PATH bug hit this session, and refuses to write a finalize until they parse cleanly.
- [ ] [#p5wat] tagpath watch: long-running watcher that emits index update events on file changes (newline-delimited JSON), suitable for tsift's incremental graph projection and agent-doc tag validators that need to react inside a cycle.
- [ ] [#p5mcs] MCP convention helpers for tsift and agent-doc: add tagpath mcp tools family_by_path, lint_session_doc, and index_handle so both upstreams can call tagpath without bespoke shell wrappers.
- [ ] [#p5wnp] [recommended] Publish the wasm-pack tarball to npm (@btakita/tagpath-wasm) so agent-doc front-end surfaces and tsift JS adapters can call parse / alias / normalize_query / search_over_rows in-browser without a native dep.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tagpath.done.md -->

<!-- completed work archived in tasks/software/tagpath.done.md -->
<!-- /agent:done -->
