---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
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

**Core decisions:**
- tsift owns deterministic AST-derived indexing/search/graph operations; graphify owns LLM-semantic layers.
- `tsift status` is the standard session-start check; search defaults to lexical; `tsift summarize` stays opt-in.
- stale-index protection is hook-first (`tsift init --codex`, Claude hook script), but search/status also do fast stale checks and safer recovery paths.
- keep using SQLite; the real fixes were WAL/read-only/query-path/locking behavior, not swapping databases.

**Major completed work:**
- output/token-efficiency surface: `--compact`, compact JSON by default + `--pretty`, `--terse`, `--schema`, `--tabular`, compact-mode abbreviations, project-relative paths by default via `--absolute`, and per-command display caps.
- navigation/query surface: `communities`, `path`, richer `explain`, `status`, `audit`, `lint`, `summarize`, Codex init/hook support, instruction version markers, and workspace-aware hook installation.
- index/search reliability: lexical-by-default search, stale-index fast-fail + autoindex flow, rollback-journal snapshot fallback, `tsift locks`, inline lock diagnostics, OS-backed `index.lock`, transactional index + summary-cache writes, read-only query opens, helper-process search timeout cleanup, bounded symbol candidate queries, Anthropic API status checks, and explicit `wal_autocheckpoint=256` on writable index connections.
- versioning: tsift now has explicit private version tracking via `Cargo.toml`, `tsift --version`, and `VERSIONS.md`; current binary version reached `0.1.2` at `btakita/tsift@1381e76`.

**Review / investigations:**
- code review found high-impact issues around transactional safety, diff-path filtering, summary-cache replacement, API status handling, lock handling, unbounded symbol loads, timed-out search workers, WAL checkpointing, silent `.ok()` failures, and missing E2E coverage.
- completed fixes covered `#txn`, `#diffpath`, `#sumtxn`, `#apicheck`, `#lockfix`, `#symlimit`, `#threadjoin`, and `#walckpt`.
- tmux pane crashes on this document were treated as an `agent-doc` issue instead of tsift work; tracked externally as `#tmuxcrash` with a diagnostic plan in `tasks/agent-doc/plan-tmux-pane-crash-stability.md`.

**Live open work:**
- `#silentok` — log warnings on silent file-read / parse `.ok()` failures.
- `#e2e` — add end-to-end CLI coverage for search, graph, explain, communities, and path.

❯ do #silentok. spec-test-commit-push
### Re: do #silentok — gpt-5

Implemented in tsift.

What changed:
- `IndexSummary` now carries structured `warnings` entries for non-fatal indexing failures, with stage metadata for source-read, symbol-extraction, and call-extraction failures.
- `apply_changes()` no longer silently swallows those `.ok()` paths: it records the warning, keeps the index update non-fatal, and continues without inserting partial symbols/edges for the failed file.
- shared index-update flows now emit those warnings on stderr, so both `tsift index` and search autoindex paths surface partial extraction failures instead of hiding them.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the binary version to `0.1.3`.

Verification:
- `make check` passed: 352 unit tests + 14 exit-code tests.
- added coverage for unreadable-file warning capture in `IndexSummary` and CLI stderr logging from `tsift index`.

Commit: `btakita/tsift@0ac2a22` pushed to `origin/main`.

❯ do #e2e. spec-test-commit-push
### Re: do #e2e — gpt-5

Implemented in tsift.

What changed:
- added a real indexed temp-project fixture in `tests/exit_code.rs`
- added end-to-end JSON coverage for `search`, `graph`, `communities`, `path`, and `explain`
- documented the integration-fixture expectation in `SPEC.md` and `CLAUDE.md`

Verification:
- `cargo test --test exit_code` passed: 19 tests.
- `make check` passed: 352 unit tests + 19 exit-code/integration tests.

Commit: `btakita/tsift@6ef8b2f` pushed to `origin/main`.
<!-- agent:boundary:87857922 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#e2e] Add E2E integration tests for search, graph, explain, communities, path commands
<!-- /agent:backlog -->

<!--
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
-->
