---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
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

❯ do #lintidx. spec-test-build-install-commit-push
### Re: do #lintidx — gpt-5

Implemented in `tsift`.

What changed:
- `tsift lint --index .tsift/indexes` now treats the scoped-index directory itself as a valid discovery root and loads `.tsift/indexes/*/index.db` correctly.
- added regressions in both the helper-level discovery path (`src/lint.rs`) and the compiled CLI surface (`tests/exit_code.rs`).
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the binary version to `0.1.9` so the explicit scoped-index contract is versioned.

Verification:
- `cargo test collect_entities_from_explicit_indexes_dir -- --nocapture` passed.
- `cargo test lint_accepts_explicit_indexes_dir --test exit_code -- --nocapture` passed.
- `make check` passed: 361 unit tests + 24 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.9`.

Commit: `btakita/tsift@bfb4df2` pushed to `origin/main`.

Top of backlog: `#sumro`.

I’m treating this as an `agent-doc` cycle for the `tsift` session document. First I’m locating the backing markdown file and running the required `agent-doc` preflight so I can respond and persist through the normal commit boundary.
`agent-doc` is newer than the embedded skill version, so I’m running the required skill reload step before normal preflight. If it requests a compaction reload, I have to stop there instead of continuing the cycle.
The reload path is clear, so I’m moving to `preflight` on [tasks/software/tsift.md](/home/brian/work/btakita/agent-loop/tasks/software/tsift.md). That will tell me whether there’s a real diff, whether any claims or built-in commands need handling first, and which baseline file must be used for the final write.
`preflight` found one real user request: `do #sumro. spec-test-build-install-commit-push`. I’m inspecting the summarize path and current repo state in `src/tsift`, then I’ll patch the read-only open path, run the full suite plus build/install, and only then write the response back through `agent-doc finalize`.
I’ve confirmed the bug surface: `cmd_summarize` still calls the writable `SummaryDb::open()` for both `--stats` and lookup mode, which creates `.tsift/summaries.db` and opts into WAL even when the command is supposed to be read-only. I’m checking the existing tests and docs next so I can patch the behavior and pin it with regressions instead of only swapping the opener.
I have enough context to patch this cleanly.

Plan:
1. Change `cmd_summarize` so `--stats` and lookup mode fail closed when `.tsift/summaries.db` is absent and otherwise open it through `SummaryDb::open_read_only()`.
2. Add regressions for both user-visible failures this review called out: `summarize --stats` must not create the DB, and read-only summarize commands must still work while an extract writer holds the summaries DB open.
3. Update the versioned surfaces (`SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, crate version) to record the new read-only summary-cache contract.
4. Run the requested verification in `src/tsift`: full `make check`, `cargo build`, `cargo install --path .`, then commit. I’ll attempt `git push`, but network restrictions may block that step.
I’m at the edit step now. The fix is small in code, but I’m also tightening the contract around it: `--stats` on a missing cache should error instead of creating `.tsift/summaries.db`, and compiled CLI coverage will assert that summary reads remain non-mutating.
I’m patching four files in `src/tsift`: the summarize command path, compiled CLI regressions, and the versioned docs/version metadata. After that I’ll run the targeted summarize tests first, then the full requested suite.
The code and versioned surface are patched. I’m running the summarize-specific regressions now to catch compile or contract issues before the full `make check` / build / install cycle.
The targeted test run hit Cargo’s package/artifact locks because I launched three compilations in parallel. I’m letting one finish and then I’ll serialize the rest; the change itself is still localized to the summarize path.
The regressions are passing. I’m on the requested full verification cycle now in `src/tsift`: `make check`, then `cargo build`, then `cargo install --path .`, and after that I’ll commit and attempt the requested push.
The full verification step also serialized itself behind Cargo locks, and `cargo install` is already at the dependency/update phase. I’m waiting this through before I decide whether I need to rerun anything serially; I’m not changing files further unless verification says I have to.
The full requested verification is green: `make check` passed with 362 unit tests and 26 integration tests, `cargo build` succeeded, and `cargo install --path .` replaced the local binary with `tsift v0.1.10`. I’m doing a quick diff review now, then I’ll commit in the `src/tsift` submodule and see whether `git push` is possible from this environment.
The diff is clean. One expected extra file changed is `src/tsift/Cargo.lock`, reflecting the crate version bump to `0.1.10`; I’m including that in the submodule commit.
The code/test/build/install part is finished. The only blocker now is Git metadata: committing inside the `src/tsift` submodule failed because the submodule gitdir under `.git/modules/src/tsift` is mounted read-only in this environment, so I can’t create the Git index lock needed for `git commit` or any subsequent push.
I’m at the document write-back step now. I’m checking how this repo expects `agent:backlog` patches to be expressed so I can mark `#sumro` correctly without bypassing `agent-doc`’s component rules.
`finalize` was blocked by `agent-doc`’s pane-ownership guard, not by the response content. The session is still owned by pane `%308` while this harness is on `%309`, so I’m reclaiming the document with `agent-doc claim` and then I’ll rerun the normal write/commit path instead of patching the file directly.
The reclaim worked. `preflight` reaped `#sumro` from the live backlog during recovery, so I only need to write the exchange response now and then run `session-check` immediately.
The response content is in the document now, but `agent-doc` cannot cross its commit boundary because this environment also blocks writes to the root repo’s `.git/index.lock`. I’m verifying the exact session state once so I can report the real blocker precisely instead of guessing.
Implemented locally in `tsift`: [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs), [src/tsift/tests/exit_code.rs](/home/brian/work/btakita/agent-loop/src/tsift/tests/exit_code.rs), plus the versioned docs/version bump in [src/tsift/SPEC.md](/home/brian/work/btakita/agent-loop/src/tsift/SPEC.md), [src/tsift/CLAUDE.md](/home/brian/work/btakita/agent-loop/src/tsift/CLAUDE.md), [src/tsift/VERSIONS.md](/home/brian/work/btakita/agent-loop/src/tsift/VERSIONS.md), and [src/tsift/Cargo.toml](/home/brian/work/btakita/agent-loop/src/tsift/Cargo.toml). `tsift summarize --stats`, `tsift summarize <symbol>`, and `tsift summarize --file <path>` now open `summaries.db` read-only and fail closed when the cache is missing. I added regressions for missing-cache failure and read-only-cache lookup, and `tsift --version` now reports `0.1.10`.

Verification is green: the targeted summarize tests passed, `make check` passed with 362 unit tests and 26 integration tests, `cargo build` passed, and `cargo install --path .` succeeded.

Two blockers remain, both caused by this environment mounting Git metadata read-only:
- `git commit` in `src/tsift` failed on `.git/modules/src/tsift/index.lock`, so there is no new `btakita/tsift@...` commit or push.
- `agent-doc finalize` wrote the response into [tasks/software/tsift.md](/home/brian/work/btakita/agent-loop/tasks/software/tsift.md) and reaped `#sumro`, but it could not cross the document commit boundary because root `.git/index.lock` is also read-only. `agent-doc session-check tasks/software/tsift.md` currently fails with cycle state `response_captured`.

Once Git writes are available again, the recovery path is:
```bash
agent-doc preflight tasks/software/tsift.md
```
That should replay/close the open document cycle, after which you can commit/push `src/tsift` normally.
❯ do #sumro. spec-test-build-install-commit-push
### Re: do #sumro — gpt-5

Implemented in `tsift`.

What changed:
- `tsift summarize --stats`, `tsift summarize <symbol>`, and `tsift summarize --file <path>` now share a read-only summary-cache opener that fails closed when `.tsift/summaries.db` is missing, so lookup paths no longer create or contend on the cache DB.
- added regressions in both the direct command path (`src/main.rs`) and the compiled CLI surface (`tests/exit_code.rs`), including missing-cache failure and read-only-cache lookup coverage.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, bumped the crate version to `0.1.10`, and refreshed the installed binary.

Verification:
- `cargo test summarize_stats_fails_closed_when_cache_missing -- --nocapture` passed.
- `cargo test summarize_stats_fails_closed_when_cache_missing --test exit_code -- --nocapture` passed.
- `cargo test summarize_symbol_query_accepts_read_only_cache_permissions --test exit_code -- --nocapture` passed.
- `make check` passed: 362 unit tests + 26 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.10`.

Commit/push:
- blocked in this environment. `git commit` in `src/tsift` failed with `fatal: Unable to create '/home/brian/work/btakita/agent-loop/.git/modules/src/tsift/index.lock': Read-only file system`, so no new `btakita/tsift@...` commit or push was produced.

Top of backlog: `#tmv7`.

❯ Error when running agent-doc on /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md
```
• Ran tsift search "build-documentation"
  └ Error: autoindexing index

    … +12 lines (ctrl + t to view transcript)
    index.db (lock: /home/brian/work/btakita/agent-loop/src/session-share/.tsift/index.lock). A concurrent `tsift
    index` or `tsift search --autoindex` is already updating this index; wait for it to finish before retrying.
```
### Re: lock error in `claudescore-3.md` — gpt-5

That failure is the expected fast-fail path for a concurrent index writer, not evidence of index corruption.

I checked the live `src/session-share` state just now:
- `tsift locks .` reports `lock: absent` and `journal: absent`
- `tsift status .` reports `index: fresh` with 748 tracked files

So the most likely sequence is that another `tsift index` or `tsift search --autoindex` was already running when that `agent-doc` session searched, and the writer finished afterward.

What to do when it happens:
- retry the search after the active writer finishes
- if it keeps recurring, run `tsift locks /home/brian/work/btakita/agent-loop/src/session-share` while the failure is live; that will distinguish a live writer from a stale sidecar and print the recommended next step

I am not adding a new `tsift` backlog item from this single repro because the current state does not show a stale lock or failed recovery path. If this starts happening without overlapping index work, that would justify a new concurrency/UX bug.

❯ #code-review
### Re: code review — gpt-5

1. High: `tsift summarize --extract <relative>` ignores `--path` for the actual file walk. In [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2397), `cmd_summarize()` canonicalizes the project root from `--path` and even computes `extract_scope`, but the non-`--diff` branch still calls `collect_source_files(&extract_path)` at [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2414). That means `tsift summarize --path /repo --extract src/` walks `src/` relative to the caller's current working directory, not `/repo/src`. From outside the repo it can silently extract nothing or extract the wrong tree, while still writing into `/repo/.tsift/summaries.db`.

2. Medium: workspace summarize extraction uses an arbitrary scoped symbol index, so symbol context is nondeterministic and can come from the wrong submodule. [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2791) returns the first `index.db` it sees under `.tsift/indexes/*`, and [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2404) reuses that one DB for every file being summarized. Then [src/tsift/src/summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:411) loads symbols with `WHERE file LIKE '%' || ?1` at [src/tsift/src/summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:431), which suffix-matches any same-named file. In a workspace with multiple `src/lib.rs` or `main.rs` files, summarize extraction can pull symbols from the wrong module, and which module wins depends on `read_dir()` order.

Verification: review-only turn. I did not run the suite.

❯ #next-steps
### Re: next steps — gpt-5

Added the two actionable follow-up items from the review and put them ahead of the longer-term roadmap items.

Top of backlog: `#sumpath`

Added:
1. `#sumpath` — fix `tsift summarize --extract <relative>` so the walked extraction path resolves against `--path` / project root instead of the caller's CWD.
2. `#sumscope` — fix workspace summarize extraction so symbol context comes from the correct scoped index instead of an arbitrary first `index.db` plus suffix-based file matching.

I did not add more than these two because they cover the concrete correctness gaps from the review without turning the backlog into duplicated implementation detail.

❯ do #sumpath. spec-test-build-install-commit-push
### Re: do #sumpath — gpt-5

Implemented in `tsift`.

What changed:
- `tsift summarize --extract <relative>` now resolves the walked extraction path against `--path` / the canonical project root before the non-`--diff` file walk, so extraction no longer depends on the caller's current working directory.
- added regressions in both the direct summarize path (`src/main.rs`) and the compiled CLI surface (`tests/exit_code.rs`), including an out-of-tree CLI repro that would previously return `No files to extract.` from the wrong `src/`.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, bumped the crate version to `0.1.11`, and refreshed the installed binary.

Verification:
- `cargo test summarize_extract -- --nocapture` passed.
- `make check` passed: 363 unit tests + 27 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.11`.

Commit/push:
- `btakita/tsift@1c3964a` (`fix: harden summarize cache and extract paths`) pushed to `origin/main`.
- that commit also carried the already-verified local summarize cache hardening changes that had been left uncommitted from the prior blocked turn.

Top of backlog: `#sumscope`.

❯ do #sumscope. spec-test-build-install-commit-push
### Re: do #sumscope — gpt-5

Implemented in `tsift`.

What changed:
- `tsift summarize --extract` now resolves symbol context per extracted file, so workspace files use the matching scoped `index.db` instead of an arbitrary first workspace DB.
- summarize symbol preload now uses exact normalized file-path matches instead of suffix matching, preventing duplicate `src/lib.rs`-style paths from bleeding across scopes.
- added regressions in both direct helper paths (`src/main.rs`, `src/summarize.rs`) and the compiled CLI surface (`tests/exit_code.rs`), including a mocked Anthropic response + captured prompt proving `src/beta/src/lib.rs` only receives beta symbols.
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, bumped the crate version to `0.1.12`, and refreshed the installed binary.

Verification:
- `cargo test summarize_ -- --nocapture` passed.
- `make check` passed: 365 unit tests + 28 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.12`.

Commit/push:
- `btakita/tsift@e40793d` (`fix: scope summarize extraction symbol context`) pushed to `origin/main`.

Top of backlog: `#tmv7`.

❯ #code-review
❯ #code-review
### Re: code review — gpt-5

1. High: workspace scope identity is not unique, so different submodules with the same trailing directory name collide onto the same scope and index DB. In [src/tsift/src/config.rs](../../src/tsift/src/config.rs) lines 88-104, `submodule_dirs()` derives the scope name from `Path::file_name()`, and line 84-85 maps that name directly to `.tsift/indexes/<name>/index.db`. That name is then trusted by workspace indexing and search in [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 1367-1388 and 2864-2908. I reproduced it with `.gitmodules` entries `pkg/app/foo` and `vendor/foo`: `tsift index --workspace` ran two `[foo]` passes against the same DB path, and `tsift search --scope foo vendor_only` resolved back to the first `foo` root instead of the second module.

2. Medium: `tsift status` reports `index: missing` for a workspace that was successfully indexed with scoped DBs only. [src/tsift/src/status.rs](../../src/tsift/src/status.rs) lines 93-98 hard-code status to `.tsift/index.db` and `.tsift/summaries.db`, and lines 110-113 immediately return `Missing` when the root DB is absent. I reproduced this by indexing a temp workspace with `tsift index --workspace <tmp>`; both submodule indexes were created, but `tsift status <tmp> --json` still returned `{"index":{"state":"missing"}}` plus the misleading recommendation to run `tsift init && tsift index .`.

3. Medium: `tsift index --submodule <name>` accepts unknown names and indexes `root/<name>` anyway, creating a scoped DB that later commands refuse to use. In [src/tsift/src/main.rs](../../src/tsift/src/main.rs) lines 1367-1373, the `--submodule` target falls back to `root.join(name)` when the name is not present in `.gitmodules`. But search resolves scopes only through configured submodules in lines 2864-2892. I reproduced this with a workspace that only configured `alpha`: `tsift index --submodule nope <tmp>` created `.tsift/indexes/nope/index.db`, then `tsift search stray --path <tmp> --scope nope` failed with `unknown scope \`nope\`. Available scopes: alpha`.

Verification: review-only turn. I validated findings with direct `cargo run -- index/search/status ...` repros in temporary workspaces.

Top of backlog: `#scopeid`.

❯ do #scopeid. spec-test-build-install-commit-push
### Re: do #scopeid — gpt-5

Implemented in `tsift`.

What changed:
- workspace scope ids are now collision-free. Unique submodules keep the short leaf name, but duplicate trailing names promote to the full `.gitmodules` path such as `pkg/app/foo` and `vendor/foo`, so workspace indexing and scoped search no longer share one `index.db`.
- duplicate leaf-name selectors now fail closed as ambiguous and list the concrete scope ids to use, instead of silently resolving to the first matching submodule.
- threaded the new `WorkspaceScope` model through workspace index/search/query paths and updated the versioned docs (`SPEC.md`, `CLAUDE.md`, `VERSIONS.md`).
- added regressions in both the in-process command tests and the compiled CLI surface for duplicate-leaf workspaces.

Verification:
- `cargo test submodule_dirs_use_full_path_when_leaf_names_collide -- --nocapture` passed.
- `cargo test scoped_search_cmd_errors_on_ambiguous_legacy_scope_name -- --nocapture` passed.
- `cargo test search_scope_errors_on_ambiguous_duplicate_leaf_name --test exit_code -- --nocapture` passed.
- `make check` passed: 369 unit tests + 29 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.13`.

Commit/push:
- `btakita/tsift@7bb0585` (`fix: uniquify workspace scope identifiers`) pushed to `origin/main`.

Top of backlog: `#statusws`.
### Re: do #scopeid — gpt-5

Implemented in `tsift`.

What changed:
- workspace scope ids are now collision-free. Unique submodules keep the short leaf name, but duplicate trailing names promote to the full `.gitmodules` path such as `pkg/app/foo` and `vendor/foo`, so workspace indexing and scoped search no longer share one `index.db`.
- duplicate leaf-name selectors now fail closed as ambiguous and list the concrete scope ids to use, instead of silently resolving to the first matching submodule.
- threaded the new `WorkspaceScope` model through workspace index/search/query paths and updated the versioned docs (`SPEC.md`, `CLAUDE.md`, `VERSIONS.md`).
- added regressions in both the in-process command tests and the compiled CLI surface for duplicate-leaf workspaces.

Verification:
- `cargo test submodule_dirs_use_full_path_when_leaf_names_collide -- --nocapture` passed.
- `cargo test scoped_search_cmd_errors_on_ambiguous_legacy_scope_name -- --nocapture` passed.
- `cargo test search_scope_errors_on_ambiguous_duplicate_leaf_name --test exit_code -- --nocapture` passed.
- `make check` passed: 369 unit tests + 29 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.13`.

Commit/push:
- `btakita/tsift@7bb0585` (`fix: uniquify workspace scope identifiers`) pushed to `origin/main`.

Top of backlog: `#statusws`.

❯ do #statusws. spec-test-build-install-commit-push
do #statusws. spec-test-build-install-commit-push
### Re: do #statusws — gpt-5

Implemented in `tsift`.

What changed:
- `tsift status` now detects workspace-only indexes under `.tsift/indexes/<scope>/index.db` instead of reporting `index: missing` whenever the root `.tsift/index.db` is absent.
- workspace status now aggregates scoped freshness into the top-level `index` state, reports the contributing scopes in both human and JSON output, and recommends `tsift index --workspace .` / `tsift init --workspace` for workspace roots.
- added regressions in both the direct status helpers (`src/status.rs`) and the compiled CLI surface (`tests/exit_code.rs`).
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the crate version to `0.1.14`.

Verification:
- `cargo test status_workspace --manifest-path src/tsift/Cargo.toml -- --nocapture` passed.
- `cargo test status_reports_workspace_scoped_indexes_in_json --test exit_code --manifest-path src/tsift/Cargo.toml -- --nocapture` passed.
- `make check` passed: 372 unit tests + 30 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.14`.

Commit/push:
- the resulting `tsift` changes are already present in `btakita/tsift@61bd2f2` (`chore: commit workspace changes`) on `origin/main`; `git commit` had nothing left to record and `git push origin main` reported `Everything up-to-date`.

❯ Top of backlog: `#substrict`. 

❯ do #substrict. spec-test-build-install-commit-push
### Re: do #substrict — gpt-5

Implemented in `tsift`.

What changed:
- `tsift index --submodule <name>` now uses the same strict workspace scope resolution as `--scope`, so unknown selectors fail closed instead of indexing `root/<name>` into an unreachable scoped database.
- ambiguous duplicate leaf-name selectors now fail closed for submodule indexing too, requiring the concrete scope id when `.gitmodules` contains colliding leaf names.
- added regressions in both the direct command path (`src/main.rs`) and the compiled CLI surface (`tests/exit_code.rs`).
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, bumped the crate version to `0.1.15`, and refreshed the installed binary.

Verification:
- `cargo test workspace_index_single_submodule_errors_on_unknown_scope -- --nocapture` passed.
- `cargo test index_submodule_fails_on_unknown_submodule_name --test exit_code -- --nocapture` passed.
- `make check` passed: 373 unit tests + 31 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.15`.

Commit/push:
- `btakita/tsift@ab7f349` (`fix: fail closed unknown submodule indexing`) pushed to `origin/main`.

Top of backlog: `#tmv7`.

❯ #code-review
### Re: code review — gpt-5

1. High: `tsift status` can report a partially indexed workspace as fully fresh. In [status.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/status.rs:162), `check_workspace_index()` skips any configured scope whose `index.db` is missing (lines 165-168) and then computes freshness only from the remaining scopes (lines 191-220). I reproduced this with a workspace containing `alpha` and `beta`, after indexing only `alpha` via `tsift index --submodule alpha <root>`: `tsift status --json <root>` returned `{"state":"fresh"}` with only one `workspace_scopes` entry and no warning that `beta` had never been indexed.

2. High: workspace-only indexes are still unusable by default for `graph`, `explain`, `path`, and `communities`. [main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:1952) routes those commands through `open_index_db()`, which falls back to `root/.tsift/index.db` whenever `--scope` is omitted (lines 1956-1969); `graph` also duplicates the same root-only fallback in [main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:1658). After `tsift index --workspace <root>`, `tsift graph alpha <root>` still fails with `no index found at .../.tsift/index.db` unless the caller already knows the right scope to pass.

3. Medium: `tsift summarize --extract ... --diff` ignores untracked new files. In [main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2405), diff extraction takes its file list entirely from `summarize::git_changed_files()`; [summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:599) implements that as `git diff --name-only HEAD`, which excludes untracked paths. I reproduced this in a repo with a committed `src/main.rs` plus a new untracked `src/new.rs`: `tsift summarize --extract src --diff --path <root>` returned `No files to extract.` instead of attempting to summarize the new file.

Added three recommended backlog items from these findings and ordered them by priority.

Top of backlog: `#wspart`.

Added:
1. `#wspart` — `[recommended]` status: report missing workspace scopes instead of false `fresh` when some configured scoped indexes are absent.
2. `#wsquery` — `[recommended]` workspace queries: make `graph` / `explain` / `path` / `communities` workspace-aware by default, or fail with an explicit scope-selection error instead of a missing root DB.
3. `#sumdiff` — `[recommended]` summarize diff mode: include untracked files, not just `git diff --name-only HEAD`.

Verification: review-only turn. I validated the findings with direct CLI repros against the current `tsift` binary; I did not change code or run the full suite.

❯ do #wspart. spec-test-build-install-commit-push
### Re: do #wspart — gpt-5

Implemented in `tsift`.

What changed:
- `tsift status` no longer drops configured workspace scopes whose `.tsift/indexes/<scope>/index.db` is missing. Fully unindexed workspaces still report `index: missing`, but partially indexed workspaces now report `index: stale` and include explicit `missing_scopes` data in both human and JSON output.
- workspace recommendations now distinguish missing scopes from stale files, so `status` can tell the user to run `tsift index --workspace .` with messages like `(1 missing scope)` instead of pretending the workspace is fresh.
- added regressions in both the direct status helpers (`src/status.rs`) and the compiled CLI status surface (`tests/exit_code.rs`).
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, bumped the crate version to `0.1.16`, and refreshed the installed binary.

Verification:
- `cargo test status_workspace --manifest-path src/tsift/Cargo.toml -- --nocapture` passed.
- `cargo test status_reports_partially_indexed_workspace_as_stale_with_missing_scopes_in_json --test exit_code --manifest-path src/tsift/Cargo.toml -- --nocapture` passed.
- `make check` passed: 374 unit tests + 32 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.16`.

Commit/push:
- `btakita/tsift@645f803` (`fix: report missing workspace status scopes`) pushed to `origin/main`.

Top of backlog: `#wsquery`.

❯ do #wsquery. spec-test-build-install-commit-push
### Re: do #wsquery — gpt-5

Implemented in `tsift`.

What changed:
- `tsift graph`, `tsift communities`, `tsift path`, and `tsift explain` now fail closed on workspace roots that only have scoped `.tsift/indexes/<scope>/index.db` files. Instead of blaming a missing root `.tsift/index.db`, the shared query resolver now requires `--scope <scope>` and lists both the available and currently indexed scope ids.
- `cmd_graph` now shares the same read-only query opener as `communities`, `path`, and `explain`, so all four commands enforce the same workspace-scope contract.
- added regressions in both the direct command path (`src/main.rs`) and the compiled CLI surface (`tests/exit_code.rs`), updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the crate version to `0.1.17`.

Verification:
- `cargo test graph_cmd_requires_scope_for_workspace_root_without_shared_index -- --nocapture` passed.
- `cargo test workspace_graph_queries_require_scope_without_shared_root_index --test exit_code -- --nocapture` passed.
- `make check` passed: 378 unit tests + 33 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.17`.

Commit/push:
- `btakita/tsift@c58a465` (`fix: require scope for workspace graph queries`) pushed to `origin/main`.

Top of backlog: `#sumdiff`.

❯ do #sumdiff. spec-test-build-install-commit-push
### Re: do #sumdiff — gpt-5

Implemented in `tsift`.

What changed:
- `tsift summarize --extract <path> --diff` now includes untracked files within the requested extract scope, and it filters deleted tracked paths before the extraction walk.
- added regressions in both the helper-level summarize diff path (`src/main.rs`, `src/summarize.rs`) and the compiled CLI surface (`tests/exit_code.rs`).
- updated `SPEC.md`, `CLAUDE.md`, `VERSIONS.md`, and bumped the crate version to `0.1.18`.

Verification:
- `cargo test summarize_diff_extract_includes_untracked_files -- --nocapture` passed.
- `make check` passed: 379 unit tests + 34 exit-code/integration tests.
- `cargo build` passed.
- `cargo install --path .` passed, and `tsift --version` now reports `0.1.18`.

Commit/push:
- `btakita/tsift@595b195` (`fix: include untracked summarize diff files`) pushed to `origin/main`.

Top of backlog: `#tmv7`.

❯ #code-review
### Re: code review — gpt-5

1. High: most query/status commands still treat the caller's current subdirectory as a brand-new project root instead of resolving the ancestor that already owns `.tsift/`. [`cmd_search()`](../../src/tsift/src/main.rs) canonicalizes `--path` directly at lines 3048-3052, [`cmd_status()`](../../src/tsift/src/main.rs) does the same at lines 2666-2670, and [`cmd_graph()`](../../src/tsift/src/main.rs) plus [`open_index_db()`](../../src/tsift/src/main.rs) repeat the pattern at lines 1658-1662 and 1981-1985. That means running from `repo/src/` ignores `repo/.tsift` and can synthesize `repo/src/.tsift/index.db` instead. I reproduced it in a temp repo: after indexing the repo root, `tsift status <root>/src --json` reported `{"index":{"state":"missing"}...}` and `tsift search alpha_helper --path <root>/src --json` created `<root>/src/.tsift/index.db`. [`lint::find_project_root_for_path()`](../../src/tsift/src/lint.rs) lines 162-177 already has the ancestor-walk logic the rest of the CLI is missing.

2. High: workspace status hides missing scoped indexes whenever a shared root index happens to exist. In [`status::check_index()`](../../src/tsift/src/status.rs) lines 138-175, the presence of `.tsift/index.db` short-circuits all workspace inspection, so [`check_workspace_index()`](../../src/tsift/src/status.rs) lines 177-240 never runs. I reproduced this with a workspace containing `alpha` and `beta`: after `tsift index <root>` and then `tsift index --submodule alpha <root>`, `tsift status <root> --json` still returned `{"index":{"state":"fresh","total_files":2,...}}` with no `workspace_scopes` or `missing_scopes`, even though the `beta` scoped index had never been built.

3. Medium: default search on a workspace that only has scoped indexes silently creates a second shared-root index layout. [`resolve_search_index_targets()`](../../src/tsift/src/main.rs) lines 2928-2934 always falls back to `<root>/.tsift/index.db` when neither `--scope` nor `--federated` is set, and [`precheck_search_indexes()`](../../src/tsift/src/main.rs) lines 2975-2980 autoindexes any missing target. The result is that `tsift search ... --path <workspace-root>` mutates a workspace that was intentionally indexed via `.tsift/indexes/*/index.db`, instead of failing closed like `graph`/`path`/`communities` now do. I reproduced it in a temp workspace: after `tsift index --workspace <root>`, `<root>/.tsift/index.db` did not exist; one plain `tsift search alpha_fn --path <root> --json` created it.

Verification: review-only turn. I validated the findings with direct CLI repros in temporary repos; I did not change code or run the full suite.
<!-- agent:boundary:eab852f3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#tmv7] Release workflow: crates.io + GitHub Releases with prebuilt binaries
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:backlog -->

<!--
-->
