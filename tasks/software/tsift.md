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
<!-- agent:boundary:c31bee72 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog patch=replace -->
- [x] [#giti] [recommended] tsift init should add .tsift/ to .gitignore (index + summaries are local artifacts)
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
