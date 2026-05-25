---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Queue manual-mode acknowledged; awaiting next explicit pick.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-233421.md`*

Compacted content:
- Archived 2 response topic(s): do [#stuckcycledup]; do [#stuckcycleorphan] + do [#stuckcyclerepair]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: queue manual mode + struck-out done items — opus-4-7


Acknowledged the queue edits. No new repo work this cycle — preflight classified the diff as `task_class: prompt_response, model_tier: low` (content-edit only, no `prompt_target`), so this is a steering acknowledgement and no actionable follow-up items were uncovered beyond the three I had already split out in the previous cycle.

Three things changed in the queue component:

- `auto` attribute removed from `&lt;!-- agent:queue --&gt;`. The auto-advance is off; the next `/agent-doc` invocation will not auto-pull a head item — explicit pick required.
- `~do [#stuckcyclesuperseded]~` and `~do [#runintegcodexbare]~` struck through. Both are already in `agent:done` archive (commits 9d20d6a2f and 39844dc57); the strike-through is cosmetic.
- Three new explicit items now top the queue: `do [#jbrsrbusyjb]`, `do [#jbrsrbusyint]`, `do [#jbrsrbusysim]` — the deferred phase-2/3/4 sub-plans split out from `#jbrsrbusy` in the previous cycle. They appear in both `agent:backlog` and the manual queue, ready for picking.

#### Possible next steps

The three queued items have different cost profiles, so explicit ordering matters:

- `#jbrsrbusyjb` — JetBrains Kotlin/IntelliJ plugin work. Separate build/test cadence from the Rust binary (`./gradlew buildPlugin signPlugin`) and a manual install step before live testing.
- `#jbrsrbusyint` — pre-interrupt step in the binary. Pure Rust, tests in `session_actor_cmd`, builds on the `--force` plumbing already live.
- `#jbrsrbusysim` — SimWorld scenario + the remaining two telemetry events (`session_restart_busy_pre_interrupt_idle`, `session_restart_busy_force_killed`). Depends on the pre-interrupt code from `#jbrsrbusyint` for the `pre_interrupt_idle` event to be observable.

If `#jbrsrbusyint` lands first, `#jbrsrbusysim` can cover both new event variants in one pass. `#jbrsrbusyjb` is independent of those two and can go in any order.

Also still in the queue tail: the remaining `#jbcc*` items (`#jbccfix`, `#jbccreg`, `#jbccdocs`) and the trailing prompt about defaulting auto-compact exchange off (which would touch `compact.rs` / `frontmatter.rs` / `project_config.rs`).

Ready for your explicit next pick.
<!-- agent:boundary:41d813bc -->
<!-- /agent:exchange -->
###
<!--
Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#stuckcyclesuperseded]~
- ~do [#runintegcodexbare]~
- do [#jbrsrbusyjb]
- do [#jbrsrbusyint]
- do [#jbrsrbusysim]
- Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#jbrsrbusyjb] JetBrains Restart Supervisor action: prompt 'Supervisor is busy on a running turn. Interrupt and restart?' when the pane is alive-busy, invoke agent-doc session restart <FILE> --force on confirm, surface restart progress in the IDE notification area without swallowing stderr (plan phase 2). Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [ ] [#jbrsrbusyint] Optional harness-aware pre-interrupt step before restart: send Ctrl-C / harness escape, wait up to 2s for idle, fall through to --force when still busy (plan phase 3). Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [ ] [#jbrsrbusysim] SimWorld + IDE telemetry coverage for session_restart --force path, including session_restart_busy_pre_interrupt_idle and session_restart_busy_force_killed event variants (plan phase 4). Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jbrsrbusy] [partial] Phase 1 (binary) landed: agent-doc session restart <FILE> --force flag accepts alive-busy panes, refusal message now ends with the --force hint citing the exact command, and the force path logs session_restart_force_used. Plan phases 2 (JB plugin confirm dialog → invokes --force), 3 (optional harness-aware pre-interrupt with 2s idle wait), and 4 (SimWorld coverage + IDE-side telemetry) split into #jbrsrbusyjb, #jbrsrbusyint, and #jbrsrbusysim. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [/] [#stuckcycledup] [recommended] [partial] Wedged-cycle pattern: cycle_already_committed gate now refuses the short-circuit when the active capture body is absent from HEAD (flow/closeout.rs::cycle_already_committed + new stuck_captured_cycle helper), and preflight emits a stuck_captured_cycle warning. Remaining plan parts split into #stuckcycleorphan, #stuckcyclerepair, and #stuckcyclesuperseded. Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [/] [#jbresstart] Refuse cross-file pane rescue and extend Starting-actor wait after stash recovery; see tasks/agent-doc/plan-jb-route-rescued-stash-pane-starting-actor.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
