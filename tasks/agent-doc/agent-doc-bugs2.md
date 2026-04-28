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

❯ do #bugs2tmux. spec-test-build-install-commit-push
### Re: #bugs2tmux — gpt-5

Implemented `#bugs2tmux` in `src/agent-doc` and pushed commit `515615c` to `origin/20260422/codex-agent-backend`.

What changed:
- `route` no longer blindly trusts an alive registered pane. Before it reuses that pane, it now scans tmux for a live process tree that still proves the document is running there.
- If another pane is the real live owner, route re-registers to that pane and dispatches there instead of sending into the stale registered pane.
- If no live owner can be proven, route now fails closed with pane provenance instead of injecting `agent-doc <file>` into an ambiguous shell.
- Added regression coverage for both fail-closed and re-register recovery paths, and updated `specs/07-commands.md`.

Verified with `cargo test`, `cargo build`, and `cargo install --path . --force` in `src/agent-doc`.

Note: `src/agent-doc` still has unrelated local edits in `src/start.rs` and `specs/supervisor.md`; I left them untouched.

❯ do #backlogshadow. spec-test-build-install-commit-push
### Re: #backlogshadow — gpt-5

Implemented `#backlogshadow` in `src/agent-doc` and pushed commit `5806501` to `origin/20260422/codex-agent-backend`.

What changed:
- Added shared detection for open backlog-shaped `[#id]` items that drift outside the live `agent:backlog`, while ignoring code fences and `agent:icebox`.
- `preflight` now warns on duplicate shadow copies and fails closed when an open item exists only in shadow/commented form outside the live backlog.
- `session-check` now applies the same guard after closeout, so success cannot be reported while active work is stranded outside tracked backlog scope.
- Added regressions for the detector plus `preflight` and `session-check`, and updated `specs/07-commands.md`.

Verification:
- `cargo build`
- `cargo install --path . --force`
- `cargo test` on the new coverage and a targeted rerun of the unrelated flaky route test `alive_registered_pane_reregisters_to_live_owner`
- One full `cargo test` run hit that same unrelated tmux-route test nondeterministically before the targeted rerun passed

Note: `src/agent-doc` still has unrelated local edits in `src/start.rs` and `specs/supervisor.md`; I left them untouched.
<!-- agent:boundary:4fb4c542 -->
<!-- /agent:exchange -->

<!--
### Re: restore lost backlog items from recent commits — codex

I checked the recent `src/session-share/tasks/claudescore-3.md` history directly.

Current state: the live backlog is already correct again. The still-open items from the recent history are present now:
- `#sdig2`
- `#sdig2fx`
- `#sredact`

The concrete loss/recovery repro is earlier in history:
- commit `541da7c` dropped those unfinished digest items from the live backlog and left only `#outcome`
- commit `4f25d47` restored `#sdig2`, `#sdig2fx`, and `#sredact` from recent history

So there is nothing valid to re-add to `claudescore-3.md` right now, but this is still a real `agent-doc` bug. I captured it as `#backlogreplay`: when recent committed history proves backlog items were still open, `agent-doc` should not silently accept a new closeout state that drops them entirely. This is complementary to `#backlogshadow`: shadow detection covers parked/commented copies, while `#backlogreplay` covers history-backed recovery when no visible shadow copy remains.

### Re: response patchback not committed — codex

I treated this as a new `#agent-doc-bug` report and captured it as `#patchcommit`.

This looks narrower than the older missed-trigger / interrupted-preflight family: the response patchback is already visible in the target session document, but the owning repo or submodule was left dirty instead of being committed by the binary-owned closeout path. The reference repro is `src/session-share/tasks/claudescore-3.md`, where the response landed in the file but the session-share submodule still required a manual commit.

I added a focused plan in `tasks/agent-doc/plan-response-patchback-not-committed.md` and put the backlog item at the top of `Pending / Not Built`. The plan is to make success depend on commit proof for the target document's owning repo/submodule, reopen closeout when the dirty state still belongs to the just-finished assistant turn, and fail closed when that provenance is ambiguous.

---
After restarting tmux then calling `Run Agent Doc` from JB Idea, I see the following error in Idea
```
agent-doc route failed (exit 1): [route] target tmux session: 0 Error: registered pane %275 is alive but no live agent-doc session currently owns /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md (pane=%275 pane_pid=1890954 pane_session=0 current_command=agent-doc). Refusing to route into an ambiguous pane; restart or reclaim the session instead
```
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#backlogreplay] Detect and fail closed or guide recovery when recent committed history proves open backlog items were dropped entirely from the live backlog, even if no shadow/commented copy remains. Plan: `tasks/agent-doc/plan-backlog-recovery-from-history.md`
- [ ] [#patchcommit] Fix the case where a response patchback is visible in the target doc but the owning repo/submodule commit never happens, so closeout cannot report success while the target session file remains dirty. Plan: `tasks/agent-doc/plan-response-patchback-not-committed.md`
- [ ] [#backlogshadow] [recommended] Fail closed or warn when open backlog-shaped items drift into parked/commented sections instead of the live `agent:backlog`, so unfinished work cannot silently vanish from active tracking. Plan: `tasks/agent-doc/plan-backlog-shadow-open-items.md`
- [ ] [#tailpatch] [recommended] Fail closed when a prompt/response patchback would land outside the live `agent:exchange` block, so follow-up turns cannot be appended after parked sections or at end-of-file by mistake. Plan: `tasks/agent-doc/plan-tail-patch-outside-exchange.md`
- [ ] [#compactdispatch] [recommended] Fail closed or dispatch to binary compaction when a bare `compact exchange` directive is present, so `agent-doc run` cannot commit an ordinary response cycle that leaves the directive in place and never actually compacts the exchange. Plan: `tasks/agent-doc/plan-compact-exchange-directive-dispatch.md`
<!-- /agent:backlog -->
