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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Landed #jbrsrbusyint pre-interrupt step; #jbrsrbusysim + auto-compact-default still pending.
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
### Re: do [#jbrsrbusyjb] — opus-4-7


Landed phase 2 of `plan-jb-restart-supervisor-busy-refusal.md`. The JetBrains Restart Supervisor action now detects the binary's alive-busy refusal, surfaces a warning notification with a "Force restart" action, prompts for explicit user confirmation, and re-invokes the binary with `--force` on accept. Phase-1 wiring (the `--force` flag and refusal-message hint in the Rust binary) already landed under `#jbrsrbusy`; the IDE side was the only missing piece for the user-visible UX.

#### Kotlin changes (`TerminalUtil.kt`)

- **`BUSY_RESTART_REFUSAL_HEADER_REGEX`** — anchors on `session_restart refused for (.+?) because pane (\S+) is alive-busy`. Matches both the bare binary error and the `agent-doc command failed (exit 1): Error: ...` wrapper that `runDocumentCommand` produces.
- **`BusySessionRestartRefusal`** data class — mirror of `BusySessionClearRefusal` (file, pane, source, currentCommand, tail). No `protectedReason` field; restart only has the busy-pane shape, not the protected-prompt-input shape.
- **`parseBusySessionRestartRefusal(output)`** — uses the new regex + the existing `BUSY_CLEAR_SOURCE_REGEX` / `BUSY_CLEAR_COMMAND_REGEX` extractors and the shared `extractBusyClearTail` helper. Returns `null` when no match (the caller falls through to a generic error notification).
- **`restartSessionInternal(project, file, force, onComplete)`** — replaces the prior single-shot `restartSession` body. Builds `["restart-supervisor"]` or `["restart-supervisor", "--force"]` per `force`. Routes failure through a new `onFailure` callback that parses the output via `parseBusySessionRestartRefusal`; on match (and not already in force mode) it calls `notifyBusySessionRestartBlocked`, otherwise it surfaces the raw `agent-doc command failed (exit N)` text. `force = true` failures intentionally bypass the busy parse and report verbatim so the user sees real errors from the force path.
- **`forceRestartSession(project, file, onComplete)`** — public entry point that the action chain reuses after confirmation. Internally calls `restartSessionInternal(force = true)`.
- **`notifyBusySessionRestartBlocked(...)`** — non-blocking warning notification with three actions: `Force restart` (opens the confirmation dialog), `Show status` (runs `agent-doc session status`), `Copy details` (clipboard-copies the raw output for support).
- **`confirmAndForceRestartSession(...)`** — modal `Messages.showYesNoDialog` on the EDT: "Supervisor is busy on a running turn for `<relativePath>`. Pane `<pane>` (`<current_command>`) will be interrupted and the supervisor restarted. Proceed?" with `Interrupt and restart` / `Cancel` buttons and the warning icon. Only the explicit Yes path calls `forceRestartSession`; Cancel is a no-op.
- **`buildBusySessionRestartBlockedMessage(...)`** — message-builder helper, extracted so the wording is unit-testable independent of the IntelliJ notification plumbing.

Surface contract:

- Stderr is **not** swallowed: the bare-failure branch and force-failure branch both call `notifyError` with the exit code + raw output, satisfying the plan's "do not silently swallow stderr" requirement.
- `Restarting...` / `Force-restarting supervisor for <file>` shows as an inline hint via `showHint` when the command is dispatched, satisfying "surface restart progress in the IDE notification area."
- The notification group reuses the existing `Agent Doc` group (no `plugin.xml` change needed).

#### Tests (`TerminalUtilTest.kt`)

Four new tests:

1. `busy restart refusal parses alive busy details and force hint` — full binary refusal output (with the `--force` hint suffix), asserts file/pane/source/currentCommand/tail extraction.
2. `busy restart refusal parses generic command wrapper output` — wrapped `agent-doc command failed (exit 1): Error: ...` form, asserts the parser still recovers all fields.
3. `busy restart refusal returns null when output is unrelated` — fail-safe guard so the busy branch is not taken for unrelated errors.
4. `busy restart blocked message advertises force restart action` — message-text assertions: contains `Restart Supervisor Process is blocked`, `Pane %13 is busy (agent-doc)`, `Force restart`, `Show status`, `Latest pane output: ...`, and does **not** contain the generic `agent-doc command failed` text.

`./gradlew test --tests "com.github.btakita.agentdoc.TerminalUtilTest"` → BUILD SUCCESSFUL. Full `./gradlew test`: BUILD SUCCESSFUL, 13 test reports clean.

#### Versioning + build

- Bumped `gradle.properties` `pluginVersion` 0.2.130 → 0.2.131 per the per-edit JB-plugin version-bump rule. Without the bump, IDEA dedupes install-from-disk by version string and would leave the prior bytecode cached.
- `./gradlew buildPlugin signPlugin` produced both:
  - `build/distributions/agent-doc-jetbrains-0.2.131.zip` (unsigned)
  - `build/distributions/agent-doc-jetbrains-0.2.131-signed.zip` (signed)

#### How to use

User clicks **Restart Supervisor Process** in JetBrains:

- **Idle pane** → existing behavior: invokes `agent-doc session restart-supervisor <FILE>`, shows hint, done.
- **Busy pane** → binary returns the new refusal with `--force` hint suffix → plugin parses it → warning notification appears with `Force restart` / `Show status` / `Copy details` actions. Click **Force restart** → modal confirm dialog → click **Interrupt and restart** → plugin invokes `agent-doc session restart-supervisor --force <FILE>` → idle hint on success or raw `notifyError` on failure.

#### Remaining plan parts

- `#jbrsrbusyint` — optional harness-aware pre-interrupt step in the binary (Ctrl-C / harness escape with 2s idle wait, fall through to `--force` if still busy).
- `#jbrsrbusysim` — SimWorld scenario + the two remaining telemetry events (`session_restart_busy_pre_interrupt_idle`, `session_restart_busy_force_killed`).

Both still in `agent:backlog`, ordered as `#jbrsrbusyint` → `#jbrsrbusysim` (the pre-interrupt code is the natural dependency for the `pre_interrupt_idle` event to be observable in the SimWorld harness).
❯ preset #spec-test-build-install-commit-push
❯ - do [#jbrsrbusyint]
❯ - do [#jbrsrbusysim]
❯ - Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
### Re: queue auto-dispatched 3 prompts at once + do [#jbrsrbusyint] — opus-4-7


The auto queue fed all three remaining items into the exchange as `❯`-prefixed prompts in a single batch. I landed `#jbrsrbusyint` this cycle and left `#jbrsrbusysim` and the auto-compact-exchange-default prompt for follow-up cycles; the queue head will advance to `#jbrsrbusysim` on the next `/agent-doc` invocation.

#### `#jbrsrbusyint` — pre-interrupt step in `session_actor_cmd::restart`

Plan phase 3 calls for a harness-aware pre-interrupt before the hard supervisor kill in the `--force` path. Landed in `session_actor_cmd.rs`:

- **Entry point.** In `restart(file, mode, force)`, when `force == true` the existing `session_restart_force_used` audit-log fires first, then `attempt_pre_interrupt_for_force_restart(&ctx, &tmux, mode)` runs. The supervisor IPC kill that follows is unchanged.
- **`attempt_pre_interrupt_for_force_restart`.** Reads `live_pane_evidence`; returns early if the pane is not `AliveBusy` (idle / dead panes skip the whole step). Logs `session_restart_busy_pre_interrupt_attempt file=... pane=... harness=... mode=...` before sending keys. Reuses the existing `send_operator_interrupt_sequence` so the cancel sequence matches the harness — Codex gets `C-g` + Escape + `C-c`, OpenCode gets a double-Escape, others get bare `C-c`.
- **Settle wait.** New `wait_for_force_restart_pre_interrupt_settle(ctx, tmux, pane, timeout)` polls live pane evidence every `FORCE_RESTART_PRE_INTERRUPT_POLL` (200 ms) until idle, until the pane closes, or until `FORCE_RESTART_PRE_INTERRUPT_WAIT` (2000 ms — the constant the plan specifies) expires. Returns a `ForceRestartPreInterruptOutcome` enum (`Idle` / `Closed` / `TimedOut`).
- **Telemetry events.** Plan phase 4 names two new variants; both now land:
  - `session_restart_busy_pre_interrupt_idle file=... pane=...` — the cancel sequence drained the turn cleanly within 2 s. Stderr also gets a `[session-restart] pane <pane> drained to idle after pre-interrupt` line so operator history is human-readable.
  - `session_restart_busy_force_killed file=... pane=... reason=pre_interrupt_timeout wait_ms=2000` — the pane stayed busy and the supervisor IPC kill proceeds. A sibling `reason=interrupt_send_failed err=...` variant covers the failed-`send_keys` case so that path is also auditable.
  - Bonus: `session_restart_busy_pre_interrupt_pane_closed file=... pane=...` for the rare case where the pre-interrupt closes the pane entirely; this still lets the supervisor kill proceed without surfacing a spurious force_killed.
- **Best-effort posture.** Every failure path (send-keys error, tmux query failure inside `live_pane_evidence`) logs and falls through to the supervisor IPC kill. The `--force` contract is preserved: the user's explicit kick-this-loose gesture always reaches the supervisor restart.

#### Tests

Two unit tests in `session_actor_cmd::tests`:

- `force_restart_pre_interrupt_outcome_variants_are_distinct` — pins the enum's three-way distinction. The caller emits a different ops_log event for each variant, so collapsing two of them would silently flatten the audit trail.
- `force_restart_pre_interrupt_timing_constants_match_plan` — locks `FORCE_RESTART_PRE_INTERRUPT_WAIT == 2000 ms` (the plan-specified value) and asserts the poll cadence stays ≤ 500 ms.

The loop body itself reads tmux + live pane evidence, which needs real tmux to exercise. The SimWorld coverage for that is owned by `#jbrsrbusysim` (next item).

`cargo test --bin agent-doc session_actor_cmd::` → **37 passed, 0 failed** (35 prior + 2 new). Library suite **2340 passed** (+2). `run_integration` **14**, `test_cli` **65** including the FlowCore budget gate. `cargo build --release` clean; `cargo install --path .` refreshed `~/.cargo/bin/agent-doc v0.33.16`.

The pre-existing PoisonError cascade across `plan::tests::*` / `preflight::tests::*` / `tsift_graph::tests::*` only reproduces in the threaded full-suite run; each test passes in isolation. Same pattern flagged earlier and unrelated to this cycle.

#### Re: `#jbrsrbusysim` (queued head, not done this cycle)

The pre-interrupt code now exists, so the SimWorld harness can drive both `pre_interrupt_idle` and `force_killed` paths deterministically:

- Fixture: a SimWorld pane that reports `alive-busy` initially and transitions to `alive-idle` after a configured key-sequence is observed → exercises the `Idle` outcome.
- Fixture: a SimWorld pane that ignores cancel keys → exercises the `TimedOut` outcome (must be tagged ignored or wired to the configured wait so the deterministic harness does not real-sleep 2 s).
- Assertions on the ops_log lines listed above.

This is one tracked item, not three; ready to pick next cycle.

#### Re: "auto-compact exchange off by default" (queued tail prompt)

This is a behavior change to the default for `agent_doc_auto_compact` / `auto_compact_exchange` in `frontmatter.rs` / `project_config.rs` / `compact.rs`. Concretely:

- `frontmatter::Frontmatter::resolve_*` should treat absent `agent_doc_auto_compact` as `false`.
- `project_config.rs` `auto_compact_exchange` field needs the same default flip.
- The auto-compact trigger sites (preflight session-accretion guidance, runtime compaction calls) must read the resolved value and short-circuit when it's `false`.
- Tests covering the prior default behavior need to be updated; the README / SPEC sections that describe the default need to be edited in the same turn.

That is a separate small change but touches enough files that it deserves its own focused cycle rather than being bundled here. Captured as the next backlog item below.

#### Remaining plan parts

- `#jbrsrbusysim` — SimWorld + telemetry coverage now unblocked by this cycle.
- New backlog entry needed for the auto-compact-default flip; capturing it via `--pending-add` on this finalize so the work isn't lost.
<!-- agent:boundary:c3857972 -->
<!-- /agent:exchange -->
###
<!--
Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#stuckcyclesuperseded]~
- ~do [#runintegcodexbare]~
- ~do [#jbrsrbusyjb]~
- do [#jbrsrbusyint]
- do [#jbrsrbusysim]
- Please make auto-compact exchange off by default. Only explicit frontmatter or .agent-doc/config.toml setting should make auto-compact exchange active.
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [x] [#jbrsrbusyint] Optional harness-aware pre-interrupt step before restart: send Ctrl-C / harness escape, wait up to 2s for idle, fall through to --force when still busy (plan phase 3). Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
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
