---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
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
<!-- agent:boundary:52dafa9a -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:pending -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
-->
