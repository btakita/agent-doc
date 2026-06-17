---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260617-021642.md`*

Compacted content:
- Archived 10 response topic(s): JB Run Agent Doc Enter submit; JB Run Agent Doc route submit race; JB clear/run submit restart race closeout; 7 more
- Prior summary/context: prior compacted content: Archived 31 response topic(s): JB Run Agent Doc stale queue pause; binary logic for stale qu...
Implemented the JetBrains Run Agent Doc parity cleanup.

Dispatch-only reroutes now use one harness-neutral success policy: accepted shared tmux text+`Enter` delivery succeeds for Claude Code, Codex, and OpenCode, logging `proof=accepted proof_scope=accepted_only` when no stronger dispatch-start proof arrives. Codex hook proof and OpenCode pane-state proof still upgrade telemetry to `dispatch_start`, but Codex hook tracking no longer changes progress reporting or gates success differently.

The common tmux submit boundary remains centralized; the remaining harness-specific logic is readiness/blocker detection and optional proof observation, not a different submit path.

Commits pushed:
- `src/agent-doc`: `d314ee59 fix: make dispatch-only route proof harness-neutral`
- parent repo: `9f6725990 agent-doc: make dispatch-only route proof harness-neutral`

Verification passed: focused dispatch-only tests, the ignored live-tmux parity regression, `make check` (4613 nextest tests, doctests, medium SimWorld), `make install`, `agent-doc --version`, `git diff --check`, and latest GitHub Actions CI was completed/success.

Closeout note: the visible session document had already been compacted, so I rebuilt snapshot/CRDT sidecars from the current visible file before writing this response; I did not restore the stale pre-compaction snapshot over the editor buffer.
Implemented the active-editor IPC fail-closed change for this queue head.

> **Queue prompt:**
>
> :pushpin: Changes to the editor buffer should always win. 0 External Disk Writes. If CRDT fails, try again. Do not fallback to external disk writes on the document. I lose work when this happens as I am typing.


Summary:
- Stream/finalize/explicit IPC/flush no longer write the session document directly after an active editor IPC timeout or insufficient proof. The pending patch/response is retained for editor retry instead.
- Sidecar normalization, IPC dedupe, partial materialization, and session-check proof labels now fail closed with retry_without_disk_write instead of falling back to disk writes.
- Specs, reference docs, and regression tests were updated.

Verification:
- cargo test -p agent-doc-orchestration ipc -- --nocapture
- cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture
- cargo test -p agent-doc --test ipc_closeout_model --test finalize_integration --test equityfundingsource_ipc_replay -- --nocapture
- make check
- make install; agent-doc --version -> 0.34.1

Commits:
- agent-doc submodule: 0fe421f8
- parent gitlink: d52a3f56c
- GitHub Actions: workflow named CI is not present; latest Agent Doc run 27663436397 was in_progress when checked.
<!-- agent:boundary:c5376204 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: Changes to the editor buffer should always win. 0 External Disk Writes. If CRDT fails, try again. Do not fallback to external disk writes on the document. I lose work when this happens as I am typing.~~
- :pushpin: monsterrodholders.md JB `Run Agent Doc` dispatch stalled after a restart with `/clear` that should not have happened.
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof remains gated: code is landed in agent-doc cddad87b and headless targeted-delivery proof exists via SimWorld, but agent closeout on 2026-06-16 found only JetBrains live broadcast queue markers and no JetBrains-plus-VS Code live convergence/ack proof. Acceptance still requires opening the same managed doc in JB and VS Code, editing both buffers, and confirming both converge with no clobber and zero File Cache Conflict. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
