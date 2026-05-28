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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260527-233459.md`*

Compacted content:
- Archived 1 response topic(s): JB Run Agent Doc on tsift.md
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: do [#h9g0] — gpt-5

Implemented `#h9g0`.

The closeout proof now treats a template patch response as materialized by the body it applies, not by the wrapper comments. That means `stuck_captured_cycle` and IPC sidecar validation can recognize a response written through the boundary/template path even though committed `HEAD` contains only the visible exchange body.

The stream write path also now fails before response capture if patch markers are present but parse to zero patch blocks. Plain patch-free responses can still log the `0 template patches found` warning, but marker-bearing malformed input no longer gets captured and later reported as a stuck cycle.

```mermaid
flowchart LR
    A["finalize --stream input"] --> B["Parse template patchback"]
    B -->|"patch markers + zero patches"| C["Fail before capture"]
    B -->|"valid patch:exchange"| D["Capture response"]
    D --> E["Apply patch body to exchange"]
    E --> F["Validate materialization using patch body"]
    F --> G["Commit / session-check"]
    F -->|"body absent"| H["IPC proof failure or stuck-cycle recovery"]
```

Changed paths:
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/flow/closeout.rs`
- `src/agent-doc/specs/07-closeout-commands.md`

Verification:
- `cargo test stuck_captured_cycle_ignores_committed_template_patch_body_in_head`
- `cargo test materialization`
- `cargo test marker_bearing_zero_patch_parse_is_rejected_before_capture`
- `cargo test patch_wrapped_response_is_materialized_by_visible_patch_body`
- `cargo test malformed_patchback_is_rejected_instead_of_appended_as_unmatched`
- `cargo test stream_ipc_timeout_commit_removes_fallback_patch_file`
- `make check`
- `make install`
- GitHub Actions CI run `26545667145`: `make check` passed; `make tmux-ci` passed.

Commit/push:
- `btakita/agent-doc@0410445b30a58667837a3bcab7facb14ef67357a` pushed to `20260422/codex-agent-backend`.
- Root submodule pointer commit `cecf6e98d` is ready in `btakita/agent-loop`; this closeout will be pushed with the session-document commit.
### Re: do [#zeb5] — gpt-5 (HEAD)

Closed `#zeb5` with the `#h9g0` implementation.

`#zeb5` was the narrower form of the same failure: `finalize --stream` could report `0 template patches found` and then let marker-bearing heredoc input enter response capture, producing a later stuck-cycle warning. The write path now rejects marker-bearing zero-patch parses before capture, while valid `<!-- patch:exchange -->` responses are validated by the materialized patch body instead of by wrapper comments.

```mermaid
flowchart LR
    A["Marker-bearing heredoc"] --> B["Parse template patches"]
    B -->|"zero patches"| C["Reject before capture"]
    B -->|"valid exchange patch"| D["Apply patch body"]
    D --> E["Materialization proof uses body text"]
    E --> F["No stuck captured cycle"]
```

No additional code changes were needed beyond `btakita/agent-doc@0410445b30a58667837a3bcab7facb14ef67357a`.

Verification already completed for that commit:
- `cargo test stuck_captured_cycle_ignores_committed_template_patch_body_in_head`
- `cargo test materialization`
- `cargo test marker_bearing_zero_patch_parse_is_rejected_before_capture`
- `cargo test patch_wrapped_response_is_materialized_by_visible_patch_body`
- `cargo test malformed_patchback_is_rejected_instead_of_appended_as_unmatched`
- `cargo test stream_ipc_timeout_commit_removes_fallback_patch_file`
- `make check`
- `make install`
- GitHub Actions CI run `26545667145`: `make check` passed; `make tmux-ci` passed.
<!-- agent:boundary:7a59647b -->
<!-- /agent:exchange -->
###
<!--
JB `Run Agent Doc` on tsift.md error:
```
Session is still running for tasks/software/tsift.md. Pane %21 is busy (agent-doc). Wait for the turn to finish, then retry Clear Session Context. Use Refresh and retry if the pane has returned to an idle prompt, or Interrupt and clear to discard the running turn. Latest pane output: 22.4K (11%) ctrl+p commands
```
---
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

<!-- agent:queue auto -->
dispatch #spec-test-build-install-commit-push
- ~do [#h9g0]~
- ~do [#zeb5]~
- do [#already-applied-divergence]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
- [ ] [#already-applied-divergence] already_applied IPC path saves stale content_ours as snapshot, diverging HEAD from working tree — plan: tasks/agent-doc/plan-already-applied-snapshot-divergence.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
