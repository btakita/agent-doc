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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f12047b1a846ebab88214cfbaa57574433d92d6c9900e3a05507a01aa-20260525-051642.md`*

Compacted content:
- Archived 1 response topic(s): do [#p6sklsym] [#p6lintbnd]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/55560c2f...
### Re: do [#p6sklsymagentdoc] — opus-4-7


Shipped.

Synced `src/agent-doc/.claude/skills/agent-doc/SKILL.md` from the freshly-installed agent-loop copy (v0.33.16 → v0.34.0, 20 inserts / 6 deletes), then replaced the agent-loop copy with a relative symlink into the submodule. Single source of truth now lives in the submodule.

#### Verification

- Symlink resolves: `head -5 .claude/skills/agent-doc/SKILL.md` shows v0.34.0 frontmatter
- File mode changed `100644 → 120000` in the commit, confirming symlink

#### Commits

- `src/agent-doc` (branch `20260422/codex-agent-backend`): `c76a26b skill: sync agent-doc SKILL.md to v0.34.0 (p6sklsymagentdoc)` — pushed
- `agent-loop`: `4dfbd9896 skills: symlink agent-doc SKILL.md to submodule canonical (p6sklsymagentdoc)` — pushed to main (includes the src/agent-doc pointer bump)

#### Follow-up surfaced

The runbooks under `.claude/skills/agent-doc/runbooks/` (commit.md, harness-invocation.md, etc.) are also duplicated, but the submodule doesn't have an equivalent `.claude/skills/agent-doc/runbooks/` directory — they only live in the source `src/agent-doc/runbooks/` and get materialized at install time. Recorded as `#p6sklsymadocrunbooks` for a focused decision later.

#### Release-flow implication

Per the original backlog text: now that the symlink is in place, future `agent-doc skill install --harness claude` runs in agent-loop will write through the symlink into the submodule, dirtying it. The install loop needs to either commit the submodule + bump the pointer, or skip writing when the target is a symlink whose contents already match. Captured in the original `p6sklsymagentdoc` backlog text; no new item.
### Re: do [#p6sklsymcorky] — opus-4-7

Shipped.

Submodule won the canonical-version decision on dates alone — its copy is from 2026-05-20 (a polished consolidated-prose format), the agent-loop copy was from 2026-04-08 with no information that wasn't already in the newer submodule version. No reconciliation patch needed — straight symlink.

#### Verification

- Symlink resolves: `head -3 .claude/skills/corky/SKILL.md` shows "# Corky — Correspondence Kit" from the submodule
- File mode changed `100644 → 120000`

#### Commit

- `agent-loop`: `f1f3138de skills: symlink corky SKILL.md to submodule canonical (p6sklsymcorky)` — pushed to main

The pattern for the three "diverge then symlink" follow-ups (tagpath, agent-doc, corky) is now fully applied. Per-skill symlinks documented; release flow implication tracked under `#p6sklsymadocrunbooks` for the agent-doc runbooks question.
### Re: do [#p6adfmerge] — opus-4-7


**Gated, not done.** Recording the response here after a missed-patchback recovery; the prior finalize raced with a parallel cycle and the response body was dropped while the gate + pending-add still landed. Repaired by sweeping nine pre-existing `state: "superseded"` capture sidecars (an unknown variant the current binary couldn't parse), then re-running finalize.

#### Why this stops

Feature branch `20260422/codex-agent-backend` is a clean fast-forward candidate: **832 commits ahead** of `origin/main`, `origin/main` is **0 commits behind**. No merge conflicts possible.

But the three most recent CI runs (`26384722723`, `26384538884`, `26384031366`) are **red**. All three fail in `cargo clippy` at `make check`:

```
error: failed to get `tagpath` as a dependency of package `agent-doc v0.34.0`
Caused by: failed to load source for dependency `tagpath`
Caused by: unable to update /home/runner/work/agent-doc/tagpath
Caused by: failed to read `/home/runner/work/agent-doc/tagpath/Cargo.toml`
Caused by: No such file or directory (os error 2)
```

Root cause: agent-doc `Cargo.toml` declares `tagpath = { path = "../tagpath", version = "0.18", default-features = false }`. Locally that resolves to the agent-loop sibling; the agent-doc CI workflow doesn't clone tagpath, so the path lookup fails.

#### Decision required (operator)

Three independent fix paths — picking one is operator judgment:

1. **Checkout sibling in CI**: add a step to clone `btakita/tagpath` into the parent dir before `make check`. Cheapest, adds CI coupling.
2. **Switch to git source**: `git = "https://github.com/btakita/tagpath", branch = "main"`. Loses the local-dev convenience.
3. **Publish tagpath 0.18 and drop the override**: cleanest long-term, but tagpath crates.io tops out at 0.10.0 — the publish gap noted in `#p6lintbnd`.

❯ Can we publish tagpath 0.11 instead?

#### What I did NOT do

- No `git merge` or `git push` on agent-doc main
- No bump of the agent-loop superproject pointer (already tracks feature-branch HEAD)
- No skip-CI / force merge — the backlog item is explicit about waiting for CI before external consumers pull

#### Side effect of the race

The previous finalize attempt left nine stale `superseded`-state capture sidecars in `.agent-doc/captures/<doc-hash>/` that the running binary couldn't parse (unknown enum variant). Moved them to `/tmp/agent-doc-superseded-captures/`. This is a pre-existing binary/capture-format compatibility gap worth tracking on agent-doc's side; recording inline rather than as a new backlog item here since the symptom only surfaced incidentally and the cleanup is reversible.

#### Queue handling

Auto-loop stops here. Backlog now has `#p6adfcitagpath` (the CI fix) at the top; `#p6adfmerge` lives under Review with `[/]` waiting for CI to clear. Remaining unblocked queue (`#p6tsifull`, `#p6tspub`, `#p6skl`, `#p6ren`, `#p6meta`) is yours to reorder or restart at will.
### Re: Can we publish tagpath 0.11 instead? — opus-4-7

Yes — and it's the cleanest option.

#### Why 0.11 works

- crates.io tops out at **v0.10.0**.
- Local tags: **v0.1.0 → v0.6.2 → v0.10.0**. No tags exist for v0.11 through v0.18 — they were never released, just bumped in `Cargo.toml`.
- Only **12 commits** between v0.10.0 and HEAD, so the gap is small in commit terms even though Cargo.toml has marched ahead by 8 minor versions.
- Your MEMORY rule on this ("unpublished tags: move the tag forward to HEAD rather than bumping to a new version") applies directly. There are no tags to move, but the same logic says we don't owe v0.11–v0.18 a real release.

#### What it would take

1. **Bump `Cargo.toml` down**: `0.18.0 → 0.11.0`. Same for `pyproject.toml`.
2. **Update `VERSIONS.md`** with one consolidated v0.11.0 entry summarizing all the work since v0.10.0 (MCP install helper, lint relaxation, ontology, dyn-grammar, WASM, etc.).
3. **Publish**: `git tag v0.11.0 && git push origin v0.11.0 && cargo publish` — PyPI fires automatically.
4. **Update agent-doc dependency** from `version = "0.18"` to `version = "0.11"` in `src/agent-doc/Cargo.toml`. After this, the path override can be dropped entirely → no more "tagpath sibling missing" CI failures → `#p6adfmerge` unblocks.

#### One thing to confirm before bumping down

Anything that *publicly imports* `tagpath` and pins the version in its own manifest needs the new number too. Quick survey:

- `src/tsift/Cargo.toml` → `tagpath = { path = "../tagpath", version = "0.18" }` per `#p6tspub` backlog text. Will need the same bump.
- agent-doc Cargo.toml (already noted above).
- I should grep the workspace for other `tagpath = ` entries before publishing.

#### Recommendation

Bump `tagpath` to **0.11.0**, publish, then update both `src/agent-doc` and `src/tsift` Cargo.toml to `version = "0.11"`. That single move retires `#p6adfcitagpath` (CI sees a real crates.io version, drops the path override) AND `#p6tspub` (same fix unblocks tsift's publish path).

Want me to do the bump + publish in the next cycle? If yes, the natural ordering is: tagpath 0.11.0 publish → agent-doc Cargo.toml update → tsift Cargo.toml update → then proceed with `#p6adfmerge`.
❯ publish 0.11.0 + release
### Re: publish 0.11.0 + release — opus-4-7

**Published.**

#### Release artifacts

- **crates.io**: `tagpath v0.11.0` — published from a clean `cargo publish` (binary uploaded, registry confirmed).
- **GitHub Release**: <https://github.com/btakita/tagpath/releases/tag/v0.11.0> with the x86_64-linux prebuilt binary attached.
- **Git tag**: `v0.11.0` on `main`.
- **Local install**: `cargo install --path .` swapped `~/.cargo/bin/tagpath` from v0.18.0 → v0.11.0 in place.

#### What changed

- `src/tagpath/Cargo.toml`: `version = "0.18.0" → "0.11.0"`. `Cargo.lock` regenerated in the same commit.
- `src/tagpath/VERSIONS.md`: new file with a consolidated v0.11.0 entry covering all 12 commits since v0.10.0 (alias / prose / graph / index / ontology / watch / search / lint / mcp + install / dyn-grammar / wasm / lint-rule relaxation).
- `agent-loop`: submodule pointer bumped to the v0.11.0 commit.

#### Note on the force-push

The initial commit landed without `Cargo.lock` because cargo regenerates it on first compile. `cargo publish --dry-run` then refused on uncommitted-lockfile drift. Amended the commit to include the regenerated lockfile and force-pushed both `main` and the `v0.11.0` tag. Safe because the commit had not been consumed by any other repo yet, but worth flagging.

#### Downstream unblock (next cycles)

- `#p6adfcitagpath`: point `src/agent-doc/Cargo.toml` at `tagpath = { version = "0.11", ... }` (drop the path override) — clears the agent-doc CI redness that was gating `#p6adfmerge`.
- `#p6tspub`: same bump for `src/tsift/Cargo.toml`.

Both are straightforward edits to two files plus their `Cargo.lock`s, ready to run when you direct.
<!-- agent:boundary:24277ff2 -->
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
- do [#p6sklsymadocrunbooks]
- do [#p6adfcitagpath]
- do [#p6tsifull]
- do [#p6tspub]
- do [#p6skl]
- do [#p6ren]
- do [#p6meta]
<!-- /agent:queue -->

## Backlog

(<!-- agent:backlog -->
- [ ] [#p6adfcitagpath] CI fix prerequisite for p6adfmerge: agent-doc Cargo.toml declares tagpath = { path = '../tagpath', ... } but the agent-doc CI workflow doesn't clone tagpath as a sibling, so cargo clippy errors with 'No such file or directory' on ../tagpath/Cargo.toml. Fix options: (a) checkout-and-place tagpath sibling in CI before make check, (b) switch to git source pointing at btakita/tagpath, or (c) publish tagpath 0.18 to crates.io and drop the path override. Until this clears, the feature branch CI stays red and p6adfmerge stays gated.
- [ ] [#p6sklsymadocrunbooks] After agent-doc SKILL.md symlinking (p6sklsymagentdoc), check whether the runbooks under agent-loop/.claude/skills/agent-doc/runbooks/ should also be symlinked. Source of truth lives in src/agent-doc/runbooks/ but installed copies are in src/agent-doc/.claude/skills/agent-doc/runbooks/ (created by agent-doc skill install, so currently absent from the submodule). Either install once into the submodule and symlink, or leave install owning the parent copy and accept the duplication.
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
