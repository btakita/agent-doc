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
❯ Did [#p6tsi] implement the wiring into tsift or does tsift.md need an item added to its backlog?
### Re: queue auto — do [#p6adf] — opus-4-7


**Done.** Agent-doc finalize and write --commit gates shipped in agent-doc 0.34.0 (commit `845ddfd6` on feature branch). The `&lt;!-- agent:done archive PATH --&gt;` bug class now fails closed at the session-doc boundary, not deep inside the snapshot path. **Concrete proof:** a malformed fixture run through finalize errors with `bad.md:11:1 error: attribute archive on agent:done is missing =value [agent-doc/malformed-attr]` and a `try archive=<value>` fix hint — no commit happens.

**Integration choice: library call (Option A).** Added `tagpath = { path = "../tagpath", default-features = false }` to agent-doc's `Cargo.toml`. `tagpath::lint::agent_doc::lint_agent_doc` was already pub-exposed; no tagpath-side edit required. Library call beats subprocess: ~0ms gate overhead vs 5-50ms shell-out, type-safe error envelope.

**Four-mode resolution chain (CLI > frontmatter > config > default=warn):**
| Source | Knob |
|---|---|
| CLI | `--lint=off|warn|strict` on finalize / write |
| Frontmatter | `agent_doc_lint_dialect: off|warn|strict` |
| Workspace `.agent-doc/config.toml` | `[lint] dialect = "off|warn|strict"` |
| Default | `warn` (errors block; warnings surface on stderr) |

**Changes (agent-doc submodule, 845ddfd6):**
- `src/lint_gate.rs` (NEW, 363 lines) — gate logic + 10 unit tests.
- `src/main.rs` — module registration + `--lint` CLI flag parsing.
- `src/write.rs` — `lint_override` field + gate calls in pending_only + main paths.
- `src/frontmatter.rs` — `LintDialectMode` enum + `agent_doc_lint_dialect` field.
- `src/project_config.rs` — `LintConfig` struct + `[lint]` field.
- `Cargo.toml` — tagpath dep + bump `0.33.x → 0.34.0`; `Cargo.lock` refreshed; `pyproject.toml` synced.
- `VERSIONS.md` — changelog entry.
- `runbooks/commit.md` — new "Lint Gate" section.

**Verification:**
- `cargo fmt` + `cargo clippy --all-targets` — my files clean; pre-existing warnings in `session_check.rs`/`sync.rs`/`route.rs`/`agent/codex.rs` are unrelated.
- `cargo test --bin agent-doc lint_gate` — **10/10 pass**.
- `cargo test --bin agent-doc -- --test-threads=1` — **2369/0 pass** (12 parallel-run flakes are pre-existing PoisonError / harness_prompt_lock issues, pass individually and serially).
- `cargo build --release` + `cargo install --path .` — `~/.cargo/bin/agent-doc` at 0.34.0.

**Live smoke:**
- `agent-doc 0.34.0 preflight tasks/software/tagpath.md` → passes (this doc is clean).
- Malformed fixture (`&lt;!-- agent:done archive tasks/x.done.md --&gt;` missing `=`) → finalize fail-closes with the lint-gate error above. Same fixture + `--lint=off` → bypasses gate, caught later by the existing deeper guard (proves the new gate IS the boundary-level shortcut for the same bug class).

**Commits:**
- agent-doc `845ddfd6` on `20260422/codex-agent-backend`: `feat: finalize gate calls tagpath lint --dialect agent-doc (p6adf)` → pushed.
- agent-loop `2e4f411`: `phase 6: agent-doc finalize gate calls tagpath lint (p6adf)` → pushed (bumps agent-doc submodule).

**Two follow-ups captured:**
- `p6adfstream` — agent-doc Stream subcommand bypasses the gate (separate write loop in `src/stream.rs`). Mechanical mirror of the lint_gate call at stream commit time.
- `p6adfmerge` — feature branch `20260422/codex-agent-backend` carries this + prior Phase 4/5 prep. Needs a merge sweep to agent-doc main before any external consumer pulls.

Auto-queue advances next to MCP install helper (`p6mcr`).
### Re: queue auto — do [#p6skl] — opus-4-7


**Done.** `tagpath` skill refreshed for the 0.18.0 surface in both the canonical `src/tagpath/.claude/skills/tagpath/SKILL.md` and the agent-loop mirror. The skill description that Claude Code matches against ("Parse, search, lint, **index, and serve** tag-based identifiers...") now reflects what shipped through Phase 4 and Phase 5.

**Coverage added (was missing from the v0.5.0 skill):**
- `normalize-query`, `family`, `compression-report`, `ontology` (commands that already existed but weren't documented).
- `index` — full subcommand surface (`--update`, `--check`, `--force-full`, `--emit jsonl`, `--schema-version`) + the `fam:`/`mem:` handle semantics.
- `watch` — NDJSON event stream (`hello` / `ready` / `index_update` / `lint_finding` / `shutdown`), debounce, single-instance lock.
- `grammars list/check` (`dyn-grammar` opt-in feature).
- `mcp serve` + the `install` helper covering all 5 harnesses (`--list`, `--print`, `--apply`, `--uninstall`).
- `lint --dialect agent-doc|auto|identifier` + `--fs-checks` + `--rule`.
- `search --index` flag (instant index-backed search).
- Three new Use Cases: **cross-tool symbol graph** (tsift adapter), **session-doc tag enforcement** (agent-doc finalize gate), **MCP one-command install**.
- Key Concepts updated with handle derivation and SPEC §15 cross-link.

**Layout:** the file is 198 lines (was 130). Restructured Commands section so related surfaces sit together (index right next to watch, lint right next to extract).

**Changes (tagpath submodule, 1e6cdb9):**
- `.claude/skills/tagpath/SKILL.md` — full refresh (105 insertions / 37 deletions).

**Changes (agent-loop):**
- `.claude/skills/tagpath/SKILL.md` — mirrored from canonical copy.
- `src/tagpath` — bumped to `1e6cdb9` (docs-only; no version bump).

**Verification:**
- `diff -q` between the two copies → identical.
- Skill-tool listing in this session already picks up the new description ("Parse, search, lint, index, and serve...") — confirms Claude Code reload sees the change.
- No code change → no test run needed (this is docs-only).

**Commits:**
- tagpath `1e6cdb9`: `docs: refresh tagpath skill for 0.18.0 surface (p6skl)` → pushed.
- agent-loop `8f421a7`: `phase 6: refresh tagpath skill for 0.18.0 surface (p6skl)` → pushed.

**Follow-up captured:** `p6sklsym` — symlink the agent-loop mirror to the canonical copy so future updates only need one edit. Same pattern likely applies to `tsift`, `agent-doc`, and the other co-installed skills if they show similar duplication.

Auto-queue advances next to `tagpath rename` (`p6ren`).
<!-- agent:boundary:c069c099 -->
<!-- /agent:exchange -->
###

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#p6tsi]~
- ~do [#p6adf]~
- ~do [#p6mcr]~
- do [#p6adfstream]
- do [#p6adfmerge]
- do [#p6tsifull]
- do [#p6tspub]
- do [#p6skl]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#p6sklsym] Symlink the agent-loop .claude/skills/tagpath/SKILL.md to src/tagpath/.claude/skills/tagpath/SKILL.md so future updates only need one edit. Currently both copies are maintained by hand and drift is inevitable. Mirror the same approach for the other co-installed skills (tsift, agent-doc, etc.) if they exhibit the same duplication.
- [ ] [#p6adfstream] Instrument agent-doc Stream subcommand with the lint gate from p6adf: src/stream.rs has its own write loop that bypasses the finalize/write --commit gate path. Mirror the lint_gate call at stream commit time so streaming responses can't sneak malformed directives past the boundary.
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
