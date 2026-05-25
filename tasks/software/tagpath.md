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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-060001.md`*

Compacted content:
❯ - Archived 2 response topic(s): do [#p6adfcitagpath]; Why are we not auto-looping?
❯ - Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#p6adftestfix] — opus-4-7

Shipped. Picked option (b) — add legacy alias acceptance to tagpath's agent-doc lint dialect. Sweeping ~50+ references to legacy component names across agent-doc src+tests was the wrong direction.

#### Change

- `src/lint/agent_doc.rs`: new `LEGACY_COMPONENTS` const (`agent:pending`, `agent:pending-done`, `agent:backlog-done`) — the exact migration-input names from agent-doc's `src/migrate.rs`. Lint now accepts them silently; the canonical names remain in the fix-hint so the message still nudges toward `agent:backlog` / `agent:done`.
- Removed a duplicate `is_known_component` function I introduced mid-edit.
- 4 new tests: each legacy form accepted, plus a regression test confirming a truly unknown component still fails.

#### Release

- `tagpath v0.11.1` published to crates.io
- GitHub Release: <https://github.com/btakita/tagpath/releases/tag/v0.11.1> with x86_64-linux binary
- VERSIONS.md updated with a focused v0.11.1 entry
- `~/.cargo/bin/tagpath` reinstalled

#### Downstream

- `src/agent-doc/Cargo.toml`: `version = "0.11"` → `"0.11.1"`. Cargo.lock regenerated in the same commit.
- agent-loop superproject pointer bumped to pick up the new agent-doc submodule HEAD.

#### CI

New run `26386002572` on `20260422/codex-agent-backend` kicked off on the push and is in progress. Backgrounded a watcher to alert on completion. Once it lands green, `#p6adfmerge` is fully unblocked.

#### Verification

- `cargo test` in tagpath: **238 passed** (up from 234, +4 new legacy-component tests)
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo build --lib` in agent-doc with the new tagpath dep: clean

#### Commits

- `src/tagpath`: `8511dfb release: v0.11.1 — accept legacy agent-doc components in lint (p6adftestfix)` + tag `v0.11.1`
- `src/agent-doc` (`20260422/codex-agent-backend`): `9ed0bc17 deps: bump tagpath 0.11 → 0.11.1 (p6adftestfix)`
- `agent-loop` (`main`): `1f24e097b agent-doc: bump submodule for tagpath 0.11.1 (p6adftestfix)`
### Re: do [#p6sklsymadocrunbooks] — opus-4-7


Decision: **option (b) — leave install owning the parent copy; do not symlink runbooks.**

#### Why

The runbooks under `.claude/skills/agent-doc/runbooks/` are install-generated artifacts, not hand-maintained content. The source of truth is `src/agent-doc/runbooks/` (committed); the binary embeds those files at compile time and writes them out on `agent-doc skill install --harness claude`. They are a projection, not a duplication — same status as `target/` or a generated lockfile.

Symlinking would actually make the topology *worse*:

1. We'd add a third copy in `src/agent-doc/.claude/skills/agent-doc/runbooks/` that has to stay in sync with both the source dir and the binary's embedded content.
2. Future `agent-doc skill install` runs in agent-loop would write through the symlink into the submodule, dirtying it on every install — same problem we noted in `p6sklsymagentdoc` but now applied to 14+ files instead of one.
3. The install boundary already owns this directory; introducing a symlink would add a second owner.

#### What surfaced instead

The investigation found a real but unrelated install bug: `agent-doc skill install` does not reconcile the target directory. agent-loop currently has **21 installed runbooks vs 14 source files** — 8 stale extras from older versions still in place, AND `manual-job-packets.md` (new in source) never written because the install saw SKILL.md as already-up-to-date and skipped the runbooks pass.

Captured as `#p6skillinstallreconcile`. That is the right place to fix the drift — not by symlinking, but by making install delete-then-write the runbooks subtree (or compute the canonical set and reap stragglers).

#### Aligning the three "symlink the duplicated skill" follow-ups

| Item | Resolution | Notes |
|---|---|---|
| `p6sklsym` (tagpath, email, linkedin) | Symlinked | Identical content, hand-maintained surface, single source of truth in submodule |
| `p6sklsymagentdoc` (agent-doc SKILL.md) | Symlinked | Install regenerates the file each cycle; symlink routes the regen into the submodule |
| `p6sklsymcorky` (corky SKILL.md) | Symlinked | Hand-maintained surface, submodule was newer |
| `p6sklsymadocrunbooks` (agent-doc runbooks) | **Not symlinked** | Install-generated subtree; symlink would multiply the sync surface |

Pattern: symlink hand-maintained surfaces and single files; leave install-generated subtrees to the install workflow and fix install bugs at the source.
❯ should [#p6skillinstallreconcile] be added to the backlog of agent-doc-bugs2.md?
### Re: should [#p6skillinstallreconcile] be added to the backlog of agent-doc-bugs2.md? — opus-4-7 (HEAD)


Yes — you're right, that's its proper home. Moved.

#### What I did

1. Created the plan file: `tasks/agent-doc/plan-skill-install-runbooks-reconcile.md` (concrete diagnosis with the 21-vs-14 evidence, fix scope, verification cases).
2. Added `[#p6skillinstallreconcile]` to `tasks/agent-doc/agent-doc-bugs2.md` backlog via `--pending-add-to`, with the plan path referenced inline (matches the convention used by `#ccloopguard`, `#jbrsrbusysw`, etc.).
3. Marked the entry done on `tagpath.md` here — "done" in the sense of "tracked elsewhere now, not pending in this doc". The work itself is open; the bookkeeping moved.

The frontmatter preset `#agent-doc-bug` already documents this routing convention ("Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md") — I should have caught it last cycle when I first captured the item here. Noted for next time.
<!-- agent:boundary:c1fdbacb -->
<!-- /agent:exchange -->
###

<!-- agent:queue -->
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
- do [#p6tsifull]
- do [#p6tspub]
- do [#p6skl]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#p6tsifull] Wire the remaining tsift consumer surfaces to the tagpath adapter: graph, explain, communities, path, federated-search. The adapter (src/tsift/src/tagpath_adapter.rs) and annotate helper are already public; each command needs TagpathSearchOpts plumbing + a per-row annotation pass + tagpath_handle field on StoredSymbol / StoredEdge / community report structs. Mechanical follow-up to p6tsi which only wired tsift search.
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
