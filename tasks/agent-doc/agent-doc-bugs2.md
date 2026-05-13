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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260513-222126.md`*

Compacted content:
- Archived 1 response topic(s): tmuxcontrolmode
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Re [#tmuxhybridpty]: Can we avoid the Kitty vs non-Kitty protocol mapping layer? Does [#tmuxcontrolmode] allow for send-keys to bypass any translation layers we have that cause corrupted inputs?
❯ Is [#ownedpty] still relevant given we are implementing[#tmuxcontrolmode] and [#tmuxhybridpty]?
### Re: tmuxhybridpty — gpt-5

Short answer: control mode helps a lot, but it does not by itself avoid the Kitty vs non-Kitty input mapping layer.

`#tmuxcontrolmode` gives us an event-driven tmux control channel: pane output events, pane death/exit notifications, and a long-lived way to send tmux commands without repeatedly polling `capture-pane`. That is useful for lifecycle and observation. But when the command being sent is still `send-keys`, tmux remains the input synthesizer. The bytes that reach the child process are still shaped by tmux's key model, the pane's terminal modes, and the child TUI's keyboard protocol expectations.

That means control mode can replace some fragile `capture-pane` polling and can make routing/state transitions easier to prove, but it does not bypass the translation layer that caused the OpenCode submit corruption. The current code already shows this boundary: `tmux-router::Tmux::send_keys()` sends literal text, sleeps, then sends named `Enter`, while OpenCode needs a separate `send_keys_with_kitty_return()` path that writes the Kitty Return sequence. Agent-doc mirrors that in `sessions.rs` for managed OpenCode panes. Those branches would still exist if the command was delivered through a control-mode client, because the target operation would still be tmux `send-keys`.

So the right shape is:

- `#tmuxcontrolmode`: use for tmux lifecycle/events and to avoid stale snapshot polling.
- `pipe-pane` or control-mode `%output`: use for streaming output where possible, so we stop depending on lossy `capture-pane` views for live state.
- Small harness input adapter: keep only the minimum mapping needed for tmux-backed input, ideally centralized around submit/navigation primitives rather than scattered conditionals.
- `#ownedpty`: keep as the escape hatch for byte-exact managed harness input, not as a blanket replacement for tmux layout management.

`#ownedpty` is still relevant, but narrower than the backlog wording can imply. It is not needed to replace all tmux usage. Tmux is still valuable for panes, layout, attach/detach, stash behavior, and operator-visible sessions. Owned PTY matters when the requirement is: agent-doc must inject or mediate input as the actual terminal endpoint, without asking tmux to reinterpret a key. That is the only path that can truly eliminate the Kitty/non-Kitty `send-keys` distinction for managed panes, because agent-doc can negotiate or emulate the terminal protocol and write the exact byte stream to the child PTY.

I would treat `#tmuxhybridpty` as the umbrella architecture and `#ownedpty` as its phase-3 backend for managed panes where input correctness is still not provable through tmux. We should not delete `#ownedpty`; we should keep it scoped to managed Claude/OpenCode/Codex panes and avoid using it as a general-purpose tmux replacement.
<!-- agent:boundary:c9d9a1a2 -->
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
- [ ] [#tmuxhybridpty] Implement hybrid control-mode + pipe-pane + owned-PTY architecture: control mode for lifecycle events, pipe-pane for output streaming, owned PTY only for managed panes (OpenCode, Claude) where input reliability matters. Depends on #tmuxcontrolmode.
- [ ] [#ownedpty] Implement portable-pty + alacritty_terminal backend for managed panes — agent-doc owns the child PTY directly instead of tmux send-keys/capture-pane, eliminating the input heuristic failure class. Depends on #multiplexertrait abstraction boundary. [recommended]
- [ ] [#opencodeinputdiag] #agent-doc-bug: Add structured diagnostic logging for all tmux input paths — supervisor stdin forwarding, send-keys dispatch, Kitty keyboard protocol state, permission prompt detection transitions. Emit a log line per key event with source/destination/transform so regressions can be caught with log assertions, not manual testing.
- [ ] [#tmuxe2etests] #agent-doc-bug: Create end-to-end tmux input test harness that launches real tmux sessions (in CI-safe headless mode) and verifies Enter, arrow keys, and prompt submit through the actual supervisor→tmux→harness pipeline. Current SimWorld tests verify logic but not the tmux IPC surface.
- [ ] [#multiplexertrait] Evaluate Multiplexer trait abstraction over tmux to reduce coupling (785 tmux references across 43 files). Would allow future Zellij backend and make the tmux surface testable via mock backend. [recommended]
- [ ] [#sshcapfalsecapture] #agent-doc-bug: Required-SSH guard falsely failed a Monster Rod Holders `agent-doc` run by matching historical `.agent-doc/captures/...json` text containing `socket: Operation not permitted`; `command_execution_required_ssh_failure()` scans arbitrary command output before proving the command was an SSH probe. Plan: tasks/agent-doc/plan-required-ssh-false-positive-historical-capture.md
- [ ] [#codexverbosepb] #agent-doc-bug: Codex child-run response patchback is too verbose because `src/agent-doc/src/agent/codex.rs` concatenates every `item.completed` `agent_message`, so progress/status chatter from a Monster Rod Holders run was committed into `tasks/monsterrodholders.md` along with the final closeout. Plan: tasks/agent-doc/plan-codex-response-patchback-verbose-progress.md
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->
- 2026-05-13 [#donearchiveattr] Implement external completed-work archive support for agent:done using archive=<repo-relative>.done.md. Plan: tasks/agent-doc/plan-external-done-archive-attribute.md
<!-- /agent:done -->
