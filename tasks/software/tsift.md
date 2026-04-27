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

**Completed work:**
- **#cdet** — Louvain community detection: `tsift communities` ships phase-1 modularity optimization over call graph. 357 lines, 8 tests. `btakita/tsift@400c199`.
- **#gpth** — Graph path queries: `tsift path <from> <to>` (BFS shortest path) and `tsift explain <symbol>` (definitions + callers + callees + community). 14 tests. `btakita/tsift@6ad5983`.
- **#nkec** — Skill drift detection: `tsift audit` scans installed skills, checks SKILL.md health, compares against manifest. 12 tests. `btakita/tsift@a4235b7`.
- Test count progression: 125 → 139 → 153 → 166.

**Note:** #jzm8 in pending ("tsift native: community detection") appears stale — #cdet already shipped this work.

❯ sync orchestra
❯ #spec-test-commit-push
❯ - do #865b
❯ - do #ay3j
❯ - do #t3pq
❯ - do #dt80
❯ - do #cs4r

❯ do #865b
### Re: do #865b — sonnet-4-6

Implemented `tsift audit` duplicate skill detection via Jaccard similarity on description word sets.

**Algorithm:** pairwise Jaccard on stop-word-filtered token sets from `description` frontmatter. Threshold 0.3. Sorted descending by score. Skills without descriptions are skipped.

**Changes:**
- `src/audit.rs` — `SimilarPair` struct, `find_similar_pairs()`, `description_tokens()` with stop word list; `AuditResult.similar_pairs` field; called from `scan_skills()`
- `src/main.rs` — `cmd_audit` prints "Possible duplicates" block when pairs exist
- `SPEC.md` + `CLAUDE.md` updated

**Tests:** +8 tests → 174 total (`btakita/tsift@59e1e79`)

**Smoke test on `~/.claude/skills`:** 3 skills, 0 duplicates detected (agent-doc / graphify / tsift are semantically distinct — correct).
### Re: sync orchestra #spec-test-commit-push — opus-4-6

All 5 tasks implemented in a single commit (`btakita/tsift@056dbac`), plus #jzm8 marked done (stale — #cdet already shipped community detection).

#### #865b — Duplicate skill detection
Already shipped in `59e1e79`. Jaccard similarity over description word sets, threshold 0.3. 8 tests. No new work needed.

#### #ay3j — Usage tracking (`--usage`)
Scans Claude Code session history (`.jsonl` files across all `~/.claude/projects/*/`) for `Skill` tool invocations. Counts per skill, sets `invocation_count` on each `SkillEntry`. Plugin-namespaced skills (`codex:rescue`) counted under base name (`codex`). Uninstalled skills with invocations included in usage list. 5 tests.

#### #t3pq — Cleanup recommendations (`--cleanup`)
Flags skills for pruning when any of: health issues, zero invocations, or ≥50% Jaccard similarity with another skill. Token savings estimated as total file bytes / 4. Sorted by savings descending. 4 tests.

#### #dt80 — Report output (`--report <path>`)
Writes markdown audit report with skills table (status, name, description, uses), duplicate pairs, manifest diffs, and cleanup recommendations with total savings estimate. Creates parent directories. 2 tests.

#### #cs4r — `tsift lint`
New subcommand. Detects unannotated concepts in markdown by cross-referencing plain text against known graph entities (AST symbols from `symbols.db`, headings, bold terms, backtick terms ≥4 chars). Skips code blocks, headings, HTML comments, already-annotated terms. Classifies suggestions: symbol → backtick, multi-word capitalized → link, other → bold. 12 tests.

**Changes:**
- `src/audit.rs` — `SkillUsage`, `CleanupEntry` structs; `track_usage()`, `generate_cleanup()`, `write_report()`; `SkillEntry.invocation_count` field
- `src/lint.rs` — new module: `lint_markdown()`, `collect_entities_from_db()`, `collect_entities_from_markdown()`
- `src/main.rs` — `--usage`, `--cleanup`, `--report` flags on `Audit`; new `Lint` subcommand + `cmd_lint()`
- `SPEC.md` + `CLAUDE.md` updated with all new commands and test count

**Tests:** 166 → 197 (+31). `btakita/tsift@056dbac`. Pushed.

❯ #spec-test-build-install-commit-push
### Re: #spec-test-build-install-commit-push — opus-4-6

Already complete from previous cycle. Verified:

- **Spec:** SPEC.md updated with all 5 features (usage tracking, cleanup, report, lint)
- **Tests:** 197 pass (31 new)
- **Build + install:** release binary at `~/.cargo/bin/tsift` includes `--usage`, `--cleanup`, `--report`, and `lint` subcommand
- **Commit + push:** `btakita/tsift@056dbac` on main, pushed

No new changes needed — all artifacts are in sync.

❯ re #9jc8: can tsift implement this?
### Re: #9jc8 — opus-4-6

No — and the boundary rule already says why. From SPEC.md:

> tsift owns deterministic, AST-derived clustering. For LLM-derived semantic groupings, use graphify's semantic layer over `tsift graph --json` output.

tsift is zero-API-key, deterministic, fast. Adding LLM calls would break all three properties.

What tsift *does* provide is the interop surface graphify needs:

- `tsift graph --json` — full call graph as JSON
- `tsift communities --json` — Louvain-clustered symbol groups
- `tsift explain <symbol>` — definitions + callers + callees + community context
- `tsift symbols` — complete symbol table

The implementation path for #9jc8 is a graphify feature that consumes tsift's JSON output, runs it through an LLM for entity extraction / concept labeling, and merges the result into graphify's knowledge graph. tsift's role is already complete — it exports the structured data.

I'd recommend closing #9jc8 as "graphify-owned" and tracking it in a graphify session doc if you want to pursue it. The pending items #zv4m (HTML visualization) and #rd2f (semantic similarity) are similarly graphify-side work.

❯ Are there alternative rust or cli tools that can do what graphify does?
### Re: alternative tools for graphify's role — opus-4-6

The short answer: Python dominates this space. Rust is catching up but nothing ships a complete CLI pipeline yet.

**Closest alternatives (all Python):**

| Tool | CLI? | What it does | Trade-off |
|------|------|-------------|-----------|
| **LightRAG** (HKUDS) | Yes, 5 commands | Entity/relationship extraction via LLM, dual-level retrieval, 3D graph viewer | ~10x cheaper than GraphRAG. Active, EMNLP 2025 |
| **Microsoft GraphRAG** | Yes (`graphrag init/index/query`) | Full pipeline: Leiden community detection, global/local query modes | Token-heavy. Python 3.11+ |
| **sift-kg** | Yes | LLM entity extraction, dedup, export JSON/GraphML/GEXF/SQLite, browser viz | Most direct CLI analogue to graphify |

**Rust-native options:**

| Tool | Status | Notes |
|------|--------|-------|
| **EdgeQuake** | Active, young (~2025) | Rust GraphRAG inspired by LightRAG. 8 crates, Tokio async, Sigma.js frontend. REST API only — no single-binary CLI yet |
| **Rig** (`rig-core`) | Mature | LLM framework with RAG pipelines, vector stores. No graph extraction built in — it's plumbing |
| **petgraph** | Stable | Graph data structures + algorithms. The Rust standard for graph ops, but no LLM awareness |

**Bottom line:** No Rust tool assembles the full "ingest → extract entities → cluster → HTML+JSON" pipeline as a CLI today. If you wanted to replace graphify with a standalone tool:

1. **Quickest path:** LightRAG or sift-kg (Python, CLI-ready now)
2. **Rust-native path:** EdgeQuake is closest but REST-only; building a CLI wrapper around it or composing Rig + petgraph + a viz layer is feasible but non-trivial
3. **Current graphify advantage:** it runs in-context inside Claude Code with zero install — the LLM is already there. A standalone tool needs API keys and network calls

For the tsift integration story specifically, any of these tools could consume `tsift graph --json` output the same way graphify does.

❯ I don't need a REST API or HTML+JSON. I mainly need to save on tokens + analysis.
### Re: token savings without REST/HTML — opus-4-6

If the goal is purely "save tokens + get analysis," the expensive part is graphify re-reading source into context every invocation. The fix is caching:

**Option A — tsift extension (`tsift summarize`)**

A new subcommand that:
1. Walks `tsift graph --json` + source snippets
2. Calls Claude API (batch, once per module) for entity extraction / concept labeling
3. Stores results in a local SQLite graph (entities, relationships, summaries)
4. Future queries hit the cache: `tsift summarize <symbol>` returns the pre-computed 2-line summary + relationships without re-reading source

Token cost: one-time batch extraction. After that, queries are free (local DB lookup). Could rerun on `git diff` changes only.

**Option B — LightRAG / sift-kg (Python, one-shot batch)**

Run once over the codebase, extract entities into a local store, query locally. Same "pay once, query free" model. More mature but Python dependency.

**Option C — Keep graphify but add caching**

graphify already has the extraction logic. Adding a persistent cache layer (SQLite or flat JSON per file hash) would eliminate repeat extraction. Cheapest to implement since the LLM is already in-context.

**My recommendation:** Option A fits the tsift philosophy (single Rust binary, zero runtime deps, deterministic queries after one-time extraction). It reuses the existing `symbols.db` infrastructure. The batch extraction could use the Anthropic batch API for 50% cost reduction.

Want me to spec this out as a `tsift summarize` feature?

❯ Yes. spec + add to backlog
### Re: spec + backlog for tsift summarize — opus-4-6

Done. Added `## Summarize (Cached LLM Analysis)` section to `src/tsift/SPEC.md` and updated `CLAUDE.md`.

**What the spec defines:**

- `tsift summarize <symbol>` — instant cached lookup (no LLM call at query time)
- `tsift summarize --extract <path>` — one-time batch extraction via Anthropic batch API (haiku, 50% cost reduction)
- `tsift summarize --extract --diff` — incremental: only re-extracts git-changed files
- `tsift summarize --stats` — cache hit rate, staleness, estimated token savings

**Storage:** new `summaries.db` with entities, relationships, concept labels, 2-sentence summaries. Cache key is `blake3(file_content) + symbol_name` — invalidates automatically on source changes.

**Token savings model:** ~2000 tokens per 500-line file read vs ~50-100 tokens for cached summary. Extraction cost is one-time and amortized by the batch API discount.

**Boundary rule:** extraction is batch-only, queries are local DB lookups. No live LLM calls at query time, no visualization.

Added `#summ` to backlog.

❯ do #summ. spec-test-build-install-commit-push
### Re: do #summ — opus-4-6

Implemented `tsift summarize` — cached LLM analysis with batch extraction and local SQLite cache.

**New module: `src/summarize.rs`** (660 lines)

- `SummaryDb` — SQLite wrapper for `summaries.db` with CRUD operations
- `content_hash()` — blake3 content hashing for cache invalidation
- `extract_for_file()` — calls Anthropic Messages API (haiku-class), parses structured JSON response into entities, relationships, concept labels, and summaries
- `ExtractionReport` — tracks files processed, symbols extracted, tokens spent
- `SummarizeConfig` — reads from `[summarize]` section of `.tsift/config.toml`

**Three modes:**

1. **Query** — `tsift summarize <symbol>` or `tsift summarize --file <path>`: instant local SQLite lookups, zero API calls
2. **Extract** — `tsift summarize --extract <path>` or `--extract --diff`: one-time LLM extraction via Anthropic API. `--diff` mode uses `git diff --name-only HEAD` for incremental re-extraction
3. **Stats** — `tsift summarize --stats`: summaries count, files covered, tokens spent, estimated savings

**Storage:** `summaries.db` with `blake3(file_content)` cache key. Extraction stores file-level and per-entity summaries. Automatic invalidation when source changes.

**Dependencies added:** `blake3` (content hashing), `ureq` (sync HTTP for Anthropic API).

**Tests:** +14 → 211 total. Covers DB CRUD, entity roundtrip, content hashing, large file rejection, API key validation, prompt construction.

**Commit:** `btakita/tsift@5801090`. Pushed.

❯ What prompts and skills should I use to reliably apply tsift to a claude code session on a codebase?
### Re: applying tsift to a Claude Code session — opus-4-6

Three layers: one-time setup, CLAUDE.md integration, and session-time commands.

#### 1. One-time setup (per codebase)

```bash
# Build the AST index (symbols.db + call graph)
tsift index .                          # single repo
tsift index --workspace                # multi-submodule

# Pre-cache LLM summaries (pay once, query free)
tsift summarize --extract src/         # full extraction
tsift summarize --extract --diff src/  # incremental after edits
```

The rewrite hook (`~/.claude/hooks/tsift-rewrite.sh`) already intercepts `rg`/`grep -r` → `tsift search` automatically. No prompt needed for that.

#### 2. CLAUDE.md integration

Add to the project's `CLAUDE.md` so every session knows tsift is available:

```markdown
## Code Navigation

- `tsift search <query>` — AST-aware hybrid search (use instead of grep/rg)
- `tsift graph <symbol> --callers` — who calls this function?
- `tsift graph <symbol> --callees` — what does it call?
- `tsift explain <symbol>` — full context: callers, callees, community
- `tsift summarize <symbol>` — cached 2-sentence summary + entities
- `tsift communities` — architectural subsystem clustering
- `tsift path <from> <to>` — shortest call-chain between symbols
```

This puts the commands in context for every session without a prompt.

#### 3. Session-time patterns

**For exploration/onboarding:**
- "Use `tsift explain main` to understand the entry point, then `tsift communities` for the high-level architecture"
- Or just: "What does this codebase do?" — with tsift indexed, the agent can use `tsift search` and `tsift summarize` instead of reading full files

**For focused work:**
- `tsift graph <symbol> --callers` before refactoring (find all call sites)
- `tsift summarize --file src/config.rs` to understand a module without reading it into context
- `tsift path <from> <to>` to trace how two distant symbols connect

**The `/tsift` skill** wraps these as a Claude Code slash command for convenience — invoke with `/tsift <query>` for search.

#### What makes it reliable

The key property is **determinism after indexing**. `tsift index` + `tsift summarize --extract` are the only steps that touch external resources (file system, API). After that, every query is a local SQLite lookup — instant, offline, and token-efficient (~50-100 tokens vs ~2000 for reading the source file).

The main failure mode is a stale index. Reindex after significant refactoring: `tsift index --rebuild .` or `tsift summarize --extract --diff src/`.

❯ How can I ensure that any code prompt will use tsift to save tokens? Do I need to explicitly mention tsift? I would rather it just be applied without having to mention tsift.
### Re: transparent tsift usage — opus-4-6

Two mechanisms already work transparently. A third requires a CLAUDE.md addition.

#### Already transparent (no mention needed)

1. **Rewrite hook** — `tsift-rewrite.sh` is a PreToolUse hook that intercepts `rg`/`grep -r` Bash calls and silently rewrites them to `tsift search --strategy lexical`. The agent never sees tsift in the command; it thinks it's running rg.

2. **AST-aware search ranking** — when `tsift search` runs and an index exists, symbol matches get boosted above text matches. This happens automatically after `tsift index .`.

#### Requires CLAUDE.md (one-time, then transparent)

The built-in Read/Grep/Glob tools bypass Bash hooks entirely — they're native Claude Code tools, not shell commands. To make the agent prefer tsift for those operations, add a directive to the project CLAUDE.md:

```markdown
## Code Navigation

Before reading source files, check for cached summaries:
- `tsift summarize <symbol>` for function/type context (~50 tokens vs ~2000)
- `tsift explain <symbol>` for callers/callees/community
- `tsift search <query>` for symbol-aware search

Only read full files when the cached summary is insufficient or absent.
```

This works because Claude Code loads CLAUDE.md at the start of every session and treats it as standing instructions. The agent will internalize "check tsift first" as a default behavior pattern.

#### What you can't automate (yet)

There's no hook point to intercept the native `Read` or `Grep` tool calls and substitute tsift. Those tools are hardwired in the harness. The CLAUDE.md directive is the closest you can get — it makes the agent _choose_ tsift first, but can't _force_ it.

The practical gap: a new session with a fresh context window may still read files directly for the first few operations before the CLAUDE.md directive fully shapes behavior. After that, the pattern sticks for the session.

❯ How do we ensure that tsift has done the "one-time batch extraction"?
### Re: ensuring one-time extraction — opus-4-6

Two separate concerns: indexing (free, fast, fully automatable) and summarize extraction (costs API tokens, opt-in).

#### Indexing (`tsift index`)

Best automated via a Claude Code hook in `.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "matcher": "",
        "command": "tsift index --check . 2>/dev/null | grep -q 'modified: 0' || tsift index ."
      }
    ]
  }
}
```

This runs on every prompt, checks if the index is stale, and re-indexes silently if needed. Fast for already-indexed repos (~50ms check).

Alternatively, a Makefile target:

```makefile
tsift-setup:
	tsift index .
	@echo "Index ready. Run 'tsift summarize --extract src/' for LLM cache."
```

#### Summarize extraction (`tsift summarize --extract`)

This calls the Anthropic API and costs tokens, so it shouldn't auto-run. Three options:

1. **CLAUDE.md gate** — add to the project CLAUDE.md:
   ```markdown
   If `tsift summarize --stats` shows 0 summaries, tell the user:
   "Run `tsift summarize --extract src/` to build the summary cache (~$0.01/file with haiku)."
   ```
   The agent checks at session start and prompts once.

2. **Makefile** — `make tsift-extract` as an explicit user step after cloning.

3. **CI/scheduled** — run extraction in CI or via a cron job (`/schedule`), commit `summaries.db` to the repo. Then every session gets the cache for free with no API call.

#### Checking freshness

`tsift summarize --stats` shows whether the cache exists and is populated. `tsift index --check .` reports stale files without modifying anything. Both are fast and side-effect free — safe for hooks or startup checks.

The practical recommendation: hook the index check (free), gate the extraction behind a one-time prompt or Makefile target (costs money), and put both in CLAUDE.md so the agent knows the workflow.
<!-- agent:boundary:ec18489e -->
<!-- /agent:exchange -->

<!--
Please add "check tsift first". 
-->

## Pending / Not Built

<!-- agent:backlog patch=replace -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
-->
