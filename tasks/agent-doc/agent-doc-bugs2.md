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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260514-165042.md`*

Compacted content:
- Archived 6 response topic(s): #clearstartingrace; #ownedreapbusy; #opencodeinputdiag; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: Zellij vs tmux reliability — gpt-5

Zellij could help with some presentation and layout concerns, but I would not expect it to solve the important agent-doc reliability problems by itself.

The fragile parts we keep seeing are mostly above the terminal multiplexer boundary: detecting prompt readiness, distinguishing accepted input from actually dispatched input, tracking authoritative actor state, preserving output captures without false positives, and avoiding stale historical logs influencing current guard decisions. Those would still exist with Zellij if agent-doc continues to drive an interactive PTY and infer state from terminal text.

Where Zellij might help:

- Structured layout/session behavior may be cleaner than our tmux-pane lifecycle in some cases.
- Its UI could make managed panes easier to inspect manually.
- A backend abstraction for `tmux` vs `zellij` could clarify which failures are multiplexer-specific and which are agent-doc state-machine bugs.

Where it probably would not help:

- Input delivery proof still needs a reliable dispatch/start signal, not just pane write success.
- Output capture still needs scoped, current-run evidence instead of arbitrary terminal history or saved JSON text.
- Busy/idle ownership still needs an agent-doc lifecycle model that is explicit about starting, running, prompt-ready, dispatching, blocked, killed, and restarted states.

So my recommendation is: do not switch to Zellij as a primary fix. First harden the multiplexer-agnostic contract inside agent-doc: authoritative actor state, prompt-gated dispatch, scoped output evidence, and deterministic lifecycle transitions. After that, Zellij is worth a small adapter spike if we want better UX or a comparison backend, but it should be treated as an implementation option rather than the reliability foundation.
Evaluate issues and the effectiveness of agent-doc in the logs. #next-steps
### Re: log evaluation and next steps — gpt-5

The logs say agent-doc is effective at the core document-session contract, but still noisy and brittle at the terminal lifecycle boundary.

What is working:

- Response persistence is strong. The latest completed cycle has a committed capture with `write_applied_at`, `committed_at`, `response_sha256`, and `state: committed`; ops also shows recent `ipc_write_consumed` and `commit_success` events for this document.
- Dispatch proof is much better than pane-write proof. The recent ops summary shows 14 `route_dispatch_start_proven` events with `proof=consumed`, and the current route events record `proof_scope=dispatch_start`, which is the right reliability target.
- The supervisor/controller state is useful. `agent-doc session status` reports a healthy supervisor, a ready authoritative actor, capability proof as proven, and concrete generation/pane ownership state.
- Safety guards are doing real work. `session_clear_protected_input_guard_refused` prevented clearing a pane that appeared to contain drafted prompt input, which is preferable to losing user input.

Issues still visible:

- The logs are dominated by expected-but-confusing drift: ops summary reports 33 recent `post_commit_local_drift` events for this document, mostly user follow-up edits after committed cycles. That is legitimate state, but it reads like an error stream.
- Actor startup remains a race window. Recent logs include `route_authoritative_actor_starting_not_ready` before the same generation later becomes ready and dispatch proceeds. The recovery works, but the operator experience is noisy and timing-dependent.
- Clear/interrupt still has an unresolved edge. The logs show `session_interrupt_clear_settled ... outcome=timed_out` after a protected clear refusal. That means the guard prevented damage, but the recovery path can still leave the user without a crisp next command.
- Hook and prompt blockers are not yet explained well enough. One route was blocked with `reason=codex hook review prompt`; that is useful as telemetry, but it should become a direct user-facing blocker with recovery instructions.
- The preflight debounce and truncation checks are useful, but still visible as rough edges. This run itself waited for a possibly truncated line to stabilize before diffing. Correct behavior, but it is another sign that file-change detection should expose less internal machinery to the user.

My read: agent-doc is now reliable enough at writeback, commit closure, and dispatch proof for serious use. The next reliability work should not be a multiplexer swap. It should be tightening the lifecycle state machine and reducing ambiguous operator feedback around `starting`, `busy`, protected input, hook prompts, and post-commit drift.

I added four concrete backlog items from this log review: route readiness, interrupt-clear timeout reporting, ops summary noise reduction, and Codex hook route-blocker messaging.
<!-- agent:boundary:64bb7ce8 -->
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
- [ ] [#opsreadinessrace] Tighten route readiness handling: avoid dispatch/clear attempts while the authoritative actor is `starting`; surface a single actionable wait/retry state until `prompt_ready=true`.
- [ ] [#interruptcleartimeout] Investigate `session_interrupt_clear_settled outcome=timed_out` after protected clear and make interrupt-clear report the blocking state and last command deterministically.
- [ ] [#opsnoisereduce] Reduce `post_commit_local_drift` / `commit_noop` noise by separating expected user follow-up edits from anomalous drift in `agent-doc ops summary`.
- [ ] [#hookreviewroute] Clarify `route_dispatch_only_blocked reason=codex hook review prompt` as a user-facing route blocker with recovery guidance.
- [ ] [#sshcapfalsecapture] #agent-doc-bug: Required-SSH guard falsely failed a Monster Rod Holders `agent-doc` run by matching historical `.agent-doc/captures/...json` text containing `socket: Operation not permitted`; `command_execution_required_ssh_failure()` scans arbitrary command output before proving the command was an SSH probe. Plan: tasks/agent-doc/plan-required-ssh-false-positive-historical-capture.md
- [ ] [#codexverbosepb] #agent-doc-bug: Codex child-run response patchback is too verbose because `src/agent-doc/src/agent/codex.rs` concatenates every `item.completed` `agent_message`, so progress/status chatter from a Monster Rod Holders run was committed into `tasks/monsterrodholders.md` along with the final closeout. Plan: tasks/agent-doc/plan-codex-response-patchback-verbose-progress.md

Would ZelliJ help with input and output reliability over tmux?
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->
- 2026-05-13 [#donearchiveattr] Implement external completed-work archive support for agent:done using archive=<repo-relative>.done.md. Plan: tasks/agent-doc/plan-external-done-archive-attribute.md
<!-- /agent:done -->
