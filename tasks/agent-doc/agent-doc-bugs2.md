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

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
- Added one centralized `TmuxSubmitProfile` in `agent-doc-orchestration/src/sessions.rs`; `tmux_submit_mode_for_harness`, `tmux_submit_transform_for_harness`, and `tmux_submit_key_for_harness` delegate to it.
- Routed JB `Run Agent Doc`, JB `Clear Session Context`, queue dispatch, auto-trigger inject, and supervisor-owned tmux inject logging through that shared profile.
- Removed OpenCode as a default Kitty Return submit special case. The separate OpenCode PTY filtering for Kitty keyboard-mode permission prompts remains, because that is not the command-submit path.
- Kept owned PTY injection as `pty_cr`; the change is for tmux-bound live pane submit, not the supervisor child-PTY write boundary.
- Updated visible-draft recovery to be profile-driven, so a timed-out visible draft gets exactly one bare `Enter` retry for OpenCode too.
- Updated specs/docs/tests and the static ops-report example so the only tmux live-pane submit mode is `tmux_literal_text_enter_key`.
- `cargo test submit` passed.
- `cargo test queue_dispatch` passed.
- `cargo test -p agent-doc-orchestration submit` passed.
- `make check` passed: clippy, 4542 nextest tests, doctests, and the ignored medium sim-world budget test.
- `cargo build` passed.
- `cargo install --path .` completed and replaced `/home/brian/.cargo/bin/agent-doc`; `agent-doc --version` reports `0.34.1`.
- Latest `CI` on `main` was green: run `27588543817`, `fix(jb): add run-agent-doc proof logging`, completed successfully on 2026-06-16.
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260616-020038.md`*

Compacted content:
❯ - Archived 4 response topic(s): JB Run Agent Doc Codex boot timeout; JB Run Agent Doc Logging; Queue Repair Closeout; 1 more
❯ - Prior summary/context: prior compacted content: Archived 39 response topic(s): #compactdrift-agent; Compact Closeout Guard Follow-Up; Restor...

### Re: tmux submit profile and OpenCode Enter — gpt-5

> **Queue prompt:**
>
> :pushpin: Both JB `Clear Session Context` and JB `Run Agent Doc` send a CR instead of submitting ![img_24.png](img_24.png). There is also an extra prompt that bleeds into the input ![img_25.png](img_25.png). Why is this so difficult to fix? Can we design a way to detect what is sent and how it is processed by the agent? Fix root causes. #simworld


Fixed the tmux-bound submit root cause in `src/agent-doc`: the default live-pane path no longer sends a raw CR, and OpenCode no longer has a Kitty Return submit profile.

Answer to the direct tmux question: yes, the submit path now uses the terminal-neutral shape you tested, but with literal text protection: `tmux send-keys -t <pane> -l <text>; tmux send-keys -t <pane> Enter`. The split `-l` text + named `Enter` avoids tmux interpreting prompt text as keys while still delivering the real Enter key event. Codex, Claude, OpenCode, and unknown/default harnesses all resolve through that same profile now.

What changed:
❯ - Added one centralized `TmuxSubmitProfile` in `agent-doc-orchestration/src/sessions.rs`; `tmux_submit_mode_for_harness`, `tmux_submit_transform_for_harness`, and `tmux_submit_key_for_harness` delegate to it.
❯ - Routed JB `Run Agent Doc`, JB `Clear Session Context`, queue dispatch, auto-trigger inject, and supervisor-owned tmux inject logging through that shared profile.
❯ - Removed OpenCode as a default Kitty Return submit special case. The separate OpenCode PTY filtering for Kitty keyboard-mode permission prompts remains, because that is not the command-submit path.
❯ - Kept owned PTY injection as `pty_cr`; the change is for tmux-bound live pane submit, not the supervisor child-PTY write boundary.
❯ - Updated visible-draft recovery to be profile-driven, so a timed-out visible draft gets exactly one bare `Enter` retry for OpenCode too.
❯ - Updated specs/docs/tests and the static ops-report example so the only tmux live-pane submit mode is `tmux_literal_text_enter_key`.

I did not add `lazily-rs` here. The duplication was removed at the policy boundary instead: a small Rust enum/function is enough, avoids adding another dependency, and keeps submit semantics greppable from one place. If we later need generated dispatch tables for more harness behavior, that is a separate design pass.

Verification:
❯ - `cargo test submit` passed.
❯ - `cargo test queue_dispatch` passed.
❯ - `cargo test -p agent-doc-orchestration submit` passed.
❯ - `make check` passed: clippy, 4542 nextest tests, doctests, and the ignored medium sim-world budget test.
❯ - `cargo build` passed.
❯ - `cargo install --path .` completed and replaced `/home/brian/.cargo/bin/agent-doc`; `agent-doc --version` reports `0.34.1`.
❯ - Latest `CI` on `main` was green: run `27588543817`, `fix(jb): add run-agent-doc proof logging`, completed successfully on 2026-06-16.

Changed source paths include `agent-doc-orchestration/src/sessions.rs`, `agent-doc-orchestration/src/route/dispatch.rs`, `agent-doc-orchestration/src/flow/routed_reopen.rs`, `agent-doc-orchestration/src/start/idle_watch.rs`, `agent-doc-orchestration/src/start.rs`, `agent-doc-orchestration/src/start/supervisor_io.rs`, `src/queue_dispatch.rs`, `src/session_actor_cmd.rs`, `src/orchestrate/dispatch.rs`, `src/sim_world.rs`, `src/ops_report.rs`, `README.md`, and the tmux/session-routing supervisor specs.

Source commit/push status: not done. The `src/agent-doc` checkout already had broad unrelated dirty changes before this turn, and `cargo fmt` exposed large pre-existing formatting churn across many files. Staging whole files would include unrelated work, and a partial source commit built on the dirty tree would not be a trustworthy standalone commit. The safe next step is to isolate this patch on a clean branch or explicitly approve staging the broader dirty set.

JetBrains plugin status: no JetBrains plugin source changed, so no JB plugin update is required for this fix. The installed `agent-doc` CLI binary was updated. The route-owned supervisor for this document was still running an older binary during preflight, so it should be restarted to use the new submit policy.
<!-- agent:boundary:c7c85092 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: Both JB `Clear Session Context` and JB `Run Agent Doc` send a CR instead of submitting ![img_24.png](img_24.png). There is also an extra prompt that bleeds into the input ![img_25.png](img_25.png). Why is this so difficult to fix? Can we design a way to detect what is sent and how it is processed by the agent? Fix root causes. #simworld~~
- :pushpin: Why are we ever using `tmux_literal_cr` and not `tmux_literal_text_enter_key`? Why do we have so much duplicate logic with the submit keys all over the codebase? Each harness' tmux_literal should be defined in a single place. No tmux_literal values spread all over the place. Why tmux_literal_kitty_return for opencode? Is tmux_literal_text_enter_key not working? #simworld
- :pushpin: Does agent-doc use `tmux send-keys -t target "command" Enter` to submit a prompt? Would this work with opencode? I don't want to hardcode a kitty return b/c kitty is specific to my environment...not universal. I'm concerned that it wont work on other terminals. I tried `tmux send-keys -t .1 "Hello" Enter` on opencode and it worked.
- :pushpin: Can we use lazily-rs to avoid duplicate logic with the submit keys all over the codebase?
- :pushpin: Issue with restart on Codex ![img_26.png](img_26.png) ![screenshot.png](../../../../../Pictures/agent-loop/screenshot.png)
- :pushpin: Complete the `[operator-verify]` and live verification using the logs to prove/disprove the intended behavior.
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
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lazilyqrestore] [operator-verify] Restore/account for lost queue content in `tasks/software/lazily-rs.md`: that document is foreign-owned by another pane, so capture the intended lost queue heads from the owning lazily-rs session/doc history/snapshots/sidecars, restore them as backlog items there, or record explicit user-removal proof. Related #queue-clear-unrun-items/#manual-queue-head-loss/#queue-user-edit-overwrite.
- [ ] [#recyclerestart-verify] [operator-verify] #recyclerestart live proof + promotion: Q1 (recycle ENOENT / restart) + Q2 (FFI self-heal sync guard, supersede-newer-generation) are committed (agent-doc 0b31e90c, fa6a230a) but NOT promoted onto the live stale supervisor — the img_23 stall is the pre-fix binary. Recycle/restart the supervisor onto the new build (agent-doc admin recycle --all-projects, or restart), then confirm: kill-pane + JB Sync Tmux Layout no longer reports Sync deferred, and a real idle/next-queue recycle restarts/re-clears the agent session. Cannot be agent-done. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof — two-live-IDE CRDT broadcast convergence: with the installed build, edit the same managed doc in JB and VS Code at once and confirm both editor buffers receive the broadcast patch and converge (no clobber). Implementation landed agent-doc cddad87b (id/deliver/merge, SimWorld-covered, CI green). Cannot be agent-done — needs two live editors. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#1j8q] [operator-verify] #suprecyclelock live proof: drive a real next_queue_item supervisor self-recycle (go-mode queue drain over a freshly-installed binary) while a sibling/concurrent launcher holds the shared project-root controller-launch.lock; confirm the execve'd start no longer surfaces "controller launch already in progress ... (os error 11)" on the pane and ops.log shows the contended waiter adopting the published controller instead. Fixed in agent-doc-orchestration project_controller.rs LaunchLock::acquire_blocking + connect_or_launch.
- [ ] [#anw0verify] [operator-verify] #anw0 redirect live proof: install the new agent-doc binary (cargo build --release + cargo install + lib-install) then drive a real recycle / restart-supervisor race so a dispatcher holding a superseded generation hits the new path. Confirm ops.log shows dispatch_stale_generation_redirect prior_generation=N next_generation=N+1 then dispatch_retry_after_stale_generation and a clean re-dispatch (no terminal generation-N-is-closed). Code+SimWorld landed agent-doc 21a70cdd / superproject 412355fa3. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [ ] [#qt32] [operator-verify] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [ ] [#fcc0-live-verify] [operator-verify] #fcc0 live JB 0-dialog proof: with the current agent-doc binary/cdylib installed and the route-owned supervisor recycled onto it, drive a live JetBrains queue/template/compact write path and confirm ops.log shows editor IPC convergence (for example queue_consume_writeback ... transport=editor_ipc) and zero JetBrains File Cache Conflict dialogs. Cannot be agent-done; requires live IDE observation.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
