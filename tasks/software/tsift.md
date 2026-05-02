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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260501-215410.md`*

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ #code-review
### Re: code review — gpt-5

1. [high] The advertised read-only recovery path is wired to rollback-journal files, but both live databases are forced into WAL mode. `IndexDb::open` and `SummaryDb::open` both execute `PRAGMA journal_mode = WAL` ([src/index.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/index.rs:252), [src/summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:215)), yet `should_retry_read_only_with_snapshot` only triggers when `*.db-journal` exists ([src/index.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/index.rs:1225), [src/summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:508)). The user-facing lock/recovery messaging is built on the same assumption ([src/status.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/status.rs:525), [src/status.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/status.rs:556)). In practice that means the fallback path is disconnected from the journal mode the writer actually uses, so a real WAL-era lock will bypass the recovery branch entirely. Even worse, the fallback copy only snapshots the main `.db` file ([src/index.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/index.rs:421), [src/summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:484)), which drops uncheckpointed `-wal` state. Net effect: under real contention the code can either fail instead of recovering or recover from a stale/incomplete snapshot.

2. [medium] `tsift status` emits a hardcoded summarize follow-up that is wrong for any repo not organized under `src/`. In the fresh-index branch, both the partial-cache and no-cache recommendations unconditionally return `tsift summarize --extract src/` ([src/status.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/status.rs:429)). That works for this repo, but it misguides root-level projects (`main.rs`, `lib.rs`) and non-`src/` workspace layouts even though indexing itself supports arbitrary roots. The workflow regression is concrete: following the printed command can produce `No files to extract.` while valid source files exist elsewhere, so the status surface stops being trustworthy.

3. [medium] The current tests mask the WAL mismatch instead of covering it. The recovery tests manufacture DELETE-mode rollback journals (`tests/exit_code.rs` uses `PRAGMA journal_mode=DELETE` plus `*.db-journal`, [tests/exit_code.rs](/home/brian/work/btakita/agent-loop/src/tsift/tests/exit_code.rs:12)) and the unit tests do the same by writing fake `-journal` sidecars ([src/index.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/index.rs:2169), [src/summarize.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/summarize.rs:1705)). That is why the suite stays green even though production opens the databases in WAL mode. Until there is a regression test that exercises an actual WAL-backed live writer plus read-only fallback, finding 1 is likely to regress again.

What I'd recommend:
1. Make recovery/diagnostics WAL-aware end-to-end: detect `-wal`/`-shm` state or use SQLite backup APIs, and update `status` messaging to describe the real lock mode.
2. Derive summarize recommendations from the indexed roots or workspace scopes instead of hardcoding `src/`.
3. Add an integration test that reproduces a live WAL writer and proves read-only status/summarize commands still return consistent data.

`cargo test -q` passed locally (`441` unit tests, `50` integration tests).
❯ do [#pc11]. spec-test-build-install-commit-push
Closed `#sjwj` and `#mrunfix` in the session document.

I added a focused regression in [src/run.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/src/run.rs:580) that exercises the direct template run path against the `monsterrodholders.md` backlog shape, including the legacy alias-tag items `[#ss01]` and `[#wpmem]`. That validates the already-shipped run-path fix from `73f27e8` with a more realistic repro surface. The `src/agent-doc` commit is `d5d6b01` (`test: cover monsterrodholders run backlog normalization`), pushed to `origin/20260422/codex-agent-backend`.

Verification passed: `agent-doc preflight src/boost-client/tasks/monsterrodholders.md` reported `no_changes: true`, and `cargo test apply_template_response_normalizes_monsterrodholders_style_backlog_patch`, full `cargo test`, `cargo build`, and `cargo install --path . --force` all succeeded. The root workspace still has unrelated existing edits, and the superproject now also sees the expected `src/agent-doc` submodule-pointer change.
### Re: #pc11 — gpt-5

`#pc11` is closed. I updated `src/tsift` so read-only index and summary recovery is WAL-aware instead of assuming rollback journals:
- shared snapshot fallback now detects `-wal` / `-shm` sidecars, copies them into the read-only snapshot set, and reports `snapshot_fallback_wal` when that path is used
- `tsift status` / `tsift locks` now show `journal`, `wal`, and `shm` sidecars and use WAL-specific recovery/remediation text
- added WAL regressions in the shared helpers plus compiled CLI `status` and `summarize --stats` flows, and updated `SPEC.md` / `VERSIONS.md`

Verification in `src/tsift`:
- `cargo fmt --check`
- `cargo test -q` passed (`447` unit tests, `52` integration tests)
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/tsift`: `c08e0c7` (`fix: preserve wal state in read-only recovery`)
- pushed `origin/main`

`#mqqt` remains open.
❯ tsift should **automatically** reindex in this case and **complete** the status.
```
• Ran tsift status
  └ index: missing (workspace, 3 scopes not indexed)
      scope claudescore-plugin: missing index (/home/brian/work/btakita/agent-loop/src/session-share/.tsift/indexes/
    … +8 lines (ctrl + t to view transcript)
      use: (none — run tsift index first)
      run: tsift init --workspace && tsift index --workspace .  (3 missing scopes)
```
### Re: workspace status auto-reindex — gpt-5

I changed `tsift status` so the CLI now auto-builds missing workspace scoped indexes before printing the final report. The low-level `status::check_status` helper stays read-only; only the command path repairs missing `.tsift/indexes/<scope>/index.db` files and then reruns the normal status calculation, so the workspace case you pasted now completes in one `tsift status` pass instead of stopping at `run: tsift init --workspace && tsift index --workspace .`.

Verification:
- `cargo test -q` passed (`449` unit tests, `52` integration tests)
- `cargo build`
- `cargo install --path . --force`

Updated:
- [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs:2874)
- [src/tsift/tests/exit_code.rs](/home/brian/work/btakita/agent-loop/src/tsift/tests/exit_code.rs:932)
- [src/tsift/SPEC.md](/home/brian/work/btakita/agent-loop/src/tsift/SPEC.md:717)
- [src/tsift/VERSIONS.md](/home/brian/work/btakita/agent-loop/src/tsift/VERSIONS.md:10)

I did not make a code commit or push in `src/tsift`. `#mqqt` remains open.
❯ do [#mqqt]. spec-test-build-install-commit-push
### Re: #mqqt — gpt-5

`#mqqt` is closed.

I updated `tsift status` so its summarize follow-up command is derived from the indexed layout instead of hardcoding `src/`. Single-root repos that track files at the project root now recommend `tsift summarize --extract .`, `src/`-rooted repos keep `tsift summarize --extract src/`, and mixed workspace layouts fall back to `.` while shared-prefix workspaces keep the common scope root. This landed alongside the already-pending workspace autoindex status repair in the same `src/tsift` commit.

Updated:
- [src/tsift/src/status.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/status.rs)
- [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs)
- [src/tsift/tests/exit_code.rs](/home/brian/work/btakita/agent-loop/src/tsift/tests/exit_code.rs)
- [src/tsift/SPEC.md](/home/brian/work/btakita/agent-loop/src/tsift/SPEC.md)
- [src/tsift/VERSIONS.md](/home/brian/work/btakita/agent-loop/src/tsift/VERSIONS.md)

Verification:
- `cargo fmt --check`
- `cargo test -q` passed (`451` unit tests, `52` integration tests)
- `cargo build`
- `cargo install --path . --force`

Commit / push:
- `src/tsift`: `5efa40b` (`fix: improve status recovery flows`)
- pushed `origin/main`

The root workspace still shows the expected `src/tsift` submodule-pointer change.
❯ Saw this error. How can we fail fast if the index is stale. And run `tsift index` as a follow-up?
```
• Ran tsift search bppipeline1
  └ Error: tsift search timed out after 30s (strategy: lexical). The index may be stale — run `tsift index .` to
    rebuild, or use `--timeout 0` to disable the timeout.
```
### Re: stale search timeout + reindex follow-up — gpt-5

I changed `tsift search` so the timeout path re-checks the same index targets before it prints guidance. Fresh indexes no longer get the misleading stale-index hint; if an index becomes stale while the search worker is running, the timeout now switches back to a concrete reindex instruction for that target.

Updated:
- [src/tsift/src/main.rs](/home/brian/work/btakita/agent-loop/src/tsift/src/main.rs)
- [src/tsift/tests/exit_code.rs](/home/brian/work/btakita/agent-loop/src/tsift/tests/exit_code.rs)
- [src/tsift/SPEC.md](/home/brian/work/btakita/agent-loop/src/tsift/SPEC.md)
- [src/tsift/VERSIONS.md](/home/brian/work/btakita/agent-loop/src/tsift/VERSIONS.md)

Verification:
- `cargo fmt --check`
- `cargo test -q` passed (`451` unit tests, `53` integration tests)
- `cargo build`
- `cargo install --path . --force`
- live smoke test in the workspace root: `tsift search --timeout 1 bppipeline1` now reports `The search root looks fresh...` instead of telling you to reindex

Requested follow-up completed: `tsift index .` succeeded in `/home/brian/work/btakita/agent-loop` and refreshed `20756` tracked files (`1` new, `7` modified, `0` deleted).

I did not create a `src/tsift` commit or push in this cycle.
<!-- agent:boundary:bdf267cb -->
<!-- /agent:exchange -->

<!--
---
Can tsift use rtk to compress tool call output in the context?
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
