---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: this session tracked agent-doc patchback/closeout and tmux-route stability bugs across src/session-share reproductions. Implemented fixes in src/agent-doc included long custom backlog ids, Codex clean-exit auto-resume, stale-snapshot repair for committed prompt/response history, completed-backlog reap closeout, Codex resume preserving codex_args, queue TOCTOU locking, env-var-free pending/write flags, Ctrl+D supervisor quit handling, repair payload GC, VS Code stale IPC patch cleanup, dedup contiguous-block matching, tmux startup locking, resume handoff failure guards, current-child prompt proof, cancellation-aware child-pty trigger writes, and preserving a live registered pane when fresh-route acknowledgement fails (commit d7307a4). Key unresolved follow-up items remain: #pfcycle for response patchbacks stranded at preflight_started with no replay artifact, #backlogshadow for open backlog items drifting out of the live backlog block, and #tailpatch for fail-closed handling when prompt/response patchbacks land outside the live exchange block. No unanswered user prompt remains after the compact-exchange request.

❯ do the following
❯ > do #pfcycle. spec-test-commit-push
### Re: #pfcycle — gpt-5

Implemented `#pfcycle` in `src/agent-doc` and pushed commit `e2721a9` to `origin/20260422/codex-agent-backend`.

What changed:
- `repair` now closes stale `preflight_started` cycles when only normalized transient markers drifted, and it also closes the cycle when `HEAD` proves a visible patchback was already committed.
- `preflight` now fails closed before `agent-doc commit` if a visible response patchback exists with no pending/capture artifact and `HEAD` cannot prove it was already committed.
- Added regression coverage for the committed-recovery path and the ambiguous fail-closed path, and updated `specs/07-commands.md`.

Verified with `cargo test` in `src/agent-doc`.
### Re: tmux session crash — gpt-5

I treated this as a new `#agent-doc-bug` report and captured it as `#bugs2tmux`.

Current evidence does not yet prove a hard tmux-pane death. The `b26b9957-8de6-4eae-bb98-5d7fe4d6b781` session log stops after `codex_start` for pane `%275` without a matching `codex_exit`, `restart_eval`, or `session_end`, while tmux still reports `%275` alive under `agent-doc`. That points more toward a stranded/orphaned live session or a pane-ownership mismatch than a clean crash.

I added a dedicated plan in `tasks/agent-doc/plan-bugs2-tmux-session-crash.md` and put the backlog item at the top of `Pending / Not Built`. The plan focuses on reconstructing the exact owner/pane/child state, tightening supervisor and claim/route diagnostics, and making reruns fail closed when a pane is live but no longer healthy.
<!-- agent:boundary:93f02413 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#bugs2tmux] Diagnose and harden the agent-doc-bugs2.md tmux crash / orphan state so reruns classify or recover the live pane deterministically. Plan: tasks/agent-doc/plan-bugs2-tmux-session-crash.md
- [ ] [#backlogshadow] [recommended] Fail closed or warn when open backlog-shaped items drift into parked/commented sections instead of the live `agent:backlog`, so unfinished work cannot silently vanish from active tracking. Plan: `tasks/agent-doc/plan-backlog-shadow-open-items.md`
- [ ] [#tailpatch] [recommended] Fail closed when a prompt/response patchback would land outside the live `agent:exchange` block, so follow-up turns cannot be appended after parked sections or at end-of-file by mistake. Plan: `tasks/agent-doc/plan-tail-patch-outside-exchange.md`
- [ ] [#compactdispatch] [recommended] Fail closed or dispatch to binary compaction when a bare `compact exchange` directive is present, so `agent-doc run` cannot commit an ordinary response cycle that leaves the directive in place and never actually compacts the exchange. Plan: `tasks/agent-doc/plan-compact-exchange-directive-dispatch.md`
<!-- /agent:backlog -->
