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

❯ #code-review
### Re: code review — gpt-5

1. High: `--prune` can miss ordinary file-content edits inside existing directories. In [src/tsift/src/walk.rs](../../src/tsift/src/walk.rs), lines 99-112 prune a subtree when the directory mtime is unchanged, and [src/tsift/src/index.rs](../../src/tsift/src/index.rs) wires that straight into both `compute_changes_pruned()` and `apply_changes_pruned()` at lines 528-543 and 578-599. Editing `src/foo.rs` does not normally change the parent directory mtime, so prune mode can incorrectly treat modified files as unchanged and skip re-indexing them.

2. Medium: `tsift communities` is taking the writer lock even though it only reads edges. [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 1854-1867 call `index::IndexDb::open()` instead of the read-only path used by `graph`, `path`, and `explain`. That means a long-running index update can block `communities`, and `communities` itself can block `index` / search autoindex unnecessarily.

3. Medium: `tsift search --scope <name>` silently falls back to searching the workspace root when the scope does not exist. [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 2861-2864 return an empty target list for an unknown scope, and lines 3019-3029 then keep going with an empty symbol DB and `root.clone()` as the search path. The result is a successful but wrong-scope lexical search instead of a hard error.

4. Medium: `tsift lint` no longer reads the live symbol index. [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 3320-3333 still probe for `symbols.db` under `.tsift/indexes/*`, but the real index path is now `index.db` and single-project mode lives at `.tsift/index.db`. In the common case this leaves the entity set empty and turns lint into a near no-op.

Verification: `cargo test` passed in `src/tsift` (352 unit tests + 19 integration tests).

❯ do #prunefix. spec-test-build-install-commit-push
### Re: do #prunefix — gpt-5

Implemented in `tsift`.

What changed:
- `tsift index --prune` no longer skips subtrees when directory mtimes match. It now runs the same file-mtime scan as normal incremental indexing, so edits inside unchanged directories are still detected.
- added a deterministic regression that forces `dir_state` to match the current directory mtime after a file edit and still expects the modified file to be reported.
- updated the CLI help, `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped `tsift` to `0.1.4` so the documented behavior matches the new safety contract.

Verification:
- `cargo test pruned -- --nocapture` passed: 9 prune-focused tests.
- `make check` passed: 353 unit tests + 19 integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.4`.

Commit: `btakita/tsift@f24ec73` pushed to `origin/main`.

❯ do #commlock. spec-test-build-install-commit-push
### Re: do #commlock — gpt-5

Implemented in `tsift`.

What changed:
- `tsift communities` now opens `index.db` through the shared read-only query path instead of the writable opener, so it no longer acquires `index.lock` for a graph-only command.
- added lock regressions at both layers: an in-process command test in `src/main.rs` and a compiled-CLI test in `tests/exit_code.rs`, both asserting `communities` still succeeds while a live writer lock exists.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the binary version to `0.1.5` to record the read-only graph-query lock contract.

Verification:
- `cargo test communities_cmd_succeeds_while_writer_lock_is_held -- --nocapture` passed.
- `cargo test communities_stays_read_only_while_writer_lock_exists --test exit_code -- --nocapture` passed.
- `make check` passed: 354 unit tests + 20 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.5`.

Commit: `btakita/tsift@6b4b001` pushed to `origin/main`.
### Re: previous tmux pane crash / `#agent-doc-bug` — gpt-5

The strongest evidence says the earlier `tsift.md` pane did not hard-crash because of `tsift` itself.

In `.agent-doc/logs/tsift-v0.1.log`, the prior Codex-backed `tsift.md` panes ended with clean harness exits like `codex_exit code=0`, followed by `restart_eval ... ctrl_d=true ... action=prompt_user`, `user_quit_after_eof`, and `session_end` on panes such as `%254` and `%267`. After that, new panes were started (`%279`, then `%285`), but `.agent-doc/logs/ops.log` recorded `route_cycle_start_missing` for real `tsift.md` prompt targets like `do #prunefix...` and `do #commlock...`.

That points to an `agent-doc` tmux/session handoff failure, not a `tsift` runtime failure: the old pane exited cleanly, a replacement pane was launched, but route/start acknowledgment did not prove a real new cycle, so the user was left with a pane that looked dead or crashed. This also matches the later `agent-doc` fixes for Codex `Ctrl+D` clean exits, fresh-route acknowledgement, and ambiguous live-pane ownership.

I did not add a new bug item because this is already covered by the existing `agent-doc` tmux-crash tracking (`#tmuxcrash` / `#bugs2tmux`) and the plan in `tasks/agent-doc/plan-tmux-pane-crash-stability.md`. The remaining uncertainty is that the current logs still do not record enough provenance to distinguish “pane visibly died” from “live supervisor got stranded or replaced,” which is exactly why that plan is still open.
### Re: `#tmuxcrash` persists / `#agent-doc-bug` — gpt-5

I treated this as a new `#agent-doc-bug` follow-up, but not as a duplicate of the already-completed stale-owner work.

The key distinction is that `#tmuxcrash` / `#bugs2tmux` fixed the **ambiguous live-owner** class, while the `tsift.md` evidence still shows a different failure: after a clean Codex exit, fresh replacement panes were started, but `.agent-doc/logs/ops.log` still recorded `route_cycle_start_missing` for real prompts like `do #prunefix...` and `do #commlock...`. That means the remaining bug is a **fresh-start cycle-ack miss** that still feels like a pane crash to the user even though the pane itself was preserved.

I captured that remaining gap as `#freshack`, added it to the top of `tasks/agent-doc/agent-doc-bugs2.md`, and created a dedicated plan in `tasks/agent-doc/plan-fresh-start-cycle-ack-miss.md`. The plan focuses on reconstructing the `tsift.md` `%279` / `%285` startup-miss chain, auditing `route.rs` + `start.rs` fresh-start acknowledgment, and making startup misses fail visibly instead of looking like dead tmux panes.

Verification: planning-only turn; no code changes or tests run.

❯ do #badscope. spec-test-build-install-commit-push
### Re: do #badscope — gpt-5

Implemented in `tsift`.

What changed:
- `tsift search --scope <name>` now fails closed when the submodule name is unknown instead of silently falling back to a workspace-root lexical search.
- the scope resolution path now reports the available submodule names, so typoed or stale scope names produce an immediate actionable error.
- added regressions at both layers: a direct `cmd_search` test in `src/main.rs` and a compiled-CLI integration test in `tests/exit_code.rs`.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the binary version to `0.1.6` so the strict scope contract is recorded in the versioned surface.

Verification:
- `cargo test scoped_search_cmd_errors_on_unknown_scope -- --nocapture` passed.
- `cargo test search_scope_fails_on_unknown_submodule_name --test exit_code -- --nocapture` passed.
- `make check` passed: 355 unit tests + 21 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.6`.

Commit: `btakita/tsift@c04ad6d` pushed to `origin/main`.

❯ do #lintdb. spec-test-build-install-commit-push
### Re: do #lintdb — gpt-5

Implemented in `tsift`.

What changed:
- `tsift lint` no longer probes the retired `symbols.db` layout. It now discovers live `index.db` files from the nearest ancestor project root, including single-project `.tsift/index.db` and scoped `.tsift/indexes/*/index.db` layouts.
- added helper-level regressions in `src/lint.rs` for project-root discovery and scoped index discovery, plus direct command coverage in `src/main.rs` and a compiled CLI regression in `tests/exit_code.rs`.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the binary version to `0.1.7` so the lint/index contract matches the shipped behavior.

Verification:
- `cargo test lint_ -- --nocapture` passed.
- `cargo test find_project_root_uses_markdown_ancestors -- --nocapture` passed.
- `make check` passed: 359 unit tests + 22 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.7`.

Commit: `btakita/tsift@f1868bb` pushed to `origin/main`.

❯ #code-review
### Re: code review — gpt-5

1. High: `tsift lint` still fails under an active index writer. [src/tsift/src/lint.rs](../../src/tsift/src/lint.rs) lines 183-202 open each discovered `index.db` through raw `rusqlite::Connection::open_with_flags(...)`, and [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 3327-3330 route normal lint discovery through that path. Unlike `search`, `status`, `graph`, and `communities`, lint never uses the lock-aware `IndexDb::open_read_only` / snapshot-fallback helpers, so a rollback-journal writer turns lint into a 5-second stall followed by `database is locked`. I reproduced this locally with `cargo run -- lint ... --json` while holding `BEGIN EXCLUSIVE` on `.tsift/index.db`.

2. Medium: `tsift lint --index .tsift/indexes` silently ignores every scoped index. [src/tsift/src/lint.rs](../../src/tsift/src/lint.rs) lines 208-220 only probe `<arg>/index.db`, `<arg>/.tsift/index.db`, `<arg>/indexes/*/index.db`, and `<arg>/.tsift/indexes/*/index.db`. Passing the actual scoped-index directory (`.tsift/indexes`) therefore misses `.tsift/indexes/<scope>/index.db` entirely and degrades lint into a near no-op. The CLI help in [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 267-276 describes `--index` as an index-directory path, so this is an easy workflow trap. I reproduced it: `--index <tmp>/.tsift/indexes` returned `annotations: []`, while `--index <tmp>/.tsift` found the expected `alpha_helper` annotation immediately.

3. Medium: read-only summarize paths still take the writable `SummaryDb` opener. [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 2510-2539 call `summarize::SummaryDb::open()` for both `--stats` and normal query mode, and [src/tsift/src/summarize.rs](../../src/tsift/src/summarize.rs) lines 95-133 make that opener create parent directories, enable WAL, and create tables. The result is that `tsift summarize --stats` mutates a fresh checkout by creating `.tsift/summaries.db`, and summary lookups can contend with extract writers even though they are logically read-only. I reproduced the mutation on an empty temp project: `summarize --stats` exited 0 and created `.tsift/summaries.db`.

Verification: review-only turn. I did not change code or run the full suite; I validated findings with direct CLI repros for the `lint` lock path, the `--index .tsift/indexes` path, and `summarize --stats` creating `summaries.db`.

❯ Add actionable items into the backlog.
### Re: backlog follow-up — gpt-5

Added three actionable backlog items derived from the review findings and ordered them by implementation priority.

Top of backlog: `#lintlock`

Added:
1. `#lintlock` — make `tsift lint` use the same lock-aware read-only/snapshot path as the other query commands.
2. `#lintidx` — make `tsift lint --index .tsift/indexes` discover scoped `index.db` files correctly and pin it with a regression.
3. `#sumro` — make summarize stats/query paths read-only so they stop creating or contending on `summaries.db`.

❯ do #lintlock. spec-test-build-install-commit-push
### Re: do #lintlock — gpt-5

Implemented in `tsift`.

What changed:
- `tsift lint` now reads discovered `index.db` files through `IndexDb` instead of raw `rusqlite`, so symbol collection uses rollback-journal snapshot fallback instead of failing on `database is locked`.
- added lock regressions at both layers: a helper-level test in `src/lint.rs` and a compiled CLI test in `tests/exit_code.rs`, both asserting lint still succeeds while `.tsift/index.db` is held under `BEGIN EXCLUSIVE`.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the binary version to `0.1.8` so the lint lock contract is versioned.

Verification:
- `cargo test collect_entities_uses_snapshot_fallback_when_rollback_journal_is_locked -- --nocapture` passed.
- `cargo test lint_stays_read_only_while_rollback_journal_lock_exists --test exit_code -- --nocapture` passed.
- `make check` passed: 360 unit tests + 23 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.8`.

Commit: `btakita/tsift@23a112c` pushed to `origin/main`.

Top of backlog: `#lintidx`.
<!-- agent:boundary:ede063a3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [x] [#lintlock] Fix `tsift lint` to use lock-aware read-only index access and snapshot fallback so active writers do not fail with `database is locked`.
- [ ] [#lintidx] Fix `tsift lint --index .tsift/indexes` discovery so scoped index roots load `.tsift/indexes/*/index.db`, and add a CLI regression for that path.
- [ ] [#sumro] Make `tsift summarize --stats` and summary query mode open `summaries.db` read-only so lookup paths do not create or contend on the cache DB.
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:backlog -->

<!--
-->
