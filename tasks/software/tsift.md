---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
## Exchange Summary (compacted)

**Key decisions:**
- **graphify boundary rule:** tsift owns deterministic, AST-derived graph operations. graphify owns LLM-derived semantic operations. Interop via `tsift graph --json`. (#bamc closed)
- **Presets:** `#spec-test-commit-push` and `#spec-test-build-install-commit-push` established for common workflows.
- **#9jc8 (graphify integration: entity extraction) is graphify-owned.** tsift's role (structured JSON export) is already complete. #zv4m (HTML viz) and #rd2f (semantic similarity) are also graphify-side.
- **`tsift summarize` is opt-in** — high-value for large codebases and exploration-heavy sessions; skip for small repos, heavy active dev, or no API key. Free commands (search, explain, graph) always apply.
- **`tsift status` is the single session-start check** — reports index freshness, summary state, and a machine-parseable `use:` list so agents know exactly which commands are available.
- **Search default strategy: lexical** (not hybrid). Hybrid/vector available via explicit `--strategy hybrid`. (#srch fix)
- **Auto-reindex hook scope:** per-project, not workspace-wide. Install in each submodule you actively develop in.
- **pending_capture_guard enforcement:** plan for pre-commit gate in `finalize` (phase 1) and `plan` emitting `pending_mutations` (phase 2) transferred to agent-doc-feat.md backlog.

**Completed work:**
- **#cdet** — Louvain community detection: `tsift communities`. `btakita/tsift@400c199`.
- **#gpth** — Graph path queries: `tsift path` + `tsift explain`. `btakita/tsift@6ad5983`.
- **#nkec** — Skill drift detection: `tsift audit`. `btakita/tsift@a4235b7`.
- **#865b** — Duplicate skill detection via Jaccard similarity. `btakita/tsift@59e1e79`.
- **#ay3j** — Usage tracking (`--usage`), **#t3pq** — cleanup recommendations (`--cleanup`), **#dt80** — report output (`--report`), **#cs4r** — `tsift lint`. All in `btakita/tsift@056dbac`.
- **#summ** — `tsift summarize` — cached LLM analysis with batch extraction and local SQLite cache. `btakita/tsift@5801090`.
- **#hidx** — Auto-reindex hook + `--exit-code` flag. `btakita/tsift@1c712f7`.
- **#cmdx** — `tsift init` — idempotent AGENTS.md/CLAUDE.md injection. `btakita/tsift@d9af2b0`.
- **#stat** — `tsift status` — session-start health check with `use:` list. `btakita/tsift@9f4a111`.
- **#srch** — Search hang fix: default to lexical strategy + scope sift path to submodule. `btakita/tsift@882421d`.
- **#wrka** — closed (workaround obsolete after #srch fix).
- **#mext** — closed (single-command Makefile wrapper, superseded by `tsift init` + `tsift status`).
- Test count progression: 125 → 139 → 153 → 166 → 174 → 197 → 211 → 222 → 229 → 236.

**Research notes:**
- Alternative tools to graphify: LightRAG, GraphRAG, sift-kg (Python, CLI-ready); EdgeQuake, Rig, petgraph (Rust, incomplete pipelines). No Rust tool ships the full ingest→extract→cluster→export pipeline as a CLI today.
- Token savings from `tsift summarize`: ~78% per bug-fix session (~25k → ~5.5k tokens), ~$73/month at Opus pricing for 8 sessions/day.
- Markdown is first-class: indexing extracts headings + code block languages via tree-sitter-md; search + lint work on .md/.mdx; call-graph intentionally skipped.

❯ Does `tsift init` or install add `.tsift/` to .gitignore?
### Re: .gitignore handling — opus-4-6

No. Neither `tsift init` nor `tsift index` adds `.tsift/` to `.gitignore`. It's a manual step today.

Both agent-loop and the tsift repo itself already have `.tsift/` in their `.gitignore`, but that was added by hand. A new project using tsift for the first time would need to add it manually or risk committing the index database, summary cache, and config.

`tsift init` is the natural place to add this — it already creates/modifies project files and is idempotent. The check would be: if `.gitignore` exists and doesn't contain `.tsift/`, append it; if no `.gitignore`, create one with `.tsift/`. Same pattern as `cargo init` adding `/target` or `npm init` adding `node_modules`.

❯ do #giti. spec-test-build-install-commit-push
### Re: do #giti — opus-4-6

`tsift init` now adds `.tsift/` to `.gitignore` automatically.

**Changes:**
- `src/init.rs` — `ensure_gitignore()` runs before the AGENTS.md injection. Creates `.gitignore` if missing, appends `.tsift/` if absent, skips if already present. `InitResult.gitignore_added` field for reporting.
- `src/main.rs` — `cmd_init` prints `.gitignore: added .tsift/` when the entry was added.
- `SPEC.md` — updated Init behavior list (gitignore is now step 1).

**Tests:** +3 → 239 total. Covers: create new `.gitignore`, append to existing, skip when already present.

**Commit:** `btakita/tsift@60ab0d3`. Pushed. Submodule pointer updated.

❯ If I run `/tsift init` on a repo twice, will the second time reindex everything?
### Re: tsift init idempotency — opus-4-6

No. `tsift init` does not index anything — it only touches two files:

1. **`.gitignore`** — checks if `.tsift/` is already present, skips if so
2. **`AGENTS.md` / `CLAUDE.md`** — checks if the `<!-- tsift:code-navigation -->` marker exists, skips if the section matches

Running it twice produces "already present (no changes needed)" on the second run. There's a dedicated test for this (`init_is_idempotent`).

`tsift init` and `tsift index` are separate commands with no overlap. Init sets up project instructions; index builds the AST symbol database. Neither calls the other.

❯ Is tsift currently active on this repo?
### Re: tsift status on agent-loop — opus-4-6

Yes. tsift is initialized and has an index, but it's stale:

```
index: stale (last indexed 31m ago, 5885 files tracked, 14364 stale)
summaries: none
use: search, explain, graph
```

The index exists with 5,885 tracked files but 14,364 files need reindexing. No summaries are cached. The core commands (`search`, `explain`, `graph`) are available — `summarize` is not (no summaries built).

To bring it current: `tsift index .`

❯ `/tsift init` on the claude code session on src/session-share/tasks/claudescore-3.md did not add `tsift/` to src/session-share/.gitignore. Why?
### Re: tsift init didn't update session-share .gitignore — opus-4-6

`tsift init` operates on the directory you pass it, defaulting to `.` (cwd). Claude Code's cwd is always the project root — `/home/brian/work/btakita/agent-loop` — regardless of which session document is active. So `/tsift init` modified (or would modify) agent-loop's `.gitignore`, not `src/session-share/.gitignore`.

Notably, agent-loop's `.gitignore` doesn't have `.tsift/` either right now, so either `tsift init` wasn't actually run on agent-loop, or the entry was lost in a subsequent `.gitignore` edit.

To fix session-share specifically: `tsift init src/session-share`

The broader gap: the `/tsift` skill doesn't infer the target submodule from the session document path. It could resolve `src/session-share/tasks/claudescore-3.md` → submodule root `src/session-share` and pass that to `tsift init` instead of `.`. That would be a skill-level change (not a tsift binary change), since the binary already accepts an explicit path argument.

❯ Please update the skill/runbook to infer submodules. #spec-test-build-install-commit-push
### Re: submodule inference for tsift init — opus-4-6

Done. `tsift init` now resolves file paths to their git/submodule root via `git rev-parse --show-toplevel`.

**Changes:**
- `src/init.rs` — `resolve_project_dir()`: if path is a file, uses parent dir; then resolves to git root. Falls back to the directory itself outside git repos.
- `src/main.rs` — `cmd_init` calls `resolve_project_dir` before `init()`. Prints `resolved: <input> → <target>` when the path changes.
- `SPEC.md` — new "Path Resolution" section documenting the 3-step resolution.
- `/tsift` skill (`SKILL.md`) — added Init section with submodule inference docs.

**Tests:** +4 → 243 total. Covers: directory passthrough, file→parent, git root from subdirectory, git root from file.

**Integration verified:**
```
$ tsift init src/session-share/tasks/claudescore-3.md
resolved: .../claudescore-3.md → .../src/session-share
AGENTS.md: created (tsift Code Navigation section added)
.gitignore: added .tsift/
```

**Commits:** `btakita/tsift@9269260`. Session-share initialized (`0a5fa2a`). Agent-loop pointers + skill updated. All pushed.

❯ Please look in agent-loop and all submodules to see if `.tsift/index.db` was committed. Please remove `.tsift/index.db` from the git history and force push.
### Re: .tsift/index.db audit + history rewrite — opus-4-6

**Audit results:**
- **agent-loop + all submodules (working tree):** `.tsift/index.db` not tracked anywhere. Clean.
- **agent-loop history:** never committed.
- **agent-doc history:** `.tsift/index.db` (7.4MB) was committed in early test-author commits (`7a7855a`, `e43a9ed`, `7e8055b`, `c718591`) on the `20260421/release-0.33.10` branch and tags `v0.33.11`–`v0.33.13`. Not on main/HEAD.
- **All other submodules:** clean history.

**Cleanup:**
- Ran `git filter-repo --invert-paths --path .tsift/index.db --force` in `src/agent-doc`.
- Verified: zero commits now reference `.tsift/index.db` across all refs.
- Force-pushed all branches and tags to `origin`.

No other submodules were affected.
<!-- agent:boundary:249fb403 -->
<!-- /agent:exchange -->

<!--
Please investigate ways that tsift can save on token usage. You may look through as much of my session history as needed. Also note that I'm using Codex so look through Codex history as well.
-->

## Pending / Not Built

<!-- agent:backlog patch=replace -->
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
