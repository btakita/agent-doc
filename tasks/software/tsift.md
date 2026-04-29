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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260429-055011.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)

❯ #code-review
### Re: code review — gpt-5

1. `src/tsift/src/summarize.rs:430-449` opens the symbols DB directly during `tsift summarize --extract` instead of going through the lock-tolerant `IndexDb` read helpers. That loses the busy-timeout and snapshot-fallback behavior already used by `search`, `status`, and `lint`, so a concurrent index/update can fail extraction with `database is locked` instead of degrading cleanly. The root cause is the second SQLite read stack in `load_symbols_for_file`.

2. `src/tsift/src/lint.rs:256-300` only looks one directory below `.tsift/indexes` when discovering index DBs. That breaks workspaces that intentionally use nested scope ids for duplicate leaf names: the indexing path writes `.tsift/indexes/pkg/app/foo/index.db` and `.tsift/indexes/vendor/foo/index.db` (`src/tsift/src/main.rs:4459-4488`), but lint discovery only probes `.tsift/indexes/<child>/index.db`. The result is silent under-reporting from `tsift lint --index .tsift/indexes` and any other aggregate caller.

3. `src/tsift/src/lint.rs:215-219` treats both `root/index.db` and `root/.tsift/index.db` as workspace symbol sources, even though the rest of the codebase resolves the root index only at `.tsift/index.db` (`src/tsift/src/main.rs:2122-2124`, `src/tsift/src/status.rs:146-160`). On repos that already carry an application database named `index.db`, lint can ingest unrelated symbols or fail against a non-tsift schema. This looks like an accidental alternate namespace local to lint rather than an intentional compatibility surface.

Residual risk: I did not find regression coverage for duplicate-leaf scope lint discovery, so finding 2 looks unguarded today.

Verification: `cargo test -q` passed locally (403 unit tests, 40 integration tests).
❯ do #dafv. spec-test-commit-push
❯ ### Re: #dafv spec-test-commit-push — gpt-5 (HEAD)

Implemented the summarize extract lock-tolerance fix in `src/tsift` and pushed it on `main` as `ca6e490` (`fix: reuse index snapshot fallback for summarize extract`).

Changes:
- `src/summarize.rs` now routes symbol preload through `IndexDb::file_symbols_read_only(...)` instead of opening SQLite directly.
- `src/index.rs` adds the shared file-symbol read helper with the same busy-timeout plus snapshot fallback path used by other read-only index consumers, and it now preserves lock errors through the `sqlite_master` probe so the retry path actually triggers.
- `src/summarize.rs` adds a rollback-journal lock regression test, and `SPEC.md` now documents that `tsift summarize --extract` inherits the snapshot fallback behavior during symbol preload.

Verification:
- `cargo test` (404 unit tests, 40 integration tests)

Push:
- `origin/main` -> `ca6e490`
### Re: #dafv spec-test-commit-push — gpt-5

Implemented the summarize extract lock-tolerance fix in `src/tsift` and pushed it on `main` as `ca6e490` (`fix: reuse index snapshot fallback for summarize extract`).

Changes:
- `src/summarize.rs` now routes symbol preload through `IndexDb::file_symbols_read_only(...)` instead of opening SQLite directly.
- `src/index.rs` adds the shared file-symbol read helper with the same busy-timeout plus snapshot fallback path used by other read-only index consumers, and it now preserves lock errors through the `sqlite_master` probe so the retry path actually triggers.
- `src/summarize.rs` adds a rollback-journal lock regression test, and `SPEC.md` now documents that `tsift summarize --extract` inherits the snapshot fallback behavior during symbol preload.

Verification:
- `cargo test` (404 unit tests, 40 integration tests)

Push:
- `origin/main` -> `ca6e490`
❯ do #rttp. spec-test-commit-push
### Re: #rttp spec-test-commit-push — gpt-5

Implemented nested lint index discovery for duplicate-leaf workspace scopes and pushed `src/tsift` `main` to `19f0e48` (`fix: recurse lint index discovery for nested scopes`).

Changes:
- `src/lint.rs` now recursively walks explicit `indexes/` trees, so exported aggregate dirs still find `index.db` files under nested scope ids like `pkg/app/foo` and `vendor/foo`.
- Added unit coverage for nested explicit index discovery and a CLI regression that lints against an exported `indexes/` tree with duplicate-leaf scopes.
- Updated `SPEC.md` to document recursive `indexes/**/index.db` discovery for lint.

Verification:
- `cargo test` (405 unit tests, 41 integration tests)

Push:
- `origin/main` -> `19f0e48`
❯ do #98pz. spec-test-commit-push
### Re: #98pz spec-test-commit-push — gpt-5

Implemented `#98pz` in `src/tsift` and pushed `main` to `20eecfe` (`fix: ignore repo-root index db in lint`).

Changes:
- `src/lint.rs` no longer treats `root/index.db` as part of workspace aggregate lint discovery; workspace targets now only read `.tsift`-owned indexes unless the caller explicitly passes another `index.db` path.
- Added a unit regression proving workspace-root entity collection ignores an unrelated repo-root SQLite file.
- Added a CLI regression proving `tsift lint README.md --json` still succeeds in a workspace even when `./index.db` exists with unrelated schema.
- Updated `SPEC.md` to document that aggregate lint discovery ignores repo-root `index.db` unless it is targeted explicitly.

Verification:
- `cargo test` (406 unit tests, 42 integration tests)

Push:
- `origin/main` -> `20eecfe`
<!-- agent:boundary:3b2d4c09 -->
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
