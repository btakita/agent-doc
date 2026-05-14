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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260514-034125.md`*

Compacted content:
- Archived 1 response topic(s): directsubmitbudget
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

❯ do [#latefallbackloop]
❯ sync orchestra
❯ dispatch #spec-test-build-install-commit-push
❯ - do [#latefallbackloop]
❯ - do [#clearstartingrace]
❯ - do [#ownedreapbusy]
❯ - do [#opencodeinputdiag]
❯ - do [#tmuxe2etests]
### Re: latefallbackloop — gpt-5

Implemented `#latefallbackloop`.

The late IPC fallback path now treats an already committed cycle as a terminal skip instead of a successful IPC consume. `try_ipc` returns an explicit `skipped_committed_cycle` result, callers stop before logging `ipc_write_consumed`, stale queued patch JSON is cleaned up with a claimed-patch sentinel, and the socket-to-file fallback rechecks committed state before writing another fallback patch.

Updated the session-routing spec and `VERSIONS.md`, and added a regression proving a committed-cycle late IPC attempt is not reported as consumed and cannot leave a stale patch for a watcher to replay.

Verification:
- `cargo test late_fallback_patch_guard_tests`
- `make check`
- `gh run list --workflow "Agent Doc" --limit 5`: latest remote `Agent Doc` runs are red immediately
- `gh run view 25838897606 --log-failed`: `log not found`, so the remote tmux failure was not inspectable
- `make tmux-ci`: 227 live tmux tests passed, 0 failed
- `make install`: installed `agent-doc v0.33.16` and `libagent_doc-0.33.16.so`

Commits:
- `src/agent-doc` `b5a3a2b4` (`fix: stop late ipc fallback loops`) pushed to `origin/20260422/codex-agent-backend`
- parent repo `7f0d3dc66` (`agent-doc: stop late ipc fallback loops`) is committed locally and will be pushed after this document closeout commit is created
❯ Why did symphony orchestra not create a chain of subagents?
### Re: orchestration subagents — gpt-5

Because `agent-doc orchestrate --mode sequential` is not implemented as a Codex subagent chain.

The code path in `src/agent-doc/src/orchestrate.rs` says sequential orchestration injects one task into the document exchange, runs preflight, sends one fresh backend request with no session reuse, then persists through `finalize` and `session-check` before moving to the next task. Its contract explicitly says sequential orchestration never resumes prior agent sessions between tasks: each step is `session_id=None` and `fork=false`. So the intended shape is fresh step runs owned by the `agent-doc` binary, not nested Codex `spawn_agent` workers or a chain of subagents.

In the live run, it also did not get past step 1. The orchestrator injected `❯ do [#latefallbackloop]`, then failed its own closeout with:

`orchestrate template-mode responses must include a <!-- patch:exchange --> block`

After that, preflight saw only the injected first task as the active prompt. I completed that task manually in the normal document lifecycle, which is why the remaining requested tasks were not fanned out or chained.

So there are two separate points:

1. Sequential orchestration is stepwise fresh-agent execution, not subagent spawning.
2. This particular orchestration aborted on the first step because the generated template-mode response did not satisfy the required patchback shape.
<!-- agent:boundary:4a49504f -->
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
