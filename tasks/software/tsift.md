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
<!-- agent:boundary:846ea2b5 -->
<!-- /agent:exchange -->

<!--
Can tsift use rtk to compress tool call output in the context?
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#pc11] wal-lock-recovery Fix index/summarize read-only recovery and lock diagnostics to handle SQLite WAL mode correctly; copy or back up live WAL state instead of relying on -journal detection.
- [ ] [#mqqt] status-summarize-scope Make status summarize recommendations scope-aware instead of hardcoding 'tsift summarize --extract src/' so non-src layouts and workspace scopes get valid follow-up commands.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
