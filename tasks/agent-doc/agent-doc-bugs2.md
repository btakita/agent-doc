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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260514-043506.md`*

Compacted content:
- Archived 1 response topic(s): #orchplainresp
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

❯ do [#clearstartingrace]
❯ synch orchestra
❯ dispatch #spec-test-build-install-commit-push
❯ - do [#clearstartingrace]
❯ - do [#ownedreapbusy]
❯ - do [#opencodeinputdiag]
❯ - do [#tmuxe2etests]
### Re: #clearstartingrace — gpt-5

Implemented `#clearstartingrace`.

Clear/restart now run a startup-window guard after controller acceptance and before any pane input or supervisor mutation. If the actor record, matching or legacy session-scoped supervisor runtime, or matching supervisor lease still reports `starting`, `session clear` only proceeds with a dispatch-ready composer and a clean post-commit document hash; `session restart` can also proceed at the clean-exit restart prompt. Refusals log `session_operator_starting_guard_refused` and avoid sending literal Enter or `/clear` into a pane that has not reached the right prompt.

I also updated the tmux/session specs and `VERSIONS.md`, and added regressions for stale starting records, live runtime/lease override behavior, legacy supervisor runtime payloads, clean-exit prompt handling, and post-commit document drift.

Verification passed:
- `cargo test session_actor_cmd::tests::operator_starting_guard`
- `cargo test session_actor_cmd::tests::clear_falls_back_to_supervisor_inject_when_authoritative_pane_is_not_on_default_tmux -- --ignored --test-threads=1`
- `make check`
- `make build`
- `make install && agent-doc --version` -> `agent-doc 0.33.16`
- `make tmux-ci` -> 227 live/ignored tmux tests passed
- Latest GitHub Actions on `main`: `CI/check`, mdBook `build`, and mdBook `deploy` are green; no separate recent hosted tmux job was present, so local `make tmux-ci` is the tmux evidence.

Commits prepared:
- `src/agent-doc` `720c671c` (`fix: guard clear during starting actor windows`)
- parent repo `1c05f9836` (`agent-doc: guard clear during starting actor windows`)

I will push after this finalize/session-check boundary so the session closeout commit is included in the parent push.

❯ do [#ownedreapbusy]
### Re: #ownedreapbusy — gpt-5

Implemented `#ownedreapbusy`.

Route-owned completion no longer keeps a one-shot pane alive just because the visible tail still contains transient renderer text. The reap guard now treats the supervisor actor's stable `ready` prompt state as proof that the committed child is idle, while still preserving panes for explicit blocking states such as queued drafts, permission prompts, hook-review prompts, history search, and clean-exit restart prompts. I also stripped OSC title updates in the managed PTY filter so title fragments like `Working ... esc to interrupt` cannot enter prompt sampling.

Updated the route-owned docs/specs and `VERSIONS.md`, and added regressions for stale Codex `Working...` tails, blocking queued-composer state, and split OSC title sequences.

Verification passed:
- `cargo test route_owned_live_pane_busy -- --nocapture`
- `cargo test filter_strips_osc_title_updates -- --nocapture`
- `cargo test filter_stateful_osc_split_across_reads -- --nocapture`
- `cargo fmt --check`
- `make check`
- `make build`
- `make install && agent-doc --version` -> `agent-doc 0.33.16`
- `make tmux-ci` -> 227 live/ignored tmux tests passed

CI review: the latest GitHub Actions run on `main` was already red for both `check` and `tmux-ci`, but `gh run view --log-failed` returned `log not found` for both failed job IDs, so I could not retrieve hosted failure logs. The local full `check` and `tmux-ci` suites are green for this change.

Commits prepared:
- `src/agent-doc` `0d98e96a` (`fix: reap route-owned panes after ready prompt state`)
- parent repo `5d99d6c28` (`agent-doc: reap route-owned panes after ready state`)

I will push after this finalize/session-check boundary so the session closeout commit is included in the parent push.

❯ do [#opencodeinputdiag]
### Re: #opencodeinputdiag — gpt-5

Implemented `#opencodeinputdiag`.

Added structured `tmux_input_event` diagnostics across the tmux/supervisor input paths: route direct pane submits, route supervisor IPC submits, queue dispatch via supervisor and tmux, harness-aware tmux submit helpers, supervisor IPC/auto-trigger injection, stdin forwarding transforms, OpenCode permission prompt arrow translation, and Kitty keyboard protocol preserve/drop decisions. Text payloads are logged as byte length plus SHA-256 rather than raw prompt text, while each event names source, destination, transform, key, byte count, and harness when known.

Updated the supervisor/session-tmux specs and `VERSIONS.md` for the new diagnostic contract.

Verification passed:
- `cargo test input_diag -- --nocapture`
- `cargo test opencode_permission_prompt_translates_legacy_arrows_to_tab_controls -- --nocapture`
- `cargo test filter_preserves_kitty_keyboard_for_opencode -- --nocapture`
- `cargo test opencode_submit_uses_kitty_return_sequence -- --ignored --nocapture`
- `cargo fmt --check`
- `make check`
- `make build`
- `make install && agent-doc --version` -> `agent-doc 0.33.16`
- `make tmux-ci` -> 227 live/ignored tmux tests passed

CI review: latest hosted `CI` run on `main` (`25773430221`, May 13, 2026) is green for its `check` job. I did not find a separate recent hosted tmux job, so the required tmux evidence is the local `make tmux-ci` run above.

Commits prepared:
- `src/agent-doc` `abfa2357` (`feat: add tmux input diagnostics`)
- parent repo `ba2da7e1c` (`agent-doc: add tmux input diagnostics`)

I will push after this finalize/session-check boundary so the session closeout commit is included in the parent push.
<!-- agent:boundary:a0d69c5e -->
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
