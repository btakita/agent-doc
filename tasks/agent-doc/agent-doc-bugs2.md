---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260513-210617.md`*

Compacted content:
- Archived 1 response topic(s): OpenCode Enter submit
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ JB `Clear Session Context` error on this document:
```
Clear Session Context is blocked for tasks/agent-doc/agent-doc-bugs2.md. Pane %1 contains protected prompt input (drafted prompt input). Use Interrupt and clear to discard the prompt input, or Show status to inspect the session. Latest pane output: gpt-5.5 high · ~/work/btakita/agent-loop · Context 28% used
```
❯ spec-test-build-install-commit-push
### Re: JB Clear Session Context harness mismatch — opus-4-6

**Root cause:** `protected_clear_input_reason` always used the document's configured harness (`ctx.harness` from frontmatter) for draft-input detection. When `agent: codex` but pane %1 runs OpenCode, the Codex harness's prompt heuristics misclassified OpenCode TUI output as "drafted prompt input".

**Fix:** Added `HarnessConfig::from_pane_command(cmd)` that infers the actual harness from `pane_current_command` when unambiguous (`codex`, `opencode`, `claude`, `bun`), falling back to the document's configured harness for ambiguous commands (`node`, `agent-doc`, shell). Updated `protected_clear_input_reason` to use this.

**Files changed:**
- `src/harness.rs` — `from_pane_command()` + 3 test functions
- `src/session_actor_cmd.rs` — harness inference in `protected_clear_input_reason`
- `specs/07-session-tmux-commands.md` — documented the harness inference rule

**Verification:** 2592 tests pass. Pre-existing `codex_bare_run_inside_owning_pane` failure is unrelated. Built, installed, committed, pushed.
❯ agent-doc running opencode still has input issues with the prompt mode...caused by the supervisor:
❯ - Enter
❯ - All arrow keys
❯ - Selecting and submitting prompts

❯ Please add ample logging and hook detection to debug the issue. These issues keep on reoccurring and take many turns to fix. The majority of my development on agent-doc is fixing issues between agent-doc and tmux. The issue seems architectural.

❯ Why is the test suite passing yet the product has critical regressions over and over and over and over and over again?

Is tmux itself causing complications with the inputs? Is there a better tiled terminal emulator than tmux that has an api?

Would a Rust port of https://github.com/tmux-python/libtmux help with the issues?

Or is this an issue with the supervisor?

#next-steps
### Re: OpenCode input issues & tmux architecture — opus-4-6

#### The input problem

The supervisor has three separate input paths for OpenCode, each with its own failure modes:

1. **Stdin forwarding** (`start.rs`) — supervisor captures raw stdin bytes and forwards them to the PTY. It intercepts arrow keys during permission prompts and translates them to Tab/BackTab. If `opencode_permission_prompt_active()` misdetects state, normal arrow keys get swallowed or mistranslated.

2. **Send-keys dispatch** (`sessions.rs`, `tmux.rs`) — text submission uses `send-keys -l <text>` + 50ms delay + Kitty Return (`\x1b[13u`). The delay is a race against OpenCode's composer. Too short = dropped submit. Too long = visible lag.

3. **Kitty keyboard protocol** (`supervisor/pty.rs`) — OpenCode's TUI depends on Kitty keyboard protocol sequences. The PTY filter has a `preserve_kitty_keyboard` flag that must be true for OpenCode. If this flag is wrong, or if the filter strips/mangles sequences, all special keys break.

Each path has per-harness branching. When one of these three is slightly off, you get exactly the symptoms you described: Enter doesn't submit, arrows don't navigate, prompts can't be selected.

#### Why the test suite passes while the product breaks

The tests verify logic, not I/O. SimWorld tests mock tmux's response surface — they call functions like `translate_opencode_permission_arrow_keys()` with known byte sequences and assert the output. What they don't test:

- **Actual tmux `send-keys` timing** — the 50ms delay is a heuristic. Real terminal I/O is nondeterministic.
- **PTY filter state machines under real traffic** — the Kitty keyboard filter sees synthetic sequences in tests, never real terminal negotiation.
- **Permission prompt detection against live OpenCode output** — the regex/heuristic matches test fixtures, not the evolving OpenCode TUI.

This is the classic mock-prod divergence: the test suite validates the model, but the model doesn't capture the actual tmux IPC surface. Every OpenCode TUI update or tmux version change can shift the timing or output patterns enough to break detection without touching any tested code path.

#### Is tmux the problem?

Partly. tmux's `send-keys` is fundamentally fire-and-forget — there's no acknowledgment that the target pane received or processed the input. The 50ms delay between text and submit is a best-effort workaround for this. tmux also interposes its own input processing layer between the supervisor and the child process, adding another state machine that can eat or reinterpret key sequences.

That said, **every terminal multiplexer has this problem**. Zellij has a similar `write-chars` command with no ack. Wezterm's mux doesn't expose send-keys at all. The Kitty terminal has direct keyboard protocol support but isn't a multiplexer.

The better framing: tmux is the *only* multiplexer with a rich enough CLI API to build this on. The issue isn't tmux per se — it's that **programmatic input injection into terminal applications is inherently unreliable** regardless of multiplexer.

#### Would a Rust port of libtmux help?

No. libtmux is a Python wrapper around `tmux` CLI commands — it shells out to `tmux send-keys`, `tmux list-panes`, etc. A Rust port would be `tmux-router` (which you already have). The limitation is tmux's CLI surface itself, not the language binding.

#### Is this the supervisor's fault?

The supervisor adds necessary complexity (harness detection, permission prompt interception, Kitty protocol filtering) but each layer is a new place for state to desync. The root tension:

- OpenCode needs Kitty keyboard protocol → supervisor must preserve those sequences
- Permission prompts need arrow→Tab translation → supervisor must detect prompt state
- Submission needs Kitty Return → supervisor must know which harness is running
- All of this runs through tmux's fire-and-forget `send-keys`

The supervisor is doing the right things, but **the detection heuristics are fragile** because they pattern-match against terminal output that changes with every harness update.

#### Next steps

The highest-leverage fix is adding structured diagnostic logging across all three input paths so that the next regression can be diagnosed from logs in one turn instead of many. Second priority is real tmux integration tests that catch the mock-prod gap. I've added both to the backlog along with a recommended longer-term Multiplexer trait evaluation.
<!-- agent:boundary:089d0544 -->
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
`[#2fha] 2026-05-11 [#nopatchbackopencode] ...` backlog item should have had the id `[#nopatchbackopencode]`.
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
