---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Recorded Codex capability proof verification.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260513-234307.md`*

Compacted content:
- Archived 1 response topic(s): startreadytimeout
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

❯ (preset #spec-test-build-install-commit-push)
❯ update spec + tests. build + install for local testing. commit + push
❯ do [#multiplexertrait]
❯ synchronous orchestra
❯ preset #spec-test-build-install-commit-push
❯ - do [#multiplexertrait]
❯ - do [#startreadytimeout]
❯ - do [#directsubmitbudget]
❯ - do [#latefallbackloop]
❯ - do [#clearstartingrace]
❯ - do [#ownedreapbusy]
❯ - do [#opencodeinputdiag]
❯ - do [#tmuxe2etests]
### Re: orchestration handoff — gpt-5

I started the requested synchronous orchestra, but the batch hit two root blockers before the backlog tasks could safely run:

- `preset spec-test-build-install-commit-push` did not resolve to the defined `#spec-test-build-install-commit-push` frontmatter key.
- After preflight opened the handoff cycle, orchestrate could commit the first injected task into the snapshot before preflight saw it as a prompt-bearing diff, stopping with `orchestration step did not produce a prompt-bearing diff after injection`.

I fixed both in `src/agent-doc`: preset references now canonicalize bare names to hashtag keys when appropriate, and orchestrate now snapshots/abandons an open preflight handoff cycle before injecting the first task. I also updated the orchestration spec and added regressions for bare hashtag preset aliases and the preflight-handoff injection path.

I left `#multiplexertrait` and the remaining batch items open because the requested eight task executions did not finish in this run. The installed local binary contains the fixes, so the batch can be rerun without the two handoff blockers.

Verification:
- `cargo test prompt_preset`
- `cargo test close_open_preflight_handoff_cycle_snapshots_before_injection`
- `make check`
- `make tmux-ci`
- `cargo build`
- `cargo install --path .`
- `agent-doc --version` -> `agent-doc 0.33.16`

CI / publish evidence:
- Latest GitHub `CI` run on `main` was green: `25773430221`.
- Commit pushed: `src/agent-doc` `0cf58582 fix: normalize orchestrate prompt presets` to `origin/20260422/codex-agent-backend`.

No new backlog item was added; the existing batch items remain open.
<!-- agent:boundary:1054f0f5 -->
<!-- /agent:exchange -->
###
<!--
Autosync on editor file change + `Sync Tmux Layout` lands on a 1 tmux pane and 3 tmux pane layout with 2 editor panes.
spec-test-build-install-commit-push
---
The managed proof check tmux prompt should be terse and more "human readable" on a single line. Explain the status without the extra information.
spec-test-build-install-commit-push
---
`--pending-only` should be renamed to `--backlog-only`.
spec-test-build-install-commit-push
---
Please ensure the JB opencode fixes are propagated to the VSCode plugin. Ensure the specs are updated for all editor plugins. Should we implement in the ffi plugin?
#next-steps
---
JB `Run Agent Doc` on root.md with `agent: claude` sends an incorrect command
```
❯ agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md
  ⎿  API Error: 400 Could not process image
```
spec-test-build-install-commit-push
---
Should tsift fail fast in this case?
```
• Ran tsift search "transcript not found livekit-101"
  └ Error: tsift search timed out after 30s (strategy: lexical). The search root looks fresh, so reindexing is unlikely
    to help. Re-run with `--timeout 0` to disable the timeout, narrow `--path` / `--scope`, or try a different
    strategy.
```
spec-test-build-install-commit-push
---
How can we move the agent-doc concept to html files?
Can the agent-doc supervisor run a web server, using contenteditable via tinymcp or another editor to write back to the source file?
Would major editors such as VSCode, JetBrains, Zig, and TUI editors support this workflow?
---
With the latest supply chain attacks on npm and pypi, should agent-doc should run in a docker container? Should all software written run in a docker container?
---
JB `Restart Supervisor Process` error:
```
agent-doc command failed (exit 1): Error: session_restart refused for /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="⏵⏵ bypass permissions on (shift+tab to cycle)"). Run `agent-doc session status /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.
```
spec-test-build-install-commit-push
---
Move JB `Copy Session Diagnostics` outside the numeric shortcuts.
---
I don't have a good track on the lifecycle logic that starts panes, kills panes, edge cases, etc.
Can we make a nested list and diagram of the lifecycle logic?
---
Why is agent-doc starting a child Codex run?
```
• agent-doc started a child Codex run. Its preflight noticed the previous patchback was already committed, then
  submitted the new follow-up prompt for execution.
```
---
```
3. Workaround: delegate SSH via the host — if OpenCode can't SSH directly, have agent-doc run the SSH deploy           step outside the managed pane (in the harness supervisor process), not inside the OpenCode child.
```
---
Confirm that z.ai subscriptions are message based and not token based. If message based, is agent-doc inefficient on message based plans? Please review code + logs.
#next-steps
---
minor (v0.34.0) release + publish. merge into main
---
/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-project-controller-session-actor-mvp.md
---
"Press Enter to restart, or 'q' to exit."
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#multiplexertrait] Evaluate Multiplexer trait abstraction over tmux to reduce coupling (785 tmux references across 43 files). Would allow future Zellij backend and make the tmux surface testable via mock backend. [recommended]
- [ ] [#directsubmitbudget] #agent-doc-bug: Direct pane submit telemetry reports 5.18s timeouts / over-budget sends even when dispatch_start_proof later proves the prompt was consumed. Split input-acceptance latency from dispatch-start proof, avoid false timeout language after consumed proof, and tune the owned/hybrid pty submit budget around actual tmux/control-mode behavior.
- [ ] [#latefallbackloop] #agent-doc-bug: Late fallback patch writes keep firing after a cycle is already committed; logs show repeated late_fallback_patch_rejected reason=already_committed, ipc_write_consumed, commit_already_current, and one resume_commit_blocked_drift with an unresolved prompt-only tail. Stop fallback writers at terminal commit state or rebind them to the next cycle before accepting IPC patches.
- [ ] [#clearstartingrace] #agent-doc-bug: Clear/restart commands can race starting owned panes; logs show session_clear_sent while generation 170 was still starting, followed by protected-input refusals, commits, and repeated starting_not_ready. Gate clear/restart on dispatch-ready or clean-exit prompt state and document-dirty state before sending literal Enter into the pane.
- [ ] [#ownedreapbusy] #agent-doc-bug: Owned pane reap decisions are noisy and often keep panes alive from spinner tails like Working/escape-title fragments instead of a stable idle/busy signal. Normalize screen-tail sampling or prefer supervisor prompt state so route_owned_reap_decision does not preserve panes based on transient renderer text.
- [ ] [#opencodeinputdiag] #agent-doc-bug: Add structured diagnostic logging for all tmux input paths — supervisor stdin forwarding, send-keys dispatch, Kitty keyboard protocol state, permission prompt detection transitions. Emit a log line per key event with source/destination/transform so regressions can be caught with log assertions, not manual testing.
- [ ] [#tmuxe2etests] #agent-doc-bug: Create end-to-end tmux input test harness that launches real tmux sessions (in CI-safe headless mode) and verifies Enter, arrow keys, and prompt submit through the actual supervisor→tmux→harness pipeline. Current SimWorld tests verify logic but not the tmux IPC surface.
- [ ] [#sshcapfalsecapture] #agent-doc-bug: Required-SSH guard falsely failed a Monster Rod Holders `agent-doc` run by matching historical `.agent-doc/captures/...json` text containing `socket: Operation not permitted`; `command_execution_required_ssh_failure()` scans arbitrary command output before proving the command was an SSH probe. Plan: tasks/agent-doc/plan-required-ssh-false-positive-historical-capture.md
- [ ] [#codexverbosepb] #agent-doc-bug: Codex child-run response patchback is too verbose because `src/agent-doc/src/agent/codex.rs` concatenates every `item.completed` `agent_message`, so progress/status chatter from a Monster Rod Holders run was committed into `tasks/monsterrodholders.md` along with the final closeout. Plan: tasks/agent-doc/plan-codex-response-patchback-verbose-progress.md
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->
- 2026-05-13 [#donearchiveattr] Implement external completed-work archive support for agent:done using archive=<repo-relative>.done.md. Plan: tasks/agent-doc/plan-external-done-archive-attribute.md
<!-- /agent:done -->
