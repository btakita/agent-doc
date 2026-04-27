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

❯ do #bags. spec-test-build-install-commit-push
### Re: do #bags — opus-4-6

Implemented `--terse` global flag for abbreviated JSON field names with inline schema header.

**What it does:**
- `tsift --terse <command>` outputs JSON with shortened field names (implies `--json`)
- Output format: `{"_s": {short→long mapping}, "d": {data with short keys}}`
- Schema header only includes keys present in the current response
- 100+ field name mappings covering all output structs (e.g., `caller_file`→`cf`, `name`→`n`, `modularity`→`q`)
- Unknown keys pass through unchanged
- Compatible with `--pretty` for indented terse output

**Commit:** `btakita/tsift@c2010bd`. 261 tests pass (5 new terse tests). Built + installed.

❯ do #paths. spec-test-build-install-commit-push
### Re: do #paths — opus-4-6

Implemented `--absolute` global flag. All output paths are now project-relative by default (token savings).

**What it does:**
- Default: paths stripped to project-relative (`src/main.rs` instead of `/home/user/project/src/main.rs`)
- `tsift --absolute <command>` restores full filesystem paths
- Affects: search, graph, explain, index, summarize, lint
- JSON: known path keys (`file`, `path`, `caller_file`, `file_path`) stripped via `relativize_json_paths()`
- Database storage stays absolute — stripping is display-only

**Implementation:**
- `relativize(path, root)` / `relativize_pathbuf()` utility functions
- `relativize_edges()`, `relativize_symbols()`, `relativize_symbol_hits()` batch helpers
- `relativize_json_paths()` for post-processing sift library JSON output
- Threaded through `cmd_graph`, `cmd_explain`, `cmd_search`, `cmd_index` via `absolute` parameter

**Commit:** `btakita/tsift@357af45`. 268 tests pass (7 new). Built + installed.

❯ do #caps. #spec-test-build-install-commit-push
### Re: do #caps — opus-4-6

Implemented `--limit N` flag for `graph`, `communities`, and `explain` commands.

**What it does:**
- `graph --limit N` (default 20): caps edges per direction (callers/callees separately)
- `communities --limit N` (default 10): caps number of communities displayed
- `explain --limit N` (default 15): caps callers and callees lists each
- `--limit 0` disables the cap (show everything)
- Short flag: `-l N`

**Truncation indicators:**
- Text output: `(+N more)` suffix with hint to use `--limit 0`
- JSON output: `truncated: true` boolean + `total` / `callers_total` / `callees_total` counts

**Implementation:**
- Truncation applied after query, before display — no database changes
- JSON combined mode (`graph` with both directions) includes per-direction truncation fields
- Communities truncation applied after `min_size` filtering

**Commit:** `btakita/tsift@e72e982`. 277 tests pass (9 new limit tests). Built + installed.

❯ do #codex. #spec-test-build-install-commit-push
### Re: do #codex — opus-4-6

Implemented `tsift init --codex` for Codex hook integration.

**What it does:**
- `tsift init --codex` merges a `UserPromptSubmit` auto-reindex hook into `.codex/hooks.json`
- Creates `.codex/` directory and `hooks.json` if they don't exist
- Appends to existing hooks when `.codex/hooks.json` already has other hooks (e.g., agent-doc)
- Idempotent — detects existing tsift hook by status message and skips
- Hook command: `tsift index --check --exit-code . >/dev/null 2>&1 || tsift index . >/dev/null 2>&1`

**Why not the rewrite hook?** Codex doesn't support `PreToolUse` hooks, so the `grep`/`rg` → `tsift search` rewrite can't be ported. The AGENTS.md code-navigation instructions already tell Codex agents to prefer tsift over grep/rg.

**Commit:** `btakita/tsift@474ef45`. 282 tests pass (5 new Codex hook tests). Built + installed.

❯ do #tabular. spec-test-build-install-commit-push
### Re: do #tabular — opus-4-6

Implemented `--tabular` global flag for TSV output across four commands.

**What it does:**
- `tsift --tabular search <query>` — two TSV tables: symbols (`match_type`, `kind`, `name`, `file`, `line`, `score`) then hits (`rank`, `path`, `confidence`, `score`)
- `tsift --tabular graph <sym>` — edges table (`direction`, `name`, `file`, `line`) with `caller`/`callee` direction values
- `tsift --tabular communities` — table (`id`, `size`, `members`) where members are comma-separated
- `tsift --tabular explain <sym>` — definition table, then caller/callee edges, then community summary
- Truncation: `# (+N more)` comment lines; sections separated by blank lines
- Compatible with `--limit`, `--absolute`

**Commit:** `btakita/tsift@d83f1eb`. 289 tests pass (7 new tabular tests). Built + installed.

❯ do #abbrev. spec-test-build-install-commit-push
### Re: do #abbrev — opus-4-6

Implemented abbreviation conventions in `--compact` mode for shorter human-readable output.

**What it does:**
- Symbol kinds: `function`→`fn`, `method`→`meth`, `class`→`cls`, `interface`→`iface`, `type_alias`→`type`, `data_class`→`data_cls`, `sealed_class`→`sealed_cls`, `enum_class`→`enum_cls`, `companion_object`→`comp_obj`, `object`→`obj`, `heading`→`h`, `code_block`→`code`
- Match types: `exact_name`→`exact`, `partial_tags`→`partial`
- Section headers: `symbols`→`syms`, `callers`→`crs`, `callees`→`ces`, `community`→`comm`, `communities`→`comms`, `members`→`mbrs`, `symbol`→`sym`, `definitions`→`defs`
- Short kinds pass through unchanged: `struct`, `trait`, `enum`, `const`, `static`, `mod`, `impl`
- Applied in search, explain, graph, and communities compact output

**Commit:** `btakita/tsift@ea59263`. 291 tests pass (2 new abbreviation tests). Built + installed.
<!-- agent:boundary:c7ab815c -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog patch=replace -->
- [x] [#abbrev] [recommended] Abbreviation conventions in --compact (fn/mod/unch/del)
- [ ] [#schema] [recommended] Schema-then-values pattern for repeated output (headers once, rows of values)
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
