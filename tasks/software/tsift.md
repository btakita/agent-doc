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
- **Don't fork sift** — timeout wrapper is sufficient. Reconsider if fresh-index searches on ~6K-file repos regularly hit 30s. Upstream bug: [rupurt/sift#2](https://github.com/rupurt/sift/issues/2).
- **Binary JSON protocols not useful** — agents consume text tokens, not wire format. `rmp-serde` for machine-to-machine pipelines only.
- **Chinese/CJK compression not applicable** — tsift output is mostly identifiers, not translatable prose.
- **Reduce semantic content is safe in --compact mode only** — non-compact retains all fields, --json unaffected.
- **#paths and #caps remain highest-impact open items** — #paths needs project-root detection, #caps needs per-command default limits.

**Completed work:**
- **#cdet** — Louvain community detection: `tsift communities`. `btakita/tsift@400c199`.
- **#gpth** — Graph path queries: `tsift path` + `tsift explain`. `btakita/tsift@6ad5983`.
- **#nkec** — Skill drift detection: `tsift audit`. `btakita/tsift@a4235b7`.
- **#865b** — Duplicate skill detection via Jaccard similarity. `btakita/tsift@59e1e79`.
- **#ay3j** — Usage tracking (`--usage`), **#t3pq** — cleanup recommendations (`--cleanup`), **#dt80** — report output (`--report`), **#cs4r** — `tsift lint`. All in `btakita/tsift@056dbac`.
- **#summ** — `tsift summarize` — cached LLM analysis with batch extraction and local SQLite cache. `btakita/tsift@5801090`.
- **#hidx** — Auto-reindex hook + `--exit-code` flag. `btakita/tsift@1c712f7`.
- **#cmdx** — `tsift init` — idempotent AGENTS.md/CLAUDE.md injection + .gitignore setup. `btakita/tsift@d9af2b0`, `btakita/tsift@60ab0d3`.
- **#stat** — `tsift status` — session-start health check with `use:` list. `btakita/tsift@9f4a111`.
- **#srch** — Search hang fix: default to lexical strategy + scope sift path to submodule. `btakita/tsift@882421d`.
- **#hang** — 30-second search timeout (configurable via `--timeout`). `btakita/tsift@91aaab0`.
- **#iflood** — `tsift index --quiet` suppresses per-file change list. `--exit-code` implies `--quiet`. `btakita/tsift@936b2da`.
- **#compact** — Global `--compact` flag for terse human-readable output across all commands. `btakita/tsift@e01e680`.
- **#json** — Compact JSON default + `--pretty` flag. 22 call sites updated. `btakita/tsift@ce4855f`.
- **#rtk** — RTK TOML filters (`~/.config/rtk/filters.toml`) + hook v2 (`tsift-rewrite.sh`). Routes verbose commands through RTK for output capping. `btakita/tsift@951a28a`.
- **#semred** — Reduce semantic content: dropped per-community Q scores and language annotations in compact mode. `btakita/tsift@5b050af`.
- **Submodule inference** — `tsift init` resolves file paths to git/submodule root. `btakita/tsift@9269260`.
- **`.tsift/index.db` cleanup** — removed from agent-doc history via filter-repo + force push.
- **`.siftignore`** — created at agent-loop root (workaround until upstream sift fix lands).
- **Codex tsift setup** — `tsift init` ensures AGENTS.md alongside CLAUDE.md.
- **#wrka** — closed (workaround obsolete after #srch fix).
- **#mext** — closed (single-command Makefile wrapper, superseded by `tsift init` + `tsift status`).
- Test count progression: 125 → 139 → 153 → 166 → 174 → 197 → 211 → 222 → 229 → 236 → 239 → 243 → 246 → 249 → 257.

**Research notes:**
- Alternative tools to graphify: LightRAG, GraphRAG, sift-kg (Python, CLI-ready); EdgeQuake, Rig, petgraph (Rust, incomplete pipelines). No Rust tool ships the full ingest→extract→cluster→export pipeline as a CLI today.
- Token savings from `tsift summarize`: ~78% per bug-fix session (~25k → ~5.5k tokens), ~$73/month at Opus pricing for 8 sessions/day.
- Markdown is first-class: indexing extracts headings + code block languages via tree-sitter-md; search + lint work on .md/.mdx; call-graph intentionally skipped.
- Token savings investigation: 30 Claude Code + 227 Codex sessions analyzed. Priority: search hang > index flood > compact > JSON > paths > caps > codex > RTK.
- Practical compression strategies: (1) reduce semantic content, (2) shorter JSON field names, (3) tabular output, (4) abbreviation conventions, (5) schema-then-values. Items 1-2 worth building, 3-5 marginal.
<!-- agent:boundary:c89410a8 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog patch=replace -->
- [ ] [#bags] [recommended] Shorter field names in JSON — --terse mode with schema header
- [ ] [#paths] [recommended] Path prefix stripping — relative paths instead of absolute
- [ ] [#caps] [recommended] Output caps for explain/graph/communities (--limit N)
- [ ] [#codex] [recommended] Codex tsift integration — rewrite hook or AGENTS.md instructions
- [ ] [#tabular] [recommended] Tabular output for repeated structures (search results as TSV, edges as columns)
- [ ] [#abbrev] [recommended] Abbreviation conventions in --compact (fn/mod/unch/del)
- [ ] [#schema] [recommended] Schema-then-values pattern for repeated output (headers once, rows of values)
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
