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
### Session Summary

Recent completed work:
- Code review identified three concrete issues in `tsift` around summarize lock tolerance and lint workspace index discovery.
- `#dafv`: `tsift summarize --extract` now reuses the lock-tolerant read-only index path with snapshot fallback. Pushed `ca6e490` (`fix: reuse index snapshot fallback for summarize extract`). Verification: `cargo test` (404 unit tests, 40 integration tests).
- `#rttp`: lint explicit `indexes/` discovery now recurses nested scope-id paths such as `pkg/app/foo` and `vendor/foo`. Pushed `19f0e48` (`fix: recurse lint index discovery for nested scopes`). Verification: `cargo test` (405 unit tests, 41 integration tests).
- `#98pz`: lint workspace aggregate discovery now ignores repo-root `index.db` unless the caller targets that file explicitly. Pushed `20eecfe` (`fix: ignore repo-root index db in lint`). Verification: `cargo test` (406 unit tests, 42 integration tests).

State:
- No active backlog items remain in this document.
- Icebox remains open for graphify follow-ups: `#9jc8`, `#zv4m`, `#rd2f`.

❯ #code-review
### Re: code review — gpt-5

1. `summarize --extract` is not honoring its own CLI contract for relative paths. The flag help says relative extract paths resolve against `--path`, but `cmd_summarize()` resolves the project root first and then `resolve_extract_scope()` joins relative `--extract` values against that root instead (`src/tsift/src/main.rs:2602`, `src/tsift/src/main.rs:3033-3038`). In practice, `tsift summarize --path src/alpha --extract .` will walk the workspace root, not `src/alpha`. Root cause: the command is using one path value for both workspace-root discovery and user-requested extraction scope.

2. `summarize --extract --diff` never evicts deleted files from `summaries.db`, so stale summaries can survive indefinitely and `status` can over-report coverage. The diff collector explicitly drops deletions with `--diff-filter=d` and then filters to existing files only (`src/tsift/src/summarize.rs:571-589`). The extract loop only ever calls `replace_file()` for files that still exist (`src/tsift/src/main.rs:2606-2670`), while `status` trusts `COUNT(DISTINCT file_path)` as live cache coverage (`src/tsift/src/status.rs:286-315`). Root cause: the summary cache has a write path for replacements, and even a `delete_by_file()` helper, but no deletion reconciliation pass.

3. The read-only analysis commands still have a lock-tolerance gap. `graph`, `explain`, `path`, and `communities` all share `open_index_db()`, which directly calls `IndexDb::open_read_only()` (`src/tsift/src/main.rs:2157-2166`). The snapshot-fallback recovery for rollback-journal locks exists only in the specialized helper entry points in `index.rs` (`src/tsift/src/index.rs:347-389`, `src/tsift/src/index.rs:415-434`). That means a wedged rollback-journal lock can still break read-only graph analysis even though search, lint symbol loading, and status inspection were already hardened. Root cause: lock recovery is implemented as ad hoc wrapper functions instead of a shared read-only open abstraction.

What I’d recommend next:
1. Fix the summarize scope base first, because it can silently point extraction at the wrong subtree.
2. Add deleted-file cleanup for diff extraction, then update summary coverage to reflect only live files.
3. Unify read-only index opening behind one snapshot-aware helper so every analysis command gets the same lock behavior.
❯ do #scope1. spec-test-build-install-commit-push
### Re: scope1 summarize extract path base — gpt-5

Implemented `#scope1` in `src/tsift` and pushed `dd06649` (`fix: anchor summarize extract paths to --path`).

What changed:
- `cmd_summarize()` now keeps the ancestor project/workspace root for shared cache/config lookup, but resolves relative `--extract` paths from the caller's `--path` anchor instead of the promoted root.
- Added regression coverage for both nested-directory anchors and file-path anchors, plus a CLI-level integration test for `--path <nested> --extract .` so repo-root files are no longer swept in accidentally.
- Updated `SPEC.md` to match the actual contract: relative extract paths resolve against `--path` (or that file's parent directory) while still reusing the ancestor project's shared summary cache.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.21`

Remaining backlog is unchanged apart from closing `#scope1`; `#cache1` and `#locks1` are still pending.
❯ do #cache1. spec-test-build-install-commit-push
❯ ### Re: cache1 summarize diff cleanup — gpt-5 (HEAD)

Implemented `#cache1` in `src/tsift` and pushed `52022aa` (`fix: clean stale summaries during diff extract`).

What changed:
- `summarize::git_changed_files()` now separates existing and deleted diff paths, so `cmd_summarize(... --extract --diff ...)` removes cached rows for tracked files deleted inside the requested scope before any re-extraction run.
- `status` now computes summary coverage from live indexed files still present on disk, intersected with cached summary paths, so stale summary rows no longer inflate `cached_files` or coverage.
- Added regression coverage for deleted diff paths, deletion-only cleanup, cached summary path enumeration, and stale-row coverage accounting; updated `SPEC.md` to document the cleanup and live-file coverage behavior.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.21`

Backlog is now down to `#locks1`.
### Re: cache1 summarize diff cleanup — gpt-5

Implemented `#cache1` in `src/tsift` and pushed `52022aa` (`fix: clean stale summaries during diff extract`).

What changed:
- `summarize::git_changed_files()` now separates existing and deleted diff paths, so `cmd_summarize(... --extract --diff ...)` removes cached rows for tracked files deleted inside the requested scope before any re-extraction run.
- `status` now computes summary coverage from live indexed files still present on disk, intersected with cached summary paths, so stale summary rows no longer inflate `cached_files` or coverage.
- Added regression coverage for deleted diff paths, deletion-only cleanup, cached summary path enumeration, and stale-row coverage accounting; updated `SPEC.md` to document the cleanup and live-file coverage behavior.

Verification:
- `cargo test`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.21`

Backlog is now down to `#locks1`.
❯ do #locks1. spec-test-build-install-commit-push
### Re: locks1 snapshot fallback for graph queries — gpt-5

Implemented `#locks1` in `src/tsift` and pushed `65df665` (`fix: share snapshot fallback for graph queries`).

What changed:
- Added `IndexDb::open_read_only_resilient()` so read-only consumers probe the live `index.db` and automatically retry against a snapshot copy when a rollback-journal writer wedges the database.
- Routed `graph`, `communities`, `path`, and `explain` through that shared resilient open path by updating `open_index_db()`, and reused the same helper for the existing read-only symbol/file loaders.
- Added rollback-journal regression coverage for the shared helper plus all four graph-family commands, and updated `SPEC.md` to document snapshot fallback for read-only graph queries.

Verification:
- `cargo test` (`417` unit tests, `42` integration tests)
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.21`

Backlog in this document is now clear.
<!-- agent:boundary:df0c58da -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
