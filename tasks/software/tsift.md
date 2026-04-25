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
<!-- agent:boundary:23fa6c80 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#865b] tsift audit: duplicate skill detection — find overlapping skills by description similarity
- [ ] [#ay3j] tsift audit: usage tracking — analyze claude-mem/session history for skill invocation frequency, flag never-used skills
- [ ] [#t3pq] tsift audit: cleanup recommendations — generate actionable prune list with token savings estimates per removed skill
- [ ] [#dt80] tsift audit --report: write drift report to tasks/agent-doc/skills-audit.md (nightly cron target via /schedule)
- [ ] [#cs4r] tsift lint: detect unannotated concepts in markdown — cross-reference plain text against known graph entities (symbols, headings, bold terms), suggest backtick/bold/link annotations
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#jzm8] tsift native: community detection (Louvain clustering) over call_edges + symbol references
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:pending -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
-->
