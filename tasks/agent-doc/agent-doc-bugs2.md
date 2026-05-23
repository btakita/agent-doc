---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-213223.md`*

Compacted content:
- Archived 3 response topic(s): agent:queue auto stall; #tsiftipc; #narrowpatch
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ equityfundingsource.md had the duplicate prompt corrupt document issue as I was typing in a prompt. Why was the full-document IPC incorrectly used? Can we mathematically eliminate this issue?
❯ #spec-test-build-install-commit-push
### Re: compact exchange full-document IPC — gpt-5

The full-document IPC was used by Compact Exchange / Clear Session Context, not by the normal assistant response patch path. The root cause was in `compact::apply_compacted_document`: when an editor IPC listener or patch directory was present, compact exchange preferred `try_ipc_full_content_operator_mutation_from_source`, so the log showed `full_content_source_buffer_apply:compact_exchange` and `source=try_ipc_full_content_socket` for `tasks/professional/equityfundingsource.md`.

That was the wrong primitive for a live template session document. Even with source-buffer proof, a full editor-buffer replacement still has the race shape that matters here: it can replace the entire visible buffer around the time the operator is drafting the next prompt. The fix eliminates that class of failure by removing compact exchange's path to `fullContent` IPC for template session documents. Compact exchange now uses one guarded direct-write path: visible-idle plus compare-and-swap against the exact source content. If the file changed after compact read it, the write fails closed instead of touching the live prompt.

Changed paths:

- `src/agent-doc/src/compact.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/VERSIONS.md`

Coverage added/updated:

- compact exchange no longer emits a `.agent-doc/patches/*.json` full-content IPC file even when the patch directory exists.
- compact exchange is not blocked by a previous-cycle committed response fallback guard because it now uses its own guarded direct-write source.
- stale visible edits still fail closed through the compact direct-write path.

Verification:

- `cargo test -p agent-doc compact` passed.
- `make check` passed locally in `src/agent-doc`.
- `make install` passed and installed `agent-doc 0.33.16`.
- `agent-doc --version` reports `agent-doc 0.33.16`.
- GitHub CI run `26345119136` passed in 9m8s, including `Run make check` and `Run make tmux-ci`.

Commit and push:

- `src/agent-doc` commit `b11e3045` (`fix: avoid full-content IPC for compact exchange`) was pushed to `origin/20260422/codex-agent-backend`.
- Parent `origin/main` contains the `src/agent-doc` gitlink at `b11e30450b7c6c4524d061f5954bb48117563e86`; I verified that with `git ls-tree origin/main src/agent-doc`.
- I left unrelated dirty files and the unrelated local-only parent commit touching `tasks/software/tsift.md` alone.

Escalation: none.

Confidence: high. The issue is mathematically eliminated for this path because compact exchange no longer has a code edge to full-document editor IPC for template documents; the remaining write is guarded by exact source-content comparison and fails closed on concurrent edits.
<!-- agent:boundary:c1f57f6a -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
