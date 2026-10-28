---
name: supervisor
status: living
date: 2026-04-13
updated: 2026-07-22
---

# agent-doc Supervisor Spec

## Motivation

`agent-doc start` is a supervisor-lite: it wraps `claude` in a restart loop inside a tmux pane (see `src/start.rs:170`). It handles clean-exit prompts and non-zero auto-restart, but:

- **Has no pty** — `claude` inherits the tmux pane's tty directly. The supervisor cannot observe output, inject keystrokes, or detect prompt state.
- **CWD is inherited from the tmux pane** — whatever directory the pane happens to be in at spawn time, which is the root cause of the cross-project CWD drift bug this spec exists to fix.
- **Has no IPC** — external processes (editor plugins, `/agent-doc` routing from other panes) can only talk to the running claude via `tmux send-keys`, which is racy and cannot introspect state.
- **Crash recovery is a 2s sleep + `--continue`** — no escalation, no state inspection, no cooldown.

The supervisor graduates `start.rs` into a process that **owns** claude as a child behind a pty, holds a Unix-domain IPC socket per session, and enforces invariants (CWD, env, auto-restart cadence, external control) that a bare tmux-pane wrapper cannot.

## Implementation Status

| Submodule | Status | Notes |
|-----------|--------|-------|
| `supervisor/cwd.rs` | **landed** | Deterministic CWD resolution and source tagging are active in production. |
| `supervisor/pty.rs` | **landed** | `agent-doc start` now spawns the harness behind the supervisor-owned pty and forwards stdin/stdout through it. |
| `supervisor/screen.rs` | **landed** | The supervisor feeds filtered owned-PTY output into an `alacritty_terminal` screen model, so prompt/help/permission detection can inspect the current child viewport without relying on tmux `capture-pane` output. |
| `supervisor/resize.rs` | **landed** | Terminal resize events are forwarded to the child pty during the supervised session. |
| `supervisor/state.rs` | **landed** | Crash classification, restart cadence, waiting-input prompts, and halted-state handling are live. |
| `supervisor/ipc.rs` | **landed** | Per-session supervisor IPC serves `inject`, `restart`, `state`, `pid`, and `stop`. |
| `start.rs` wire-up | **landed** | The production `agent-doc start` path owns the supervisor lifecycle inline in `start.rs` while delegating focused pieces to `supervisor/*` modules. |
| Project controller registration | **landed** | Supervisor startup lazy-launches the project controller, registers the actor generation and supervisor lease, and reports lifecycle transitions through controller IPC. |
| `tmux-router` hybrid policy | **landed** | Control mode handles lifecycle/events, `pipe-pane` handles live output streams, and owned PTY input remains scoped to managed Claude/OpenCode supervisors that need byte-exact input. |

The original rollout plan in this document is retained for architectural context,
but the supervisor stack is now shipping. Current behavior should be read from
the "Implementation Status", "Actor state reporting", and the specs referenced
from the session-actor contract rather than from the old phased rollout notes.

## Non-Goals

- Not a tmux replacement. Supervisor still runs inside a tmux pane; tmux still owns the visual terminal the user sees.
- Not a sandbox. Supervisor does not restrict what claude can do — it just owns its lifecycle.
- Not a daemon for multiple sessions. **One supervisor = one claude = one session document.** Per-session daemons are easier to reason about than a global one.
- That boundary includes harness conversation identity. A replacement child must not use global "last conversation" selectors such as Claude `--continue`, Codex `resume --last`, or OpenCode `--continue`; those can attach an otherwise-correct pane/actor to another document's recent session. Continue-mode requires the exact durable harness-session binding and fails closed when it is unavailable; only explicit fresh-mode may start a new conversation.
- While the actor is live, its exact harness-session binding is controller-owned lifecycle state before, during, and after an in-place supervisor replacement. For Codex, the prompt-hook ledger supplies the exact thread-id source edge; child termination observes that edge once and launches `codex resume <thread-id>`, without polling. Frontmatter `agent_doc_session`/`resume` fields are document projections and durable cold-recovery seeds, not authority to replace or erase a proven live binding, and a bare `start --resume` must prefer the controller binding when that projection lags or is absent.
- A non-forced `restart-supervisor` accepted by a live supervisor is an owned drain-to-boundary handoff. In-place replacement requires both drained supervisor IPC and an idle harness-owned turn lease; a no-IPC instant during a live turn is not a safe replacement boundary. The request stays pending until the turn completes, so child adoption is recovery rather than the continuation mechanism for an interrupted turn. A controller-side proof timeout is observation only: it must not escalate to killing the supervisor or harness child. If the supervisor is already dead but the document's harness is still alive in its pane, continue-mode preserves that child and fails closed; only explicit fresh-mode may quit it.
- An explicit `restart-agent` may reclaim an open cycle at an actor `Ready` boundary only when repair proves the cycle is still an empty `preflight_started` record. Captured or otherwise advanced cycles remain protected from immediate child replacement. When a live child still owns such a cycle, the supervisor accepts and latches the restart intent without signaling the child; it releases that intent only after durable cycle closure and a later actor `Ready` prompt. Thus a busy operator request is neither dropped nor allowed to create two children on one pane.
- An operator **Clear Session Context** that needs a fresh child (`clear:fresh`) is a harness-context operation, not a document operation (`#clearcapturedcycle`). When the actor is at its dispatch-ready `Ready` prompt and the open cycle already holds a durable response capture (`response_captured`/`write_applied` with capture id and response hash), the child owns no turn: the remaining closeout is the supervisor's keyed captured-finalize resume, which needs no harness and keeps the retained write durable. The clear therefore replaces the child immediately instead of latching behind document convergence; the cycle stays open and closes on its own. The pure gate is `agent_doc_supervisor::lifecycle::operator_clear_restart_admission`; a busy actor, a `waiting_input` prompt, an uncaptured cycle, and Restart Agent keep the shared open-cycle latch. The `Clear` IPC answer reports `deferred_until_cycle_closes` and `captured_cycle_crossed`, and the client never prints "Cleared" for a latched clear: it logs `session_clear_deferred ... deferred_until=agent_doc_cycle_closes` and tells the operator the clear is accepted but not applied yet (or, after crossing a captured cycle, `session_clear_sent ... open_cycle=captured_supervisor_closeout`). A latched child-replacing request is not overwritten by a later supervisor-only `continue` recycle (install fan-out, `restart-supervisor`): that request answers `PendingReplacementRetained` and leaves the latch intact, matching the reexec path's `child_replacement_pending` refusal, so the stale generation recycles again after the replacement fires. Observed 2026-10-16 on `src/haiven-dev/tasks/sdk.md` (Codex): the retained write was blocked by `editor_holds_unmerged_visible_text`, two clears latched behind `agent_doc_cycle_open` while the client printed "Cleared", and the install's `session_restart mode=continue` then overwrote the latch, so no `restart_fresh_spawn` ever ran.
- Child replacement signals the foreground PTY process group, not only the direct harness PID, so harness-owned subagents and background descendants cannot survive into the replacement session.
- The controller may perform the remaining replacement asynchronously, but it does not return an accepted receipt until the worker's first supervisor lifecycle decision. A generic live-supervisor refusal is returned as an error rather than appearing later only as `background_failed` telemetry; an open-cycle deferred restart is an accepted supervisor-owned lifecycle decision, not a refusal.
- Install/recycle is an ordered document handoff, not a kill-and-relaunch broadcast. The executable path is replaced by same-directory atomic rename so controller and supervisor exec calls always see a complete old or new binary. Background auto-install waits for a clean committed source tree and disables the nested `lib-install` recycle broadcast. A direct successful `lib-install` enumerates every live `agent-doc start` process, resolves a relative document argument against that supervisor's process CWD, marks every open supervisor first, then marks controllers, exactly once. Controller fan-out is gated on use (`#installworktreecontrollers`): a root's controller is recycled (a handoff that launches a replacement) only when a listening PID-scoped editor socket or an open supervisor document proves the root is in use, and the `reload_library` fan-out may launch a missing controller only for such a root or for the root its caller seeded; an idle root is queried through an already-running controller only and skipped when none answers. A controller that self-detects a replaced binary retires instead of handing off when it owns no documents, no editor registration, allocated model, listening editor socket, or open supervisor uses its root, no RPC is in flight, and no client connected for 60s; the next client launches the installed binary lazily. A supervisor with a live child adopts the existing PTY across `execve`; a supervisor without a live child starts one fresh document-bound replacement and auto-triggers its own file.
- Supervisor-owned document paths may be relative to the supervisor CWD, but every controller request resolves them through an absolute, non-empty project root. The shared project-root walker anchors relative paths before testing `.agent-doc` ancestors; controller launch admission rejects an empty root as a fail-closed backstop. A nonexistent tmux-reported pane CWD is stale evidence and remains on the caller's explicit fallback path instead of being anchored into the supervisor process's unrelated root.
- Route-owned lifecycle is explicit at creation. Editor-origin Run Agent Doc and selected-document layout provisioning use `keep-alive`, so an idle prompt after commit or install remains the selected document's interactive owner. Controller/watchdog recovery uses `auto` and may reap a proven idle one-shot generation. Focus reconciliation must never recreate a keep-alive pane merely because a stale actor projection temporarily made it unfocusable.
- `keep-alive` bounds a *commit edge*, not a lifetime (`#stashpaneunbounded`). It means "do not reap merely because a cycle committed"; read as "never exit, ever" it leaks one permanent pane per document ever selected in the editor, because the reap decision discards the liveness reason before reading it and answers `explicit_keep_alive` unconditionally. A `layout-provision` owner exists to hold a pane for an editor column and never dispatches — queue control fences `Dispatch` and admits `LayoutProvision` for exactly that reason — so once its pane is **stashed** it is holding no visible column and is an orphan. Such an owner ends itself, publishing `layout_provision_stashed_orphan`. Three constraints make that safe. The proof is the supervisor's own start purpose and its own pane id, never a working directory or command line, and never a pane another session owns; an unresolvable pane or window is not stashed, so the reaper acts only on a pane it can positively prove. A live-pane-busy reason and an open cycle each short-circuit first, so an active harness turn is never reaped. Once direct child-output observation proves that a layout-provision owner admitted a real harness interaction, its effective purpose is promoted to `Dispatch` for the rest of that supervisor generation; returning to an idle prompt can never reclassify that used pane as an unused layout orphan. And a non-empty backlog is NOT liveness here — it is the steady state of every task document, so counting it would restore the unbounded behaviour; a queued prompt, an unanswered exchange tail, a post-commit follow-up, uncommitted drift, or an adapter failure all still keep the owner alive. The check runs independently of the commit-edge machinery and on its own throttle, because a pane provisioned and then never used never moves its document's cycle and so never reaches a commit edge at all — that is the shape that actually accumulates. Re-selecting the document simply provisions a new owner through the ordinary missing-actor path.
- A superseded generation is not the owner, so no reap policy keeps it (`#routeownedsupersededreap`, GH #133). A route-owned supervisor used to learn it was superseded only when one of its own actor transitions was rejected, which an idle stashed pane never attempts, so it lived under `explicit_keep_alive` for the life of the tmux server. Every route-owned supervisor, whatever its policy or start purpose, now re-reads the durable actor record for its document on the orphan-check throttle (with the same startup grace). When that record names a *different, live* pane, the document cycle is not open, and direct child-output observation shows no active turn in its own pane, it publishes `superseded_binding_orphan`, stops its child, and kills its own pane. A missing or unreadable record, a dead superseding pane, or a supervisor without its own tmux pane is unproven and never reaps; a record naming its own pane is the live owner and is untouched.
- A deliberate `SIGTERM` is an operator decision, not a crash (`#gh133sigterm`, GH #133 follow-up). The dead-supervisor watchdog sees only that a recorded `supervisor_pid` is gone; with the default disposition a `kill <pid>` and a panic were indistinguishable, so a supervisor killed in a *visible* column was resurrected on the next tick. Every supervisor therefore installs a `SIGTERM` handler at registration. On the signal a dedicated thread records a durable `supervisor_intentional_exit` row in `state.db` (`document_runtime_state`, keyed by the hash of the actor's canonical document id) carrying the exiting pid, generation, pane and session, then re-raises `SIGTERM` under the default disposition — the process still dies with status 143 and the PTY child is torn down exactly as before. The record has a 500 ms budget that fits inside the 750 ms SIGTERM→SIGKILL grace of the verified force-kill; a write that cannot finish is abandoned and the exit reads as a crash (the historical behaviour, never a hung process). A requested graceful self-kill (`#supkill-a`, `admin kill-supervisor`) records the same marker before it exits. The watchdog honours a marker only when it names exactly the dead pid **and** generation, logging `controller_supervisor_watchdog_skip reason=intentional_exit` once per document+pid; a missing or malformed row is a crash and keeps `#supresilience` restart behaviour, and a marker naming any other pid or generation is stale and never suppresses a later crash. The controller-initiated replacement (`session restart-supervisor`, `admin recycle --force`, the watchdog's own restart) SIGTERMs the old supervisor as controller intent to *continue* the document, so after the kill and before `cold_start_supervisor_replacement` it rewrites the marker its kill produced as a time-bounded `controller_replacement_cold_start` marker (pid from the kill outcome, else the handler's marker; logged as `controller_supervisor_replacement_cold_start_marker`). For `REPLACEMENT_COLD_START_GRACE_MS` (180 s) the watchdog skips that pid with `reason=replacement_cold_start_pending`. The cold start is typed into the pane's shell and needs seconds to register a lease, so the lease still names the dead pid during that window. Clearing the marker outright (the pre-`#runfrontendcrashed` behaviour) let the watchdog treat the replacement's own kill as a crash one tick later, and its second replacement force-killed the booting successor and left a bare shell. Once the grace elapses with no successor lease, the decision becomes `replacement_cold_start_expired` and the document is recovered as a crash. A cold start that fails synchronously clears the marker so the recovery happens immediately. A newly registered supervisor likewise clears any earlier generation's marker.
- A controller replacement's own kill is never an operator stop, even when its controller dies before the cold start (`#replacementintent`, 2026-10-20 `agent-doc-bugs.md`). A forced replacement (receipt 2172, stale supervisor after `make install`) escalated, wrote the `#supkill-a` self-kill sentinel, and then its controller self-recycled (`live_plugin_update`). The replacement worker was a thread in that process, so it died before `controller_supervisor_replacement_kill_outcome` and before the cold start. The supervisor still honoured the sentinel at its next boundary, killed its Claude child, and recorded `supervisor_intentional_exit signal=self_kill_request`, so the watchdog read the death as an operator stop and the pane stayed at a bare shell until the operator relaunched it by hand. Three rules now hold. (1) The replacement worker records the time-bounded `controller_replacement_cold_start` marker for the supervisor it is about to stop **before** it requests the kill (`controller_supervisor_replacement_cold_start_marker stage=pre_kill`), and refreshes it after the kill (`stage=post_kill`). (2) A supervisor recording its own deliberate exit (self-kill, duplicate retirement, or `SIGTERM`) never overwrites a fresh replacement marker that names exactly its pid and generation (`marker=preserved_replacement_intent`); an expired marker, or a marker for any other pid or generation, is overwritten as before, so an operator `SIGTERM` after the grace still stops it for good. The decision is the pure `intentional_exit_record_decision`. (3) If no successor registers within `REPLACEMENT_COLD_START_GRACE_MS`, the watchdog treats the death as a crash (`replacement_cold_start_expired`) and restarts it within its budget, or halts with `controller_supervisor_watchdog_halted`. A replacement whose requester vanished is therefore recovered, never silently dropped.
- A stale-binary replacement withdraws its kill when the supervisor refreshes itself in place (`#replacementwithdraw`). In the same incident, the 20 s in-place wait expired at 15:24:36, three seconds before the supervisor's own install re-exec (`execve_preserve_child`, same pid, Claude child adopted). The escalation's self-kill request was then honoured by the now-fresh supervisor, which killed the live child, including its background tasks. The kill driver (`drive_supervisor_kill_unless`) now polls the replacement's goal while the graceful request is pending. Its per-poll decision is the pure `supervisor_kill_wait_step`, where exit beats a satisfied goal and a satisfied goal beats the grace deadline. The goal predicate is `replacement_goal_satisfied_by_refresh`. A `continue` replacement started against a stale host is satisfied once the host is no longer stale. An operator restart of a fresh supervisor, Restart Agent (`agent:<mode>`), and `fresh` mode are never satisfied by freshness. A satisfied goal clears the sentinel and keeps the live supervisor and its child: `SupervisorKillOutcome::Withdrawn`, then `controller_supervisor_replacement_background_completed stage=refreshed_in_place`. The pre-kill replacement marker is left in place. It names the live pid, is read only once that pid is dead, and expires with the cold-start grace, so a supervisor that had already committed to the self-kill is still recovered.
- The dead-pane actor GC does not pre-empt the crash watchdog (`#gcdefercrash`). `sync`, `start` and gc close an actor whose pane no longer owns a live agent (`#adsessreap1`), and the watchdog skips `Closed` actors. In the incident above, `sync` closed the actor 6 s after the supervisor died (`sync_closed_stale_actor cause=dead_pane`, on an editor layout event), before the watchdog's 10 s tick, so the watchdog never saw the crash at all. Before closing a dead-pane actor, the GC now applies the watchdog's own gates through the pure `dead_pane_actor_gc_decision`. These are: the recorded supervisor pid is dead; its exit is not `Intentional`; the pane is alive and not stashed (`watchdog_binding_decision` is `Live`); the supervisor session log is still open; and the document's queue is not paused. When all of them hold, the GC keeps the actor and logs `<caller>_close_stale_actor_deferred ... reason=crash_watchdog_owns_recovery`. Any other binding closes as before: a normal harness exit (session closed), an operator stop, a stashed, unresolved or gone pane, a paused queue, or a missing lease. Every gate fails toward `Close`. The GC never kills or claims anything; it only stops hiding a binding that the watchdog is about to recover.
- A supervisor exiting on `SIGTERM` restores the operator's terminal before it re-raises the signal (`#runfrontendcrashed`). `RawMode` puts the pane's tty into `cfmakeraw` (no `OPOST`/`ONLCR`, no `ICANON`/`ECHO`), and its `Drop` never runs under the default disposition, so the shell it returned to stair-stepped every line. The handler applies the recorded cooked restore target and writes `OUTER_TERMINAL_RESET_SEQUENCE` (leave the alternate screen, disable bracketed paste and mouse/focus reporting, show the cursor). The restore target is always a cooked mode, never the inherited one, so a supervisor started on a pane that a SIGKILLed predecessor left raw does not "restore" raw. `agent-doc start` also normalizes an inherited raw terminal before its first status line, except on the `#ctlrecycle` re-exec adopt path.
- Idle **owned** supervisors whose pane is merely stashed are NOT reaped (decision restated for `#gh133sigterm`). Only an unused `layout-provision` owner (`layout_provision_stashed_orphan`) and a superseded generation (`superseded_binding_orphan`) end themselves. A `Dispatch` owner — or a layout owner promoted by a real harness interaction — holds the document's live harness conversation and its actor ownership; stashing is a reversible tmux layout effect, and re-selecting the document brings that same pane back. Reaping it would trade a few MB of idle process for a cold resume of the conversation on every tab switch. The respawn half of GH #133 is closed instead: a killed supervisor in a stash window is not resurrected (`#watchdogbindinglive`), and a deliberately killed one in a visible column is not resurrected either (above), so an operator who wants an owned stashed supervisor gone can now simply `SIGTERM` it.
- `⚠ STALE SUPERVISOR` describes binary freshness, not layout membership and not an orphan
  proof. A stale supervisor still owns its document and normally re-execs in place at a safe
  boundary; killing it from pane accounting would sever that ownership. Layout cardinality is
  derived only from editor-published columns. When those columns are unknown, pane membership is
  unchanged; when they are known, exact-visible reconciliation stashes non-member panes, and the
  purpose-aware rule above reaps only a proven stashed orphan.
- The stale-supervisor marker is keyed by tmux pane id alone, and pane ids are reused across tmux
  server restarts, so it is evidence only while the supervisor that wrote it still runs
  (`#stalecontrollerhotupdate`, `stale_supervisor_marker_holder_is_live`). A row whose writer pid is
  gone, or names a process that started after the row was written (pid reuse), reads as fresh and
  is pruned by the display probe: observed 2026-10-15, a current supervisor's pane `%180` was
  titled `⚠ STALE SUPERVISOR` from a 2026-09-09 row whose writer (pid 261721) had been gone for
  five weeks.
- Not a replacement for `ipc_socket.rs` — that socket handles write-path IPC between the binary and editor plugins. Supervisor IPC is a different socket, scoped to claude lifecycle control.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ tmux pane                                                    │
│                                                              │
│  ┌────────────────────────────────────────────────────┐     │
│  │ agent-doc start (supervisor process)               │     │
│  │                                                    │     │
│  │   ┌──────────────────────────────────────────┐    │     │
│  │   │ pty master  ←→  claude (child)           │    │     │
│  │   │               (pty slave = claude's tty) │    │     │
│  │   └──────────────────────────────────────────┘    │     │
│  │                                                    │     │
│  │   stdin  ─(copy)──→ pty master                     │     │
│  │   pty master ─(copy)──→ stdout                     │     │
│  │                                                    │     │
│  │   Unix socket: .agent-doc/supervisor/<session>.sock│     │
│  │     ├── POST /restart                              │     │
│  │     ├── POST /inject <bytes>                       │     │
│  │     ├── GET  /state                                │     │
│  │     └── GET  /pid                                  │     │
│  └────────────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────┘
```

The supervisor is a single process that:
1. Allocates a pty pair (master/slave) via `portable-pty` or `nix::pty`.
2. Forks claude with the slave as its tty, with a deterministic CWD and env.
3. Runs three threads: stdin→pty, pty→stdout, IPC socket accept loop.
4. Wait-loop on the child; on exit, applies restart policy.
5. Reports actor lifecycle facts to the project controller so the session actor
   owns authoritative state transitions.
6. Re-resolves the document frontmatter at quiet dispatch-ready prompt
   boundaries. When `agent:` now maps to a different harness and
   `agent_doc_agent_change_restart` is enabled, the supervisor logs
   `harness_change_detected`, gates with `agent_restart_boundary_gate`, requests
   a fresh restart, and records `agent_restart_performed` before spawning the
   new harness.

**One spawn per switch, consecutively (`#hswspawncount`):** the 1:1 invariant
holds across a SEQUENCE of switches on one document, not only for an isolated
switch. Each completed restart must leave the state the next switch starts from:
the running launch harness becomes the new harness, and the persisted actor record
is written back NORMALIZED (`claude-code`, never the raw launch name `claude`), so
route's normalized `expected_harness` compares equal and a later dispatch is
accepted instead of deferring to a restart that already ran. Once launch matches
frontmatter the switch is resolved, so further idle-watch ticks must neither
re-detect nor respawn — a respawn storm shows up only BETWEEN switches, which is
why the deterministic coverage drives a consecutive codex → claude → codex
sequence and asserts spawn counts over the whole sequence rather than per switch
in isolation (`route_sim_consecutive_harness_switches_spawn_exactly_once_each_with_no_storm`).
A sequence fixture must carry state forward between legs rather than re-seed it;
re-seeding the launch harness and the persisted record turns the sequence into
three independent first-switches and cannot observe this class of defect.

**Live verification (`#hswdisposabledoc`):** `agent-doc verify-harness-switch <FILE>
[--expect-switches <n>]` reads the same receipt trail back out of
`.agent-doc/logs/ops.log`, scoped to that document. It reports four outcomes
distinctly rather than collapsing them into pass/fail: no `harness_change_detected`
means the switch was never observed (and names the unsaved-frontmatter step that
produces it); detections with no `agent_restart_triggered` mean the switch is HELD
pending and the message carries the observed `gate=` verdict (`WaitForBoundary` — no
quiet boundary yet; `None` — the knob is off); a trigger with no
`agent_restart_performed ... action=spawn_fresh_harness` means the restart never
completed; and a spawn whose `old_harness` equals its `new_harness` is a respawn of
what was already running, not a switch. `--expect-switches` is what makes the
command able to fail a respawn storm — without it the check is presence-only, which a
storm satisfies — so the operator procedure always passes it. Counting lives in the
binary for this reason: a shell `grep -q` cannot express "exactly one per switch" and
inverts on a match.

`scripts/harness-switch-live-check.sh` scaffolds a disposable document under
`.agent-doc/live-repro/`, prints the one-screen procedure (derived from the
scaffolded `agent:` value, so the printed procedure cannot contradict the document on
disk), runs the verifier, and removes the document. It is a scaffold-and-print shell
only. A throwaway document exists because the check requires an UNSAVED frontmatter
edit in front of a live old-harness actor, which should never be done to a real
session document. Prefer the deterministic scenario above; use the live check only
when the case genuinely needs a live editor and pane.

JetBrains "Restart Agent" menu invocations must also leave an
optverify-scannable `.agent-doc/logs/ops.log` marker before dispatching the
restart command:

```text
restart_agent_menu_invoked file=<relative-path> source=jetbrains action=restart_agent
```

That marker proves the menu path was actually invoked; the later
`agent_restart_performed` marker proves the supervisor boundary restart spawned
the fresh harness.

"Restart Agent" travels the wire as `agent:<mode>` (`agent:continue` /
`agent:fresh`), distinguishing an explicit operator harness replacement from a
plain controller recycle (`#agentrestartwire`). Every hop must preserve that
prefix verbatim:

- `session_actor_cmd::restart_agent` encodes it.
- Editor retry UI must preserve the same typed restart target. In particular, a
  failed JetBrains **Restart Agent** followed by **Interrupt and restart** sends
  `restart-agent --force`; it must never degrade to `restart-supervisor --force`,
  which preserves the old harness child and loses the requested frontmatter
  switch. Supervisor retries likewise remain supervisor retries.
- The controller (`parse_supervisor_replacement_request` →
  `SupervisorReplacementIntent`) accepts it, keeps `restart_agent` alongside the
  mode, and forwards `intent.wire_mode()` on the supervisor IPC `Restart`
  request. A controller that normalizes the prefix away silently downgrades the
  request to a recycle; a controller that rejects it fails the operator's action
  outright with `unsupported supervisor replacement mode `agent:continue``.
- `agent_doc_supervisor_io::ipc::decode_restart_intent` decodes it, and a
Restart Agent never takes the stale-binary in-place `execve` path — that path
preserves the very child the operator asked to replace. The decoded intent
remains attached to the pending restart until the old child exits. On a
same-harness continue restart, it authorizes the new iteration to validate and
adopt the current document's exact `resume.<active-harness>` binding. The
same boundary also adopts the re-resolved launch arguments and environment, so
same-harness model/config edits apply to the replacement child. A locally
verified missing transcript retires that exact lineage and performs one fresh,
auto-triggered launch instead of invoking the harness with a known-bad id. If an
unverifiable exact resume still exits nonzero before establishing its handoff,
the next launch degrades to fresh rather than repeating that id through crash
backoff. The
persisted `resume:` value is a harness-keyed map (`claude`, `codex`, `opencode`),
and restart/cold-recovery paths must never read another harness's entry. Legacy
scalar values are interpreted only for the document's active harness and migrate
to its map entry on the next managed write. Crash restarts and plain
controller recycles never refresh lineage from frontmatter. A Claude `/clear` or non-resume
startup moves `resume.claude` to the new conversation through the
`SessionStart` hook (`#clauderesumeafterclear`,
`specs/15-turn-lifecycle.md` § Claude conversation-id capture), so the next
Restart Agent or relaunch adopts it. A crash (or other non-operator continue)
respawn of a Claude child instead re-reads the hook's durable, provenance-bearing
capture (`document_runtime_state` kind `claude_session_start_capture`) and
adopts its id only when it names this supervisor, this supervisor still holds
the document's current-generation lease, and it names the exact exited child
by pid and kernel start time (`#claudecrashresumeafterclear`, same section).
- On the cold-start path a Restart Agent authorizes `RestartLiveHarness` even in
  continue mode. The foreign-pane guard is unchanged: only a pane running *this
  document's* harness is replaceable.

A restart iteration that re-resolves the harness records its outcome in
`.agent-doc/logs/ops.log` whether or not the harness changed
(`agent_restart_performed` / `agent_restart_respec_refreshed` /
`agent_restart_respec_failed` / `agent_restart_respec_skipped`), so "the restart
came back on the old agent" is answerable from the log instead of inferred.
`agent_restart_respec_refreshed resolved_from=frontmatter:<value>` names the
`agent:` the authority text itself declared (`frontmatter:<unset>` when it has
none), never the actor the resolver substituted for an absent key.

Restart Agent re-resolves the harness from the controller's document
authority, not from the editor buffer, so it cannot see an `agent:` edit the
authority has not merged yet (`#restartagentharness`). Observed 2026-10-27 on
`tasks/software/lazily.md`: the editor replica was fenced at 16:07:54Z
(`crdt_document_op_delta_ingested outcome=StructuralQuarantined`, then
`crdt_replica_update_quarantined recovery=lazy_canonical_projection`), the
operator's later `agent: claude` → `agent: grok` edit was recorded by op capture
but never reached the authority, and five Restart Agent invocations each logged
`resolved_harness=claude resolved_from=frontmatter:claude` and relaunched
Claude Code. The editor therefore sends the `agent:` it shows:

- `agent-doc session restart-agent <FILE> --expect-agent <AGENT>`. JetBrains
  reads the open buffer's frontmatter through the native
  `agent_doc_frontmatter_agent` export (the binary's own parser) and passes the
  value whenever the buffer names an agent; an older native library, an
  unparsable frontmatter, or no `agent:` key sends no flag. The action runs on
  the EDT, where native calls are forbidden, so it captures only the buffer
  text there and makes the native read and the command launch on a pooled
  thread (see `editors/jetbrains/SPEC.md` **Threads**).
- Before requesting any replacement, the CLI compares that value with the
  authority's declared `agent:` through the pure
  `agent_doc_supervisor::harness_authority::restart_agent_frontmatter_agreement`.
  Both sides are compared as declared names after `normalize_harness_name`
  (`claude`/`claude-code` and `grok`/`grok-build` agree), not as resolved
  binaries, so an unknown name never agrees with `claude` merely because both
  fall back to it at launch. An absent or `default` editor value is no evidence
  and admits the restart unchanged.
- On disagreement the restart is refused with both values named and nothing is
  replaced; the ops log records
  `restart_agent_refused reason=authority_lags_editor editor_agent=<x> authority_agent=<y|<unset>>`.
  Failing closed is deliberate: honouring the editor value over the authority
  would let the idle agent-change watch (which reads the same authority) switch
  the harness straight back, and the remedy for the stale authority is editor
  sync convergence, not a supervisor-side override.

The replacement child must not auto-trigger the document while that document has
a retained binary-owned editor delivery. The supervisor keeps resetting the
dispatch-readiness deadline until the pending projection settles, then admits
the normal generation-fenced auto-trigger. This prevents a post-restart
`agent-doc session-check` from racing the pre-restart write and diagnosing an
authority/disk divergence that reconciliation already owns.

Route/startup code must not cold-replace a healthy live authoritative actor
solely because the document's `agent:` frontmatter changed. A live wrong-harness
actor is deferred to the boundary restart path; only an unhealthy supervisor or
a closed actor record is replaceable through the stale-authority path.

## Core Invariants

### CWD determinism
- Supervisor resolves the target CWD **once at startup** from (priority order):
  1. `--cwd <path>` CLI flag
  2. Frontmatter `agent_doc_cwd: <path>` (new field)
  3. Project root resolved by walking up from the document until `.agent-doc/` is found
  4. Document's parent directory (fallback)
- The resolved CWD is set on the claude child process via `Command::current_dir`.
- The resolved CWD is **also** `cd`'d into at the pty level before claude spawns, so any shell-side state inside claude (e.g., `pwd` in bash tools) sees the same directory.
- On restart (`--continue` or fresh), the CWD is re-resolved — not cached — so moving the document mid-session is safe.

**Implementation invariants** (pinned by `supervisor/cwd.rs` unit tests):

- **Relative-path bases differ by source.** `--cwd` relative paths are resolved against the supervisor's invocation CWD (standard `Path::canonicalize` behavior). `agent_doc_cwd` relative paths are resolved against the **document's parent directory**, so a frontmatter value of `..` means "parent of the folder the document lives in," independent of wherever the user happened to invoke `agent-doc start` from. Absolute paths bypass the base in both cases.
- **Misconfigured paths hard-error, never silently fall through.** If `--cwd /bogus` or `agent_doc_cwd: nowhere` points at a nonexistent path or a non-directory, the resolver returns an error with a source-labelled context string (`--cwd flag: path does not exist: ...` / `agent_doc_cwd frontmatter: path is not a directory: ...`). Silent fall-through to a lower priority would mask typos and make cross-project CWD drift — the exact class of bug this spec exists to fix — harder to debug.
- **Document existence is a precondition.** The resolver canonicalizes the document path first; a missing document is a hard error with a clear message, not a fallback-to-cwd.
- **Self-contained module.** `supervisor::cwd::find_project_root` is duplicated rather than reused from `snapshot::find_project_root`. The supervisor process runs at a lifecycle layer below snapshot management and must not pull in that dependency graph; the walk is a six-line loop and is exercised by the `deeply_nested_document_still_finds_project_root` test.
- **Source tagging for logs/IPC.** `CwdSource` exposes stable `as_str()` tags (`cli_flag`, `frontmatter`, `project_root`, `document_parent`) — the log line `cwd_resolved source=<tag>` and the IPC `state` response use these verbatim. The `source_tag_strings_are_stable` test locks the tag strings so downstream tooling doesn't break on refactors.

### Env determinism
- Supervisor builds the child env from:
  1. Parent process env (whitelisted via `HOME`, `PATH`, `TERM`, `LANG`, `TMUX`, `USER`, …)
  2. Frontmatter `env:` map (already expanded by `agent_doc_config::env::expand_values`)
  3. `AGENT_DOC_SESSION=<uuid>` and `AGENT_DOC_DOCUMENT=<path>` (new, so claude can introspect its own session without parsing frontmatter)
- **No inheritance of `PWD` from the tmux pane** — supervisor sets `PWD` explicitly to match the resolved CWD, so shell-side `$PWD` matches the process-level CWD.

#### Harness program resolution (GH #380)
- The pty spawn clears the ambient env, so a bare harness name (`claude`, `codex`, `opencode`, `grok`) is looked up only on the **child** `PATH`. A route-owned supervisor inherits that `PATH` from the project controller, and some hosts start the controller with a system-default `PATH` that omits the harness's user-local install directory (the Coder workspace default leaves out `~/.local/bin`, where the Claude Code installer puts `claude`). Before GH #380 the start failed with `spawn_command failed for program: claude … No viable candidates found in PATH`.
- `supervisor_process::spawn_program::resolve_spawn_program` (defined in `agent-doc-harness` as `agent_doc_harness::spawn_program` and re-exported by `agent-doc-supervisor-process`, so the headless backends share it — see `specs/05-agent-backend.md` § Harness program resolution) runs inside `PtySession::spawn`, for every harness and every launch (initial, restart, harness switch), in this order:
  1. A program that names a path (contains `/`) is spawned as given.
  2. A bare name that is executable on the child `PATH` (relative entries joined to the child cwd, as `portable-pty` does) is spawned as given — byte-identical to the pre-fix spawn, same `argv[0]`.
  3. Otherwise the well-known install directories are searched in order: under the child `HOME` (or the supervisor's own `HOME` when the child env has none) `~/.local/bin`, `~/.claude/local`, `~/.opencode/bin`, `~/.grok/bin`, `~/.npm-global/bin`, `~/.bun/bin`, `~/.volta/bin`, `~/.cargo/bin`, `~/bin`; then `/usr/local/bin`, `/opt/homebrew/bin`, `/home/linuxbrew/.linuxbrew/bin`. The first executable hit is spawned by that absolute path (never its canonicalized symlink target, so a versioned install keeps its `claude` process name for `pane_current_command`), and its directory is **appended** to the child `PATH` so the harness's own subprocesses find the same install. Existing `PATH` entries keep precedence.
  4. If nothing is found, the spawn fails before `portable-pty` with an error naming the program, the child `PATH` entries searched, and the install directories searched, plus the fix: install the harness, or put its directory on the supervisor's `PATH` or the document `env:` `PATH`.
- The parent `PATH` is never read, so the deterministic-env invariant holds. A fallback spawn logs `harness_program_resolved harness=<name> source=install_dir_fallback program=<abs path> path_dir_appended=<dir>` to the supervisor session log. Non-Unix platforms pass the program through to `portable-pty` unchanged.
- Pinned by `spawn_program` unit tests and the `pty` regression `bare_harness_name_missing_from_coder_default_path_spawns_from_user_local_bin` (Coder default `PATH`, harness only under `~/.local/bin`, child `PATH` gains that directory last).

### Pty lifecycle
- Pty is allocated before claude spawns and destroyed after claude exits + IPC socket closes.
- SIGWINCH on the tmux pane → forwarded to the pty master so claude sees resize.
  The resize callback resizes the child PTY first and the detection screen
  model second (`#paneoomrunaway`): the child's size never waits on the model's
  lock, so a slow or wedged model cannot leave the harness drawing at a stale
  width.
- `pty.rs` owns allocation and exposes the master reader/writer handles; it does
  not start an alternate stdin reader. On Unix, `io_threads.rs` owns the sole
  stdin→pty path and polls both raw stdin and a stop pipe, allowing child exit
  to join the forwarder before the supervisor reads a restart/quit prompt
  (`#af88`).
- Filtered child output is also fed into an `alacritty_terminal` screen model
  owned by the supervisor. Readiness, help-screen, and protected-prompt checks
  prefer that current viewport text and only fall back to the byte ring when the
  screen is empty, so cursor rewrites and line clears are interpreted as terminal
  state instead of append-only scrollback.
- The screen model is bounded (`#paneoomrunaway`): `OwnedPtyScreen` clamps its
  size to `MIN_SCREEN_COLS` (2) ..= `MAX_SCREEN_COLS` (1024) columns and
  1 ..= `MAX_SCREEN_ROWS` (512) rows, whatever `TIOCGWINSZ` reports. The child
  PTY keeps the real size. tmux can give a pane a 1-column width (a 3-column
  layout that leaves the last pane at `x = window_width - 1`), and
  `alacritty_terminal` 0.26 cannot hold a wide (2-cell) glyph in a 1-column
  grid: `Term::resize` down to 1 column reflows forever in
  `Grid::shrink_columns` (it pushes a new row per pass and never reaches its
  scrollback truncation), and `Term::input` of a wide char at 1 column panics
  with an index out of bounds. On 2026-10-27 the reflow loop grew six
  `sdk-demo.md` supervisors at about 1.5 GB/s, while holding the screen lock,
  until `systemd-oomd` killed each pane at 135-142 GB.
- The supervisor carries a resident-memory ceiling (`#paneoomrunaway`,
  `memory_guard.rs`). A `memory-guard` thread samples `/proc/self/statm` every
  500 ms, logs `supervisor_memory_growth rss_mb= threshold_mb=` at each doubling
  from 512 MiB, and at the ceiling (default 4 GiB, override
  `AGENT_DOC_SUPERVISOR_RSS_LIMIT_MB`, `0` disables) logs
  `supervisor_memory_ceiling_exceeded` to the session log, the document's
  ops.log and stderr, then aborts the supervisor. Start logs
  `supervisor_memory_guard status=armed limit_mb= rss_mb=`. A named abort at a
  few gigabytes replaces an anonymous oomd kill of the whole pane.
- That viewport keeps the composer ghost style (`#clearqueuedliveturn`):
  `OwnedPtyScreen::visible_text_with_ghost_style` re-emits faint (SGR `2`) and
  italic (SGR `3`) runs, and only those, so `child_output_for_detection` carries
  the same ghost-text evidence a `tmux capture-pane -e` snapshot does and the
  shared composer projection (`prompt_candidate_is_dim_placeholder`) tells a
  harness placeholder from a typed draft. A plain viewport dropped it: after a
  turn Codex 0.163 renders the faint `› Ask a follow-up question` placeholder,
  which is on no static list, so the supervisor read it as an operator draft
  and the idle watch held `prompt_visible=false` for the rest of the
  conversation. A clear queued behind that turn never fired (haiven-dev
  `tasks/fpe.md` pane %12, 2026-10-20: queued 16:16:27Z, turn closed by
  16:24:49Z, cleared by hand at 16:29:56Z) and the same blind spot held the
  idle drain. The byte-ring fallback always kept the SGR.
- On supervisor exit (user `q`), pty slave closes, claude gets SIGHUP.
- OpenCode permission prompts are a guarded stdin exception: when recent child
  output parses as the horizontal `Allow once` / `Allow always` / `Reject`
  permission dialog, the stdin forwarder rewrites legacy arrow-key escapes to
  Tab/BackTab selector keys before forwarding them to the child. Outside that
  active permission prompt, OpenCode receives the original arrow bytes so normal
  composer navigation is unchanged.
- Supervisor input diagnostics emit structured `tmux_input_event` lines at the
  input boundaries: operator stdin forwarding, IPC/auto-trigger injection, tmux
  pane submits, and active permission-prompt key translations. Each line
  includes source, destination, transform, key, byte count, and harness when
  known; prompt text is logged only as length plus SHA-256. Child-output
  Kitty keyboard-mode preserve/drop traces are verbose-only
  (`AGENT_DOC_TMUX_INPUT_DIAG` / `AGENT_DOC_DEBUG_STDIN`) so normal supervisor
  filtering cannot print diagnostics into the managed TUI while the operator is
  typing.
- Every `agent-doc start` opens its supervisor stderr log; every start whose
  harness is a TUI (Claude/Codex/OpenCode/Grok) additionally redirects fd2 to
  that opened file in-process, route-owned **or started by hand**
  (`#perfpanebleed`). Route ownership is not an input: a hand-started
  supervisor runs the same in-process idle-queue watch, steering poll,
  closeout, and commit paths, and any stderr diagnostic they emit while fd2 is
  the pane TTY is painted over the composer, separator, and status line. A
  painted `❯ [perf] ...` composer row persists in the tmux grid (the harness
  never repaints an unchanged empty composer), projects as an operator draft,
  and the idle-queue draft guard then refuses every dispatch — the session
  wedges at the end of its turn. Only non-TUI harnesses keep inherited stderr.
  An `execve` hot-reload that adopts a surviving child moves fd2 to the log at
  its very first instruction (`start_for_surviving_child_reexec`), before start
  admission, so a predecessor image that handed over the pane TTY as fd2 cannot
  leak the replacement's reentry/status output; a redirected predecessor's fd2
  is never restored before `execve`, so the replacement inherits the log.
  Leaked lines also explain the "tabs inserted" look: the harness puts the pty
  in raw mode (`-opost`, no `ONLCR`), so a bare `\n` moves down one row without
  returning to column 0 and each leaked line starts where the previous one
  ended (a staircase). Recovery from an already-painted pane is a full TUI
  repaint (resize/zoom-toggle the pane so the harness gets `SIGWINCH`, or the
  harness's own redraw key); `tmux refresh-client` alone only replays tmux's
  grid, which still holds the foreign bytes. The live regressions are
  `pane_bleed_tests` in `agent-doc-supervisor-process-io` (`make tmux-ci`).
  Dropping the redirect guard restores the original fd2 so a start that fails
  or exits reports its error to the operator. Pure (non-`-io`) crates emit no
  stderr diagnostics at all; latency belongs in the caller's ops-log phases.
  The shell start command carries no `2>>` redirect.
  The start binary resolves `agent_doc_supervisor_stderr_log` from
  `.agent-doc/config.toml` (relative paths are project-root-relative), creates
  its parent directories, and defaults to
  `.agent-doc/logs/supervisor-stderr.log`. If the project-local target cannot be
  opened, it records the failure in a deterministic temporary fallback log.
  Routine idle-watch, stale-busy reconcile, and stale-binary hot-reload
  diagnostics must stay in the session log / ops log instead of painting over
  the foreground TUI after `/clear`, restart, or inter-queue-item recycle.
- A preserved child's PTY is never unserviced across an `execve` hot-reload
  (`#reexecptybridge`, GH #270). The replacement image cannot adopt the
  inherited master until start admission finishes, and admission (CLI-entry
  stale-supervisor probe, config, registry publish, session re-entry) can wait
  on `.agent-doc/state.db` in 30 s `busy_timeout` steps; a batch recycle of 21
  supervisors queued on that lock froze every pane for 31-90 s, including a
  pane streaming a mid-turn response. So the replacement starts a bounded
  bridge (`agent-doc-supervisor-process::reexec_bridge`) as the first step of
  `try_main`, before any of those steps: a self-contained pump over raw fds
  that forwards filtered child output to the pane and pane keystrokes to the
  child, and shares no lock with admission. Only the replacement image itself
  may bridge: the outgoing image exports its PID
  (`AGENT_DOC_REEXEC_SUPERVISOR_PID`, preserved by `execve`), so a subprocess
  that inherited the handoff environment and the non-`CLOEXEC` master fd never
  bridges; an older outgoing image without it is admitted only for the
  replayed `start` command, and `start` itself starts the bridge if the CLI
  entry did not. The outgoing image also exports its harness
  (`AGENT_DOC_REEXEC_HARNESS`) so the bridge applies the reader thread's
  terminal-filter policy; both variables are stripped from the child launch
  environment. The bridge is finished at the spawn site of the first
  generation, before that generation's reader/writer threads exist, on every
  path (adopt or fresh spawn), so two pumps never compete for the pane's
  bytes; the session log records `supervisor_reexec_pty_bridge_handoff` with
  the serviced time and byte counts. Its handoff is lossless: it never closes
  the master; bytes it read but could not write before stop (a stalled pane, a
  child not reading input) are returned and written by the adopter first; an
  escape sequence split across reads is completed within a 250 ms drain bound
  or its prefix is handed over, so the adopter's fresh filter never drops half
  a control sequence; and the bridged output tail (64 KiB) is replayed into the
  adopter's terminal-screen model so readiness and prompt detection see the
  screen the pane shows. Stop interrupts any write after at most one 1 KiB
  chunk, so a stalled pane cannot wedge adoption. Input is not read once the
  child's output has closed, leaving stdin to the restart path. `Ctrl+D` is
  dropped while bridging, the same `#stale-ctrl-d-arm` contract as a byte
  buffered across `execve`. The live regressions are `reexec_bridge::tests`
  (real socketpair/pipe fds) plus source-order guards in `main.rs` and
  `run.rs`.
- An unchanged routine stale-binary recycle deferral is quiescent state, not a
  poll effect. The supervisor keeps the cheap 500 ms installed-binary identity
  check, emits one `supervisor_binary_stale_recycle_deferred` receipt on entry,
  and bounds full turn-boundary reconciliation to the quiescent maintenance
  cadence until the deferral clears. This preserves boundary safety without
  consuming sustained CPU or growing logs while an idle session waits.
- An idle, ready supervisor reads nothing on a quiescent tick
  (`#fixthrashingbackground`). Every urgent-maintenance reason that bypasses the
  quiescent gates must be a reason something can change:
  - The `#gh179` orphaned-empty-preflight reclaim cadence (every 30 ticks) is
    urgent only while a reclaim is possible: an open durable cycle, an in-flight
    supervisor IPC handler, or an unreadable cycle state (unknown is not
    closed). One closeout read per cadence decides it. Unconditionally it ran the
    whole idle pipeline (queue head, steering wake, frontmatter refresh) on every
    supervisor every ~15–17s.
  - Turn attribution is resolved lazily on a quiescent tick and primed only
    after the quiescent gates, immediately before domain dispatch
    (`#0.35.413` ordering is preserved for dispatch).
  - The steering wake (`#steeringwake`) carries settle evidence between its own
    observations in process (`midturn_steering::WakeSettleCarry`): it never
    advances the durable hook watermark, and with an editor attached the settle
    gate has no edit timestamp, so without the carry a held candidate was "first
    seen" on every observation and re-armed its recheck — a full-text read —
    forever. With it a held candidate settles (or reaches max hold) and the
    rechecks stop.
  - A queue-head probe whose compact revision answers `Detached` reads disk
    directly instead of asking the controller for the full text it would only
    answer `Detached` to.
  `scripts/idle-supervisor-bench.sh` measures supervisor/controller CPU and
  ops-log reads per idle minute against a scratch project (isolated tmux, fake
  harness); a quiescent window writes no ops-log lines.
- Cycle-state reads ask the controller for the closeout projection only
  (`document_closeout_projection`; in-process, the closeout reader of
  `agent_doc_state_wire`), never the whole document projection, whose history
  (a full document image per visible-write commit candidate, every terminal
  closeout proof) is unbounded per document. A controller that predates the
  command answers `unknown controller command` and the client falls back to the
  whole projection. Request-admission turn-attribution priming in the
  controller uses the same closeout reader.
- The process-wide live projection and closeout readers are bound to the
  project root of the controller runtime that installed them
  (`#ctrltimingflakes`). A read inside the controller process for a document of
  root R uses them only when they were installed for R; a reader installed for
  another root never answers, and the read falls back to the durable ledger.
  The process root and the readers are written separately during `serve`, so
  with two controllers in one process (parallel in-process controller tests)
  an unscoped reader let one controller's "no state" answer a read for the
  other's root.
- The controller's orphan-drain sweep (`#orphandrain`, every 10s) probes
  supervisor liveness (a full `/proc` scan) only when the cheap facts leave a
  dispatch possible (`orphan_drain_may_dispatch`: active queue and a drainable
  head).
- The orphan drain uses the same claim-aware supervisor-scope selector as the
  supervisor idle watch: it skips a head held by a live queue claim, advances to
  the first unclaimed drainable head when one exists, and emits a transition
  diagnostic with `reason=queue_head_claimed`. Claim state is independent of the
  document revision, so a memoized document head is rechecked against the claim
  ledger before every dispatch attempt; a claim read failure fails closed.
  Claims are matched by the head's prompt text (`[#beyp]: approved`,
  `do [#beyp]`), never by the bare tracked id the drain dispatches and logs
  (`head=beyp`): a claim keys on the line's work identity, and a bare id reads
  as unrelated free text. The revision memo therefore caches the prompt text
  beside the routing name, so the fresh and memoized paths make the same
  comparison (GH #319; missing it re-dispatched a subagent-claimed id-backed
  head every drain interval, once into a busy turn).
- If an orphan-drain route finds that the authoritative supervisor recovered
  after the sweep, its first attempt fails closed and yields one bounded
  backoff interval to the recovered idle watch. That preserves the idle
  watch's ownership of queue auto-clear. If the head is still orphaned at the
  next attempt, route dispatches through the authoritative actor as a bounded
  fallback. Neither outcome may return a successful no-op (GH #307). The
  recovered route remains background-only: it neither rescues a stashed pane
  nor changes tmux focus or layout.
- A supervisor mid re-`execve` is restarting, not orphaned
  (`#autoclearorphandrain`). Between the outgoing image's `execve` and the
  replacement's registration the supervisor is unreachable, so the actor reads
  exactly like a dead one, but the outgoing image publishes the recycle
  `InFlight` before the `execve` and the replacement settles it only when its
  idle watch starts. An orphan-drain route that finds the actor not
  dispatch-eligible therefore reads the recycle projection: `InFlight` is the
  restarting case, and it is treated like the recovered one above. The first
  attempt logs `route_background_existing_pane_guard outcome=deferred
  reason=supervisor_restarting action=await_idle_watch`, records a route
  deferral, records no dispatch receipt, and types nothing; the replacement's
  watch, which carries the queue auto-clear baseline across the re-exec
  (`#autoclearbaseline`), clears and then dispatches the next item. Without
  this the route took the degraded direct-pane path, waited out the recycle,
  and typed item two past the pending clear (`make tmux-ci` under load). A
  restart that never settles still drains: the next attempt for the same head
  logs `outcome=allowed reason=restarting_supervisor_missed_backoff
  action=continue_orphan_route` and continues, with the pre-input recycle gate
  deciding as before. No `InFlight` recycle (or an unreadable projection)
  keeps the genuine-orphan path unchanged.
- The orphan drain's claim check is a snapshot, so it is re-read at every
  irreversible step (GH #319 claim visibility). The sweep reads the document's claim
  ledger before its selector's claim reads, then re-reads it at the commit
  point (after the supervisor `/proc` scan and lease reads, before taking the
  backoff lease and logging `controller_orphan_drain_dispatch`), and the route
  worker re-reads it again just before running a queued route, which may have
  waited behind another document's route. If either re-read holds a claimed
  identity the snapshot did not, the step stops with
  `controller_orphan_drain_dispatch_skipped
  reason=queue_claim_landed_after_snapshot stage=sweep_commit|route_worker`
  and the next sweep re-decides from the fresh ledger. A released or expired
  claim is not a gain and never stops a dispatch; an unreadable ledger fails
  closed (`reason=queue_claim_read_failed`). Without the re-reads a claim that
  committed while the sweep was mid-decision was invisible to it: a preset
  head `#upgrade` claimed at 04:25:00 was dispatched at 04:25:02 (GH #319,
  "also seen"). The route's own pane delivery after the worker gate is not
  re-checked: route carries no head, so a claim landing inside `route` itself
  remains a residual window.
- Codex background-terminal banners are active-turn evidence. If recent pane
  output contains `Waiting for background terminal (... esc to interrupt)`, the
  supervisor idle-queue watch and route readiness checks must treat the pane as
  busy even when Codex also renders an input prompt; text typed into that prompt
  would be queued, not dispatched.
- Queue-dispatch progress follows the same foreground-safety contract. Dispatch
  attempts record redacted `queue_dispatch_progress` / `queue_dispatch_warning`
  events in `.agent-doc/logs/ops.log` with command byte counts and SHA-256, but
  they must not mirror raw command text or progress lines into the foreground
  OpenCode TUI unless verbose input diagnostics are explicitly enabled.

### Actor lifecycle reporting

- Startup calls the project controller before the child launches:
`start_session` records the actor generation in `starting`, and
`register_supervisor` records the supervisor pid/socket lease.
- Supervisor hot-reexec with a surviving inherited child is not startup. The
replacement must detect that handoff before start admission, require the
authoritative document/session/pane binding to match and remain non-closed,
retain its generation and runtime state, and refresh only the supervisor
pid/socket lease. It must not emit `session_start`, fire session-start hooks, or
publish an ownership transition. A missing or mismatched binding fails closed.
- The adopted child's run state propagates across the hot-reexec (GH #321).
The reentry still writes a `<harness>_start mode=exact_resume` session-log line
before it adopts the child, followed by `supervisor_reexec_adopted_child`
(`agent_doc_supervisor::startup_miss::REEXEC_ADOPTED_CHILD_EVENT`, shared by the
writer and the classifier). The session-log classifier
(`session_log_status_from_content`) reads that pair as transport reentry, not a
launch: the latest run, its anchor timestamp, and its committed-cycle
readiness proof stay those of the run that was open before the recycle, and
facts the reentry window itself observed are OR-ed in. An adopted run that was
never proven ready stays unproven. With no open prior run (none logged, or it
already exited), the reentry's own line stays the anchor and route fails
closed. Without this, every upgrade recycle reset the run to "still booting",
so a dispatch-only reopen demanded a ready-prompt capture that a stashed
1-row pane can never show, and the orphan drain was refused every ~90s forever
with an unsatisfiable `wait_for_dispatch_ready_prompt`.
- Prompt observation reports `ready` with reason `prompt_ready`. This happens on
the first prompt for a child and after later `busy` dispatches return to an
idle prompt.
- Supervisor-owned dispatch paths report `busy` before injection, using
  `ipc_inject` for routed IPC and `auto_trigger_inject` for restart-triggered
  reopen commands.
- A restart-triggered reopen is derived from restart provenance and pending
  work, never hard-coded on every IPC restart (`#idlecleartriggerinject`).
- Operator Clear Session Context (`Clear` IPC) TYPES the harness-native clear
  command into the live child whenever a responsive composer can take it
  (`#idleclearrestart`): a `ready` actor with a live child receives `/clear`
  (OpenCode `/new`) through the same guarded pane-input delivery as the
  queued/deferred clear, marks the actor `busy` (`ipc_clear`) until the prompt
  returns, and answers `restart_fresh=false delivery=native_clear`. The child
  process, and every Claude background agent and shell it owns, survives;
  restarting it killed them with no report (2026-10-11 live test: a background
  agent stopped at 12/40 ticks). The pure decision is
  `agent_doc_supervisor::lifecycle::operator_clear_delivery`. The supervised
  fresh restart remains only where typing cannot work, each answered with
  `restart_fresh=true reason=<…>`: no live child (`child_not_alive`), the actor
  is `waiting_input` so the supervisor's restart-or-quit keepalive or a harness
  permission prompt owns the input (`supervisor_waiting_input`), or the
  harness's native clear exits the process (Codex, `#clear-ready-restart`:
  `harness_clear_exits_process`). Such a restart crosses an open cycle only
  when the actor is `Ready` and the cycle's response is durably captured
  (`#clearcapturedcycle`, see the restart-admission rules above); otherwise it
  latches until the cycle closes and the answer says so
  (`deferred_until_cycle_closes=true`). Explicit Restart Agent and
  `session restart-supervisor` are separate commands and unchanged. A busy or
  starting actor keeps the pre-existing typed delivery; the client-side
  `operator_clear` guard has already queued a clear behind a live turn or
  refused protected input before IPC is contacted. A typed clear has no
  relaunch, so it never re-triggers the document; queue continuation after it
  stays with the idle-queue watch and its clear cooldown.
- A restart-required operator Clear encodes its provenance in the stored
  restart mode (`clear:fresh`; the legacy decoder strips the prefix). At the child-exit boundary the supervisor observes, once and
  without timers, whether an agent-doc cycle is still open and whether the
  closed-cycle `TurnAdmission` verdict reports unanswered operator intent
  (steering or an explicit route). It re-submits `/agent-doc <file>` only when one
  of those holds, or when the observation itself failed (fail open); otherwise
  the fresh child stays at its idle prompt and no `auto_trigger_inject` runs.
  An open cycle counts as pending work only while its response is NOT durably
  captured (`#clearsessioncontext`): a clear that crossed a captured
  cycle (`#clearcapturedcycle`) leaves the closeout to the supervisor's keyed
  captured-finalize resume, and re-running the document on the fresh child only
  made it restate the retained-closeout blocker (Codex Stop hook
  `codex_stop_retained_closeout_needs_operator action=report_operator_blocker`)
  the old context had already reported, or act on work the supervisor owns. The
  fresh child therefore waits at its prompt, exactly as the old child did over
  the same captured cycle, and the client's crossed-cycle Clear message says
  so. The capture observation reads the same fail-closed projection as the
  clear's restart admission, so an unreadable projection re-triggers as before.
  The pure decision is `agent_doc_supervisor::run_loop::requested_restart_trigger`.
  The decision is logged as `operator_clear_restart_trigger auto_trigger=<bool>
  reason=<open_cycle|captured_cycle_supervisor_closeout|operator_intent_pending|pending_work_unobservable|operator_clear_no_pending_work>`.
  Go-mode queue heads are not part of this decision: the per-child idle-queue
  watch is the single owner of queue continuation and drains a pending head on
  the new child's first idle boundary. Restart Agent, IPC `session restart`,
  harness switches, crash recovery, and recycle resume keep re-triggering.
- Clean exits that require operator input report `waiting_input`; flap halts
  report `blocked`; supervisor shutdown reports `closed`.
- Every lifecycle report carries the session id, pane id, and generation. The
  controller rejects stale reports instead of letting an old supervisor mutate a
  newer actor owner.

## IPC Socket

Per-session Unix-domain socket at `.agent-doc/supervisor/<session-uuid>.sock`.

Protocol: length-prefixed JSON (same frame format as `ipc_socket.rs`, so the existing client code in FFI can be reused).

### Methods

| Method | Request | Response | Notes |
|--------|---------|----------|-------|
| `restart` | `{ "mode": "fresh" \| "continue" }` | `{ "ok": true, "pid": <u32> }` | Kills current claude, relaunches |
| `inject` | `{ "bytes": "<base64>" }` | `{ "ok": true, "n": <usize> }` | Write bytes to pty master |
| `state` | — | `{ "running": bool, "pid": u32?, "restart_count": u32, "state": "...", "actor_state": "..."?, "current_harness": "..." }` | Includes supervisor health, the current actor lifecycle state, and the live child harness identity |
| `pid` | — | `{ "pid": u32?, "supervisor_instance_id": string?, "binary_stale": bool }` | Lightweight identity/freshness query; does not build the editor-authority `state` snapshot |
| `stop` | `{ "graceful": bool }` | `{ "ok": true }` | Shuts down supervisor + child |

Socket is created with mode `0600`. Opaque to anything except the FFI library, which exposes typed C ABI wrappers.

Every document-scoped CLI command issues one bounded `pid` query at command
entry. The query refreshes the supervisor's process-scoped Lazily binary
freshness observation and returns the derived `binary_stale` projection. A
`true` result schedules the existing idempotent safe-boundary recycle effect;
the command never kills a live turn. The query has a 250 ms ceiling and fails
open. During rolling upgrades, an older supervisor may omit `binary_stale`; the
CLI then compares the returned process inode with the installed binary inode,
without reading controller SQLite state.

The supervisor resolves that installed command independently of its
`current_exe`. If it started from another launchable path, such as the native
payload behind a PyPI bootstrap, it snapshots the installed command at launch
and compares later identity/inode observations to that snapshot. It does not
compare the unrelated payload and wrapper inodes, which would report permanent
staleness even before an upgrade.

### External control use cases

1. **`/agent-doc` routing from a different tmux pane:** instead of `tmux send-keys`, the route subcommand opens the supervisor socket and calls `inject` with `/agent-doc <file>\r`. Removes the 5s sleep hack + race conditions in `start.rs:229`.
2. **Editor plugin "restart claude" button:** IntelliJ plugin opens the socket, calls `restart`, displays the returned pid.
3. **Crash-state introspection for health dashboards:** `state` returns restart count plus supervisor/actor state so a cleanup hook can escalate (e.g., "5 restarts in 60s → stop and notify").

### Actor state reporting

The supervisor is responsible for keeping the authoritative session-actor store
in sync with the live child lifecycle without creating a new ownership
generation:

- child launch after the initial `session_start` moves the actor to `busy`
- idle prompt visibility moves it to `ready`
- clean-exit or resume-failure prompts move it to `waiting_input`
- halted restart policy moves it to `blocked`
- final supervisor exit moves it to `closed`
- IPC or auto-trigger dispatch writes also mark the actor `busy` before bytes
  are injected into the child pty
- the IPC `state.current_harness` projection changes immediately when an
  in-loop frontmatter `agent:` handoff spawns the replacement child
- route accepts a live harness mismatch as a boundary handoff and sends no
  prompt bytes to the old child; idle watch preserves an active turn, switches
  at the first safe idle boundary, and auto-triggers the document on the fresh
  child exactly once
- queue pause holds that accepted handoff until resume, while disabled
  `agent_change_restart` remains an explicit rejection rather than an implicit
  manual-restart recovery path

Those updates must fail closed when the supervising pane/session no longer owns
the authoritative generation.

Both `route` and `resync` also use the supervisor socket as part of registered-document ownership proof. The primary top-down check requires the controller's canonical document binding to agree with the live pane, recorded supervisor PID, and reported `supervisor_instance_id`. When tmux argv/path inspection no longer proves ownership, the socket's `Pid` method remains a secondary diagnostic that maps the live supervisor PID back to the tmux pane before treating the transactional binding as stale.

## Crash Recovery Policy

Replaces the current "sleep 2s + `--continue`" with a state machine:

```
state = Healthy
on claude exit with code c:
    append to restart history (ring buffer, last 10 exits with timestamps)
    classify:
        c == 0  → Clean
        c != 0 AND exits_in_last_60s < 3 → Transient
        c != 0 AND exits_in_last_60s >= 3 → Flapping
    action:
        Clean:    harness-specific clean-exit handling, transition Healthy
                  Claude: prompt user (Enter/q)
                  Codex: auto-restart in resume mode so `codex exec` stays attached
                         EXCEPT when a fresh/fresh-restart Codex child exits
                         cleanly before it ever surfaces an idle prompt; treat
                         that as failed startup provenance and restart fresh
                         instead of chaining `--continue`
                         EXCEPT when stdin EOF (Ctrl+D) detected → prompt user
                         (Enter to resume the exact document conversation / q to
                         exit) so the operator can intentionally restart or quit
                         without silently replacing the conversation
                         EXCEPT when stdin-forwarded Ctrl+C terminates the
                         child → prompt user with that same menu instead of
                         treating the exit like a transient crash
                         AND only stdin-forwarded Ctrl+C counts for that path;
                         route/plugin-injected interrupts that bypass the
                         stdin writer stay on the automatic recovery path
                         EXCEPT when a fresh/fresh-restart Codex child exits
                         before it ever surfaces an idle prompt and no
                         forwarded operator quit key was observed: treat that
                         clean exit as failed startup provenance and restart
                         fresh automatically instead of prompting
                         AND the supervisor must log the prompt outcome
                         (`user_quit*`, `user_restart_fresh`, invalid input) so
                         later `session_start` / `session_end` transitions keep
                         user-input provenance in the session log
                         AND those supervisor prompts must switch stdin into a
                         canonical local prompt mode instead of trusting the
                         inherited parent harness tty flags, so Enter keeps
                         working even when the outer binding session is raw-ish
                         AND stdin EOF at the restored Ctrl+D prompt counts as
                         quit, because that prompt now only appears after a
                         real visible child prompt and reflects an intentional
                         operator quit/restart choice again
                         AND stdin EOF at the remaining resume-failure prompt
                         counts as `restart fresh`, not as quit, so detached
                         stdin cannot close the pane during keepalive recovery
                         AND non-empty non-`q` input is rejected with a
                         re-prompt instead of silently restarting fresh
AND resume auto-trigger primarily accepts a prompt line that
appears in the current resumed child's filtered pty output;
when the child-owned terminal renderer misses a prompt that the
owned tmux pane can see, it may fall back only after the current
child has emitted output, the actor independently reconciles to
ready, and the pane remains dispatch-ready for two consecutive
polls; stale tmux history alone is never enough
AND when the replacement child never re-establishes a prompt
(`auto_trigger_timeout` / trigger delivery failure), treat the
handoff as failed provenance. The watcher fails closed after 60s,
records `startup_miss`, and never types into an unproven child;
a later explicit route may recover once the owned pane becomes ready
Transient: sleep 2s, restart fresh and document-bound, state Healthy
Flapping:  sleep 30s, restart fresh and document-bound, state Degraded
                   on 5th consecutive failure → state Halted
        Halted:   do not restart. Print "supervisor halted — run 'agent-doc
                  supervisor resume <session>' to retry"
```

State is surfaced via `state` IPC method so dashboards / cleanup hooks can observe it.

## `start.rs` integration

`agent-doc start` now always runs through the supervisor-owned start path.

- The production code keeps the orchestration loop in `start.rs` instead of
  collapsing everything into one `supervisor::run` wrapper.
- `supervisor/*` owns the bounded helper concerns: pty management, IPC, resize,
  environment/CWD resolution, and crash-policy primitives.
- Release behavior is already live for sessions spawned through the current
  binary; existing pre-supervisor panes are unaffected until they are restarted.

## Five Hard Parts — Answers

From the previous exchange:

1. **Pty vs. raw inheritance** — pty. Use `portable-pty` crate for cross-platform (we need at least Linux + macOS for dev + manylinux for PyPI builds). The pty allows stdin/stdout forwarding while giving us an injection point.

2. **CWD determinism** — resolved at supervisor startup from CLI flag > frontmatter > project root > doc parent. Set both on the child process and inside the pty via a `cd` command before handing control to claude.

3. **Crash recovery** — state machine with ring buffer (Healthy → Transient → Flapping → Halted). Flap detection via `exits_in_last_60s`.

4. **External control** — per-session Unix socket, length-prefixed JSON, four methods (restart, inject, state, stop). Reuses frame format from `ipc_socket.rs`.

5. **IPC lifecycle** — supervisor owns the socket for its lifetime. Socket file is cleaned up on normal exit and on `stop`. Stale sockets are detected by connecting during `register()` — if connect fails with ECONNREFUSED and the supervisor PID in controller state is dead, delete the stale socket.

## Resize Handling

**Unix (Linux/macOS):** `signal-hook` installs a SIGWINCH handler that pushes an event into a `crossbeam_channel`. A small resize thread blocks on the channel, queries `TIOCGWINSZ` on stdin fd, and calls `pty_master.resize(PtySize { rows, cols, .. })` from `portable-pty`, then resizes the detection screen model, which clamps the size to a bounded shape of at least 2 columns (`#paneoomrunaway`, see Pty lifecycle).

**Windows:** no SIGWINCH. Resize comes from the console input queue as `WINDOW_BUFFER_SIZE_EVENT`. Options, in order of preference:

1. **`portable-pty::native_pty_system()` already abstracts ConPTY** — on Windows, `PtyMaster::resize()` calls `ResizePseudoConsole`. We need to **feed** resize events into that call ourselves.
2. **Source of resize events on Windows:** `ReadConsoleInputW` on `stdin` handle returns a `WINDOW_BUFFER_SIZE_EVENT` record whenever the console window resizes. Spawn a thread that loops on `ReadConsoleInputW`, filters for that event type, and calls `pty_master.resize()`.
3. Since `agent-doc start` already requires tmux (`sessions::in_tmux()` at `start.rs:128`), and tmux on Windows means **WSL**, in practice the Windows path is just "Linux in WSL" — SIGWINCH works normally. A pure-Win32 build path is only relevant if we ever support non-tmux sessions on Windows, which this spec does not.

**Resolution:** `#[cfg(unix)]` uses SIGWINCH + `signal-hook`. `#[cfg(windows)]` uses `ReadConsoleInputW` in a dedicated thread. Both feed into the same `pty_master.resize()` sink. The `resize.rs` submodule has two `mod platform_{unix,windows}` implementations with a common `ResizeWatcher` trait.

**WSL caveat:** the realistic "Windows support" story for `agent-doc start` is WSL, because tmux itself is not native to Windows. The ConPTY code path exists for future-proofing non-tmux Windows sessions (e.g., a future `agent-doc start --no-tmux` running claude directly in Windows Terminal), not for phase 1 shipping.

## Non-tmux Mode (Future)

The supervisor is architecturally independent of tmux — it owns claude behind its own pty, so the outer terminal the user sees is conceptually separable from how claude is wrapped. Phase 1 still requires tmux (see Non-Goals), but a future `--no-tmux` mode is reachable by changing four things:

1. **Entry gate.** `start.rs:128` currently calls `sessions::in_tmux()` and refuses to run outside a tmux pane. Becomes a mode check (`--tmux` default, `--no-tmux` opt-in).
2. **Resize source.** Today SIGWINCH originates from the tmux pane (see `resize.rs` unix path). Without tmux, the source becomes the outer controlling tty directly on Linux/macOS, and `ReadConsoleInputW` (`WINDOW_BUFFER_SIZE_EVENT`) on native Windows. The ConPTY scaffolding already called out in the Resize Handling section covers this case.
3. **IPC discoverability.** The socket path keys off session id, not tmux, so nothing in `ipc.rs` needs to change. What *does* change is how *other* processes find the socket: today "`/agent-doc` routing from a different tmux pane" (see Use Cases) assumes a tmux pane id; outside tmux the discovery key becomes session id or pid.
4. **Claim / binding model.** `agent-doc claim` binds a document to a tmux pane id. With no panes, the binding unit becomes something else — pid, socket path, or tty name. This is the biggest conceptual change, not a technical one, and needs its own spec (what does "focus" mean without tmux? what does `agent-doc focus` do?).

**Non-goals for this future mode:** visual multiplexing. If a user wants multiple simultaneous claude sessions visible at once, tmux (or an equivalent multiplexer) is still the right tool. `--no-tmux` is for single-session use cases — typically a developer running one claude in their terminal without a multiplexer layer, or an embedded context (editor terminal, CI, container entrypoint) where tmux is unavailable or undesirable.

**Not in phase 1.** Every current consumer (livestream setup, `/agent-doc` cross-pane routing, the claim system, `agent-doc focus`) assumes tmux. Dropping tmux requires re-specifying those consumers, which is a separate design cycle. This section exists so the four requirements above are not lost when phase 1 ships.

## Logging

Single log file per session at `.agent-doc/logs/<session-uuid>.log`, same path as today. Supervisor events use a `[supervisor]` tag prefix on each line for filtering. Format: `[<timestamp>] [supervisor] <event> key=value ...`, where `<timestamp>` is a human-readable ISO-8601 UTC stamp `YYYY-MM-DDTHH:MM:SSZ` (`#opslogts`) so operators reading the log can correlate events to wall-clock time. The staleness / accretion / startup-miss / `gate_verify` scanners read it back through `agent_doc_log_time::parse_log_timestamp`, which also still accepts the pre-`#opslogts` bare-epoch form for backward compatibility.

Example:
```
[1713041234] session_start file=tasks/plan.md pane=%12 session=abc12345
[1713041235] document_cycle phase=preflight_started cycle=cycle-1713041235000 event=preflight_started
[1713041237] document_cycle phase=response_captured cycle=cycle-1713041235000 event=response_captured capture_id=cycle-1713041235000
[1713041238] document_cycle phase=write_applied cycle=cycle-1713041235000 event=write_template
[1713041239] document_cycle phase=committed cycle=cycle-1713041235000 event=commit_success
[1713041234] [supervisor] pty_allocated rows=40 cols=120
[1713041234] [supervisor] cwd_resolved path=/home/brian/work/agent-loop source=project_root
[1713041234] [supervisor] claude_spawn pid=54321 mode=fresh
[1713041290] [supervisor] claude_exit code=0 exit_kind=success exit_status="Success"
[1713041291] [supervisor] user_action=restart
[1713041291] [supervisor] claude_spawn pid=54398 mode=continue
[1713041390] [supervisor] codex_exit code=129 exit_kind=signal exit_signal="Hangup" exit_status="Terminated by Hangup"
[1713041390] [supervisor] restart_eval pane=%12 harness=codex exit_code=129 exit_kind=signal exit_signal="Hangup" exit_status="Terminated by Hangup" auto_trigger_outcome=sent ctrl_d=false state=healthy action=restart_after
[1713041390] [supervisor] auto_restart delay=2s with_continue=true restart_count=1
[1713041391] [supervisor] codex_spawn pid=54444 mode=continue
[1713041421] [supervisor] auto_trigger_timeout pane=%12 harness=codex reason=no_prompt_after_30s
[1713041425] [supervisor] resume_restart_failed pane=%12 harness=codex outcome=timeout recent_failures=1 window_secs=900 restart_count=1
[1713041450] [supervisor] supervisor_exit reason=user_quit_clean_exit pane=%12 restart_count=1
[1713041450] session_end
```

Document closeout phases share the same per-session log:
- `document_cycle phase=preflight_started ...`
- `document_cycle phase=response_captured ...`
- `document_cycle phase=write_applied ...`
- `document_cycle phase=committed ...`

Those entries are not supervisor lifecycle events, but they must land in the same `.agent-doc/logs/<session>.log` timeline so crash forensics can line up child exit / pane-loss provenance with the exact `state.db` closeout transition.

At minimum, harness exit lines must preserve:
- exit code
- exit kind (`success`, `exit_code`, or `signal`)
- signal name when the child died from a signal
- the rendered child status text used for operator forensics

The final supervisor-owned closeout must append `supervisor_exit reason=...` before `session_end` so later crash analysis can distinguish deliberate quits, IPC stops, and flapping halts from missing-pane recovery written by other components.

Session-log consumers must treat any event whose first token is `session_end` as a closeout boundary, even when origin metadata follows on the same line (for example `session_end origin=registry_rebind ...` or `session_end origin=sync_missing_pane`). Provenance analysis must not require the whole line to equal the bare literal `session_end`.

When pane-loss recovery discovers that the current cycle is already `response_captured` or `write_applied`, the same session log must also record the recovery attempt before the synthetic `session_end origin=sync_missing_pane` closeout. The minimum provenance is:
- `sync_missing_pane_closeout_recovery_start ... phase=response_captured|write_applied durable_capture=<bool>`
- either `sync_missing_pane_closeout_recovery_result ... outcome=...` or `sync_missing_pane_closeout_recovery_failed ... reason=...`

That keeps pane-loss forensics and closeout forensics on the same timeline: operators can tell whether sync replayed the durable capture, finished a missing commit boundary, or failed closed and left the capture for later `preflight` / `repair`.

Auto-trigger provenance is lifecycle-bound to a single restart iteration:
- each restart spawns at most one auto-trigger thread
- when the child exits, that thread is explicitly cancelled and joined before
  the next restart iteration begins
- if a resumed child exits cleanly before the auto-trigger ever sends, that
  still counts as a failed resume handoff; clean exit alone does not clear the
  failed-resume guard
- stale auto-trigger workers must never outlive the child they were waiting on,
  so they cannot inject commands into a later replacement child in the same pane
- after the prompt appears, the trigger command is written through the
  supervisor-owned child pty writer rather than tmux pane stdin, so a late
  trigger cannot hit the supervisor restart prompt or a non-child process in
  the pane
- supervisor shutdown signals both the auto-trigger stop flag and the
  stdin->pty writer stop path before joining either thread, and the shared
  child-pty writer lock/write path is cancellation-aware, so a stop request
  does not hang behind a blocked stdin->pty writer mutex wait

This keeps the existing `.agent-doc/logs/<session>.log` contract intact for any downstream tooling (`agent-doc logs`, dashboards) and avoids a second log file to rotate.

Long-lived log storage is bounded independently from open-cycle behavior:

- active `ops.log`, `cycles.jsonl`, and per-session logs rotate at 32 MiB;
- each active file retains one zstd-compressed segment, forming a fixed storage
  budget while keeping the most recent audit history;
- rotation uses the project log directory's `.rotation.lock`; shared supervisor
  writers take the same lock for appends, reopen the active inode after a
  rollover, and all cloned handles share one writer instead of cloning file
  descriptors;
- automatic rotation is scheduled only after a durable cycle-closeout entry,
  with an admission-time check for legacy oversized session logs. It is never
  triggered by navigation, sync, an open cycle, retained-response persistence,
  or editor reconciliation;
- log consumers concatenate the retained segment and active tail, including
  crash-recovery staging files, so restart metrics, session-boundary detection,
  and proof-marker verification retain their audit window;
- supervisor telemetry reports open file-descriptor count separately from log
  byte thresholds at admission and child launch/restart boundaries.

### Idle-queue watch (`#jb-run-agent-doc-busy-queue-dispatch-deadlock`)

When a busy-pane `Run Agent Doc` route cannot inject into an active turn, it
inserts the prompt **ahead of queued active-loop items** in plain `agent:run` (a
manual operator dispatch preempts the loop rather than landing at the tail —
`#jb-run-preempt-autoloop-priority`; the priority insert lands after any leading
queue directive such as a preset/start fence and never supersedes a lone active
prompt, so the pending loop item is preserved; route-owned queue writes never add
`auto` and strip legacy `auto` from touched tags), sets `queue_active: true`,
and returns `Ok` (`route.rs` `AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue`). The
drain is otherwise harness-delegated: the Codex `Stop` hook drains on turn-end,
and Claude relies on `/loop` or a manual re-trigger. A Claude session not running
`/loop` therefore has no guaranteed drain, so the queued head can sit forever —
the operator-perceived "deadlock" that forces an agent restart.

The supervisor closes that gap with a long-lived **idle-queue watch** thread,
distinct from the one-shot restart auto-trigger:

- It runs for the whole child lifetime (spawned next to the auto-trigger,
  cancelled + joined on child exit), polling on the same
  `AUTO_TRIGGER_POLL_INTERVAL`.
- The drainable head is the shared `queue_continuation::live_continuation_head`
  definition: frontmatter `queue_active: true`, explicit `go` mode
  (`queue: go` or marker-side `go`), an active `resolve_activation`, and a ready
  prompt head. Inactive-residue queues (`queue_active: false`) and plain
  persisted-active queues without `go` are never drained passively. An explicit
  dispatch-only `Run Agent Doc` against a busy authoritative actor may promote an
  already-startable inactive queue (marker-side `go`/`start`, a start fence with
  a ready prompt head, or legacy `auto`) by setting `queue_active: true`, syncing
  the snapshot, stripping legacy `auto`, and returning the same deferred
  busy-route feedback; unattended continuation after closeout still requires
  explicit `go`. Plain inactive queues without a start trigger stay inert.
- The drain decision is the pure, deterministically tested
  `idle_queue_drain_decision(prompt_visible, active_head, last_dispatched)`:
  - `Dispatch` only on a busy→idle transition (`prompt_visible`) with an active
    head that differs from the last one dispatched.
  - `SkipNotIdle` whenever the pane is mid-turn — the same
    no-inject-into-active-turn invariant the route busy path enforces.
- `SkipAlreadyDispatched` dedups a head that is still present after a dispatch
(cycle not yet consumed, or the dispatch failed to drain), so a stuck head
cannot hot-loop the watch every idle tick.
- **Deferred-head liveness (`#deferredliveness`).** A committed response that
  exactly quotes and explicitly defers the still-active head proves the prior
  dispatch ended without consuming it. That cycle is a new continuation edge:
  the watch rearms `SkipAlreadyDispatched` once for the cycle, clears or freshly
  restarts the harness under the normal auto-clear policy, and redispatches the
  retained head. The cycle-id fence prevents idle-tick hot loops. Thus the
  state `pending deferred head + no active owner + no scheduled continuation`
  is forbidden for active `go`; when automatic continuation is unavailable,
  closeout names the explicit `agent-doc <FILE>` action and the blocker that
  must be resolved first.
- Ordinary heads are Lazily state edges, not recursive CLI commands. The
process-scoped graph derives one continuation effect from the head identity,
tracks document-state and failed-effect retry epochs separately, and consumes
the edge only after the controller projects a new cycle identity. The
owner-pane payload names the active head and explicitly continues the existing
session; it never renders `agent-doc <file>`.
- A prior `abandoned` recovery cycle is a terminal inter-item boundary, like
`committed`, once the remaining editor/IPC/actor convergence proofs hold. It
must not strand a later operator-authored head behind an impossible
`committed` proof.
- Before tmux delivery, the watcher derives payload recognition and
dispatch-ready state from one live pane capture. A missing capture defers
without writing or recording the head; a non-ready composer that does not hold
the queue payload is treated as operator-owned and also defers.
- A capture that proves the queue payload is the current draft may submit the
harness Enter key once. A capture that instead proves an empty dispatch-ready
composer may retire only the matching stale prompt-dispatch projection before a
fresh text+Enter attempt. Prompt-dispatch deduplication is never itself delivery
proof and cannot cause the queue head to be recorded as dispatched.
- `#ghostsuggestresubmit` (the idle-drain twin of route's `#routeghostdraft`):
a prompt row whose body renders dim (SGR 2) is a harness suggestion inside an
empty composer, never the pending payload, even when its stripped text equals
the trigger byte for byte. `dispatch_payload_pending_in_current_input` masks
such rows to their bare prompt glyph before matching, so the drain dispatches a
fresh text+Enter instead of pressing Enter on an empty composer. Observed
2026-10-11: Claude Code suggested the previous `/agent-doc <FILE>` dimly, the
probe read `payload_already_pending=true`, the bare Enter was a no-op, and the
queue continuation never started.
- The watcher persists a redacted pane snapshot together with its
`payload_already_pending`, `dispatch_ready`, and selected-action observation,
deduped by queue head and capture hash, so a stranded draft can be diagnosed
without separate, race-prone pane and policy samples.
- If actor state, supervisor runtime actor state, and controller lease are all
`ready` while a stale Codex queued-draft renderer cue still makes the pane
probe look `alive-busy`, the watch debounces that ready/busy conflict for the
  stale-busy repair window, logs `owned_pane_ready_busy_conflict`, and treats the
  pane as dispatchable. Active-turn, permission, hook-review, shell-search,
  help, and clean-exit blockers still produce `SkipNotIdle`.
  - `SkipNoActiveHead` clears the dedup so a later re-enqueue of the same prompt
    text fires again.
- **Queue auto-clear (`#bgclearpolicy`).** An active `agent:run` resets the
  agent context between items by default. In the drain-dispatch path — after
  every dispatch gate has proven the item boundary (dispatch-ready prompt, no
  live turn, no route submit in flight, no `/loop` drain owner, the prior cycle
  committed and converged) and before the next head is submitted — the watch
  clears once when a cycle committed since its baseline AND the queue head moved
  past the head that cycle worked on. The baseline (latest committed cycle +
  head) is recorded per harness generation by the supervisor BEFORE it spawns or
  adopts the child (`#autoclearbaseline`), so an item dispatched to that
  generation — by the watch, Run Agent Doc, or the operator — can never complete
  before it; its head is the disk head at that moment. The watch's first
  resolved queue observation then replaces that provisional head with the
  authoritative one only if no cycle has committed since the generation
  started; if one has (a slow first tick, or editor authority unavailable while
  an externally dispatched item committed), the generation-start baseline is
  kept, so that completion still earns its clear instead of being counted as
  already seen (CI 39463326025 dispatched item two with no `/clear` that way).
  Queue-row presentation is not boundary evidence: the completed row may remain
  struck and the next head may carry a live status marker such as `📥`; head
  advancement is determined by the queue parser, never raw row text.
  A harness generation is the harness CONTEXT, not the supervisor process: when
  the supervisor re-`execve`s itself and adopts the live child
  (`via=execve_preserve_child`, a stale-binary/explicit recycle or a
  restart-drain reexec), the outgoing image hands its current baseline and
  whether it was already refined to the replacement through
  `AGENT_DOC_REEXEC_QUEUE_AUTO_CLEAR_BASELINE`, and the replacement's watch
  continues it instead of recording a new one. An item that committed just
  before the re-exec but had not been cleared yet therefore still earns its
  clear; a baseline re-recorded at reentry would already contain that item's
  committed cycle and the next head. The replacement logs
  `queue_auto_clear_generation_baseline source=carried_across_reexec`. The
  handoff is consumed at every start (it never reaches a harness child) and is
  honored only when the preserved child is actually adopted: a child that died
  across the exec, or a harness change, starts a fresh context and records a
  fresh baseline (`source=carried_discarded_fresh_child`). An unreadable
  handoff, an older outgoing image without one, or a baseline whose encoding
  exceeds 64 KiB (an oversized environment string would fail the `execve` with
  `E2BIG` and disable the hot-reload; `queue_auto_clear_baseline_not_carried
  reason=handoff_oversize`) falls back to recording a fresh baseline.
  The baseline is re-taken whenever a clear is sent or settles, so a boundary is
  cleared at most once and a failed or unobserved clear is never retried into
  the pane. It never clears mid-turn
  (latest cycle open), over an operator draft in the composer, while settled
  exchange steering is unanswered, while a subagent claim on the document is in
  flight (the coordinator's context must survive to integrate it), or when the
  composer cannot be observed; those cases dispatch the head to the current
  context instead (an operator draft still defers the dispatch itself). Delivery
  follows the idle-Clear split (`#idleclearrestart`): Claude Code and OpenCode get
  their native clear (`/clear`, `/new`) typed into the SAME live child, so
  background agents and shells survive, with a `queue_auto_clear` context-clear
  projection that the settle gate holds the next dispatch behind; Codex, whose
  `/clear` exits the process, gets a supervised fresh restart whose relaunch
  re-triggers the document. Codex is never typed `/clear`. Each decision is logged
  to `ops.log` as `queue_auto_clear ... delivery=native_clear|restart_fresh
  result=...` or `queue_auto_clear_skipped reason=...`.
  Bare `clear` positively declares this default policy and is present in generated
  templates. Direct queues clear per completed turn; `subagents`/`fan-out` queues
  wait for the next safe idle boundary after claims settle. Omission remains
  enabled for existing queues. Legacy `disable-auto-clear` turns it off; the queue
  then drains in one context and only an explicit operator Clear resets the session. Auto-clear
  does not reduce session accretion: `block` measures the document's exchange
  size, which only compaction reduces.
  - The in-session drains hand the boundary over only when the supervisor can
    take it (`supervisor_drain_readiness_for_doc` is ready and no earlier
    hand-off is overdue): preflight reports `queue_auto_clear_yield: true` with
    `queue_continuation_required: false`, session-check prints
    `queue_continuation_required=false queue_auto_clear_yield=true`, records the
    supervisor drain hand-off, and releases a `/loop` drain-owner lease, and the
    Codex Stop hook allows the stop (`codex_stop_queue_continuation_yield`)
    instead of continuing in-pane. Otherwise the in-session drain continues in
    the same context — never a stall.
  - Drain readiness (`supervisor_drain_readiness_for_doc`) reads the
    current-generation supervisor lease and fails closed on missing evidence:
    no lease, pid, or live pid is `no_live_supervisor`; a live pid that does
    not map the installed binary is `supervisor_binary_stale`. Otherwise the
    supervisor is `ready` when its lease heartbeat is fresh OR the live pid's
    `agent-doc start` command line still names this document
    (`#codexstoplabel`). The lease heartbeat is written only on register,
    lifecycle transitions, and status reconcile, never periodically, so an
    owning supervisor reads heartbeat-stale whenever its agent's turn outlasts
    the 60 s window; heartbeat age is a timer, not evidence (`#netadv5` R6).
    `supervisor_heartbeat_stale` is reserved for a live pid with a stale
    heartbeat whose command line does not name the document (pid reuse, a
    re-exec onto another document, unreadable `/proc`). An owning supervisor
    that is alive but never drains is caught by the overdue hand-off gate
    below, not by heartbeat age. A `supervisor_recycle_request_settled_without_reexec
    ... cause=no_longer_live` line is unrelated: it means the recycle
    request's cause (for `install_fanout`, a stale binary) no longer holds
    because the supervisor already re-exec'd, not that the supervisor is gone.
  - A hand-off names the head the supervisor will actually dispatch
    (`#claimeddrainwarn`): the first supervisor-drainable head that no worker
    claims, skipping claimed and subagent-held heads exactly as the
    supervisor's idle drain does. When every remaining head is claimed, no
    hand-off is recorded and any earlier one is cleared. The undrained check
    (`#supdrainyieldfalsifiable`, which feeds the preflight
    `supervisor_drain_handoff_undrained` warning and the "earlier hand-off is
    overdue" gate) compares against that same unclaimed head; once the recorded
    head is no longer it (drained, claimed, or nothing supervisor-drainable
    remains) the hand-off is settled and its record cleared, so a claim released
    later never revives a stale age. The warning recommends recycling only a
    supervisor that is not ready; a ready supervisor is healthy, and the remedy
    is to drain the head in-session.
  - Why background clears were turned off on 2026-06-30 (`45ff25f994`) and how
    auto-clear avoids it: the old reset typed `/clear` into Codex while Codex
    still had a task in progress, re-armed every tick from `[focused-cycle]` head
    tags (`#qfocsup`); provenance-less markers let a restarted watch press Enter
    on a stale `/clear` draft. Auto-clear restarts Codex instead of typing,
    fires once per completed item, carries `queue_auto_clear` provenance, and the
    orphan-`/clear`-draft recovery stays off.
- The pre-`#bgclearpolicy` background reset reasons stay disabled:
  `[clean-session]`, `[focused-cycle]`, and accretion/threshold reasons never by
  themselves cause the supervisor to submit `/clear` or `/new`. Only explicit
  operator clears, explicit queued slash-command heads, and queue auto-clear may
  create a context-clear projection. Busy-loop operator clears first record `QueueContextClearDeferred`;
  the idle-watch promotes that projection to `QueueContextClearStarted` only
  after it submits the deferred clear. For explicit in-flight sources, the
  projection survives supervisor recycle/restart so the replacement watcher
  blocks drains until the clear settles, or presses the shared submit key once
  when the clear command is still visible in the composer. Legacy or
  background-sourced projections (no source, or the retired
  `supervisor_background_context_reset`) are settled rather than submitted. Once the pane
  settles for `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` consecutive polls the projection
  records `QueueContextClearSettled`. A
  manual clear cooldown remains authoritative for a plain operator clear with no
  active queue (it suppresses passive dispatch until cleared by the existing
  operator route path) and for an operator-deferred clear that explicitly paused
  the loop. But a manual clear cooldown must NOT suppress an active go-mode queue
  drain forever (`#clearcontresume`): the cooldown only exists to avoid
  dispatching a trigger into an in-flight `/clear`, so once the cleared pane
  settles to a fresh idle prompt for `CLEAR_COOLDOWN_RESUME_IDLE_TICKS`
  consecutive polls AND a `queue_active: true`
  head is waiting AND no operator-deferred clear is still pending delivery, the
  watch settles the cooldown projection and resumes the drain. The recycle +
  clear is then a continuation *step*, not a stall. The decision is the pure,
  unit-tested `decisions::clear_cooldown_resume_ready`. The full operator recycle
  + clear pipeline — `admin recycle` (mark recycle at the next idle boundary) →
  in-place `execve` recycle preserving the live pane → `session clear` (record
  the clear-cooldown/deferred-clear projection) → submit/settle → cooldown
  auto-expiry → go-mode drain resume — is also driven end-to-end by the
  deterministic SimWorld engine
  (`SimCommand::SupervisorIdleQueueTick`), which calls the SAME production
  predicates (`supervisor_recycle_action`, `clear_cooldown_resume_ready`,
  `idle_queue_drain_decision`) the live idle-queue watch uses rather than
  reimplementing the policy, so the recycle + clear pipeline is simulated offline
  across its interoperating systems.
- `#stalesupresumedeadlock`: an open cycle defers a recycle only while the cycle
  has no durable replay checkpoint. Three capture-backed facts cross it at a
  supervisor-safe checkpoint (IPC drained): a proven editor-IPC write wedge, a
  typed stale editor-delivery request, and a generation already due for
  replacement (the running binary is observably stale) whose own
  captured-finalize resume has latched `needs_operator`. The resume runs inside
  the supervisor, so a stale generation judges the retained response with stale
  code; deferring on the cycle it cannot close is a circular wait (2026-09-30,
  06:17-06:35 UTC). The idle watch logs `supervisor_stale_capture_resume_recycle`,
  and the replacement starts without the latch or the request marker, so the
  escape fires at most once per episode. A verdict the fresh build also latches
  stays with the operator.
- Recycle-request markers are durable delivery of an intent, not proof that the
  producer's cause still holds. At the policy-owner boundary, install fan-out
  and stale-supervisor requests require the running generation to remain stale;
  editor-write-wedge requests require a current, not-yet-attempted wedge; and
  stale-replica requests require the matching replica condition. A request that
  arrives after its repair is settled without `execve`. Explicit operator and
  forced requests remain unconditional. The editor-health `recycle_attempted`
  bit is monotone within a session so a concurrent stale health writer cannot
  re-arm the same wedge episode after the supervisor latches it.
- `#renamedsupervisorrecycle`: install fan-out preserves the live supervisor PID
  alongside its `/proc` document observation. A route-owned supervisor may
  outlive a document path transition (for example, its command line still names
  a removed path while the controller lease names the current path). When the
  stale startup path cannot be checkpointed and therefore cannot carry the
  document-hash recycle marker that process polls, fan-out resolves the
  authoritative supervisor lease by PID, verifies the socket reports that same
  PID, and sends the existing safe-boundary `restart continue` lifecycle intent
  over supervisor IPC. Both rungs require positive delivery evidence:
  checkpoint `Ok(None)` and the compatibility marker writer's absent-identity
  `Ok(())` are no-ops, not successful scheduling. The observed marker API makes
  that latter result explicit, and either absence enters the lease/PID fallback.
  A missing lease, PID mismatch, unreachable socket, or IPC refusal is counted
  as skipped; fan-out never signals or kills an unverified process. The normal
  document marker remains the deduplicated primary delivery for resolvable paths.
  A successful same-process `execve` also revalidates its startup document before
  admission. If the replayed argv path was removed by a rename, a preserved-child
  reentry may adopt exactly one existing, non-closed document whose authoritative
  current-generation supervisor lease names its unchanged PID in the same project.
  This lease recovery is unavailable to ordinary starts, foreign PIDs, closed
  actors, missing replacement files, and ambiguous matches; those cases retain
  the requested path and fail normally rather than redirecting ownership.
- `#wd40` / `#supboundarylivelock` state-flush: an explicit `admin recycle`
  recycles at the first supervisor-safe checkpoint even when the installed binary
  already matches the running supervisor (`supervisor:fresh`). A checkpoint is
  safe when supervisor IPC is drained and either the harness is at a turn boundary
  or the document cycle has committed. The latter must not wait for the enclosing
  harness turn marker to retire: an in-place supervisor `execve` preserves the live
  harness child, while waiting inside closeout would make the recycle depend on the
  same turn that is waiting for it. An open document cycle or in-flight supervisor
  IPC remains authoritative and keeps the durable recycle request pending. The
  recycle then restarts the supervisor *process*, which
  rebuilds its in-memory state — flushing a lagging CRDT projection
  (`projection_lag`) that can keep re-pinning a queue head (`#rt83` phantom-pin
  churn) even though there is no stale binary to swap. `supervisor_recycle_action`
  therefore returns `RecycleImmediate` for `explicit_admin` on a fresh binary
  (previously a silent no-op that left the operator unable to clear the lagging
  projection); a bare wedge on a fresh binary still stays a no-op. When a
  self-driving `/loop` holds the drain `turn_active`, the idle-watch records a
  `state_flush_drain` recycle-yield reason in the lazily-backed supervisor
  recycle projection (mirroring the `stale_binary_drain` path) so the loop
  yields one inter-item boundary and the process restart fires. Preflight,
  session-check, and queue continuation detection observe the projection through
  controller RPC instead of a `.agent-doc/recycle-yield` marker. Routine
  non-explicit maintenance still respects `turn_boundary`.
- `#midturn-controller-recycle` / `#recycleforce`: a project controller may
  start a private replacement even while durable harness dispatches or
  controller RPCs remain open. Socket promotion redirects new RPCs, then the
  unreachable predecessor drains already-accepted RPCs before retiring. The
  route-owned supervisor owns the harness child and pane, while
  dispatch/cycle/CRDT recovery state is durable in SQLite; therefore the
  controller replacement does not interrupt the turn. A non-stable handoff is
  the strict launch gate.
  `#handoffsocketownership`: once a self-recycle worker owns the predecessor's
  outgoing handoff, the predecessor's stranded-controller self-watchdog is
  disarmed through promotion and drain. After the private replacement socket is
  renamed onto the public path, the predecessor must relinquish its listener's
  pathname-reclamation guard before exit; dropping the predecessor listener
  must never unlink the successor's live `controller.sock`.
  `#replacementdrain`: a supervisor replacement worker
  (`handle_supervisor_replacement` → `drive_supervisor_replacement_background`)
  also returns its receipt before it finishes, and it may already have asked
  the supervisor to stop. Each worker holds a `SupervisorReplacementWorkerInFlight`
  guard. A promoted predecessor's drain counts those workers with the async
  editor workers (`controller_handoff_predecessor_retired_authority ...
  replacement_workers=`), under the same bound, so a self-handoff no longer
  exits under a replacement and strands its cold start. On 2026-10-20 the
  predecessor logged `workers=0` and exited mid-replacement.
  `#handoffrouteforward`: async editor commands (`editor_command_submit_async`:
  Run Agent Doc `editor_route`, `sync_tmux_layout`, `focus_document_pane`) are
  answered `accepted` before their worker dispatches, so they hold no client
  connection and the client-connection drain alone does not cover them. After a
  self-handoff promotes its successor, the predecessor marks the successor
  promoted and its drain also waits for every admitted async worker, bounded by
  twice the handoff settle budget (abandoned workers are logged as
  `controller_handoff_drain_abandoned_async_workers`). A worker that has not
  dispatched (every attempt so far was a side-effect-free `controller not
  authoritative` refusal) forwards the same `command_id` to the successor over
  the public socket (`editor_command_async_forwarded`), awaits the successor's
  terminal result, and publishes it as its own, so the editor's
  `editor_command_await` resolves on either process. The successor admits a
  known `command_id` only once (`already_admitted`), so a forward and the
  editor's own replay converge on one worker, and an await for an unknown id is
  held while the predecessor named by `previous_controller_pid` is still a live
  same-project controller. If forwarding fails the route is rejected
  explicitly with `handoff_forward_failed` and a re-run instruction; it is never
  dropped silently.
  `#drainingpredecessor`: the drain only helps if nothing kills the
  predecessor first. A predecessor that is draining holds a durable drain lease
  (`predecessor_drain_lease` in `state.db`: pid, generation, deadline), written
  before the public socket is renamed (self-handoff) or by the delegating client
  while it still holds the launch claim (below). Same-project duplicate reaping
  (`reap_stale_duplicate_controllers`, run at promotion and launch-recovery
  transitions) skips the leased pid until the deadline
  (`controller_duplicate_reap_protected_draining_predecessor`);
  after it, a wedged predecessor is reaped as before. The orphaned-preparing
  process scans (`reap_orphaned_preparing_controllers_for_caller`, run by
  `connect_or_launch`, `admin recycle` and gc, and the cross-project
  `reap_orphaned_preparing_controllers_all_projects`, which reads a foreign
  root's lease read-only) apply the same lease before any age test
  (`orphaned_preparing_reap_protected_draining_predecessor ... caller=`).
  A handoff replacement keeps its immutable `--handoff-state preparing` argv
  for life, so when it later hands off in turn it is a "preparing" process
  older than the threshold, and once its successor's bootstrap leaves
  `Preparing`/`Promoted` the lease is the only record that it is draining,
  not wedged (GH #269: a 0.35.505 → 0.35.508 upgrade SIGTERMed the
  predecessor in the second its 360 s lease was written, judged by its
  2121 s process age). An orphan reap names the process by its own
  `--controller-generation` argv, 0 when absent, never by the bootstrap
  record, which belongs to another controller. A client that finds a
  Stable controller on a stale binary no longer hands it off itself: it first
  asks it to `recycle_force` (reason `stale_binary_client`), leases it, releases
  the launch claim, and polls `status` (bounded by one replacement hydration,
  `HANDOFF_CONNECT_WAIT`) until a successor on the current binary is adoptable
  (`controller_active_stale_binary_restart_delegated` →
  `controller_active_stale_binary_restart_completed ... via=self_recycle_delegated`).
  Only when the stale controller refuses the delegation, or is still the same
  Stable generation after the budget, does the client fall back to the direct
  handoff, after expiring the lease so promotion reaps the undrained
  predecessor. Before this, a `make install` that landed while a Run Agent Doc
  route was in its final ready probe SIGTERMed the controller running it in the
  same second as promotion; the editor's await dropped and it replayed the whole
  route against the successor (2026-10-10 `tools.md`: dispatched after 58s
  instead of ~12s, including a stale-supervisor re-exec the original route had
  already passed).
  `#drainingpredecessor` retirement (GH #261): promotion ends the
  predecessor's controller role at once, not when it exits. Before it waits on
  anything, the draining predecessor
  (`ControllerRuntime::retire_after_successor_promotion`, logged as
  `controller_handoff_predecessor_retired_authority`):
  1. stops its own periodic controller work. The supervisor watchdog and the
     orphan-queue drain never tick again in that process, because the successor
     owns both. It only finishes requests and async workers it had already
     accepted;
  2. releases every parked state-plane subscriber and makes later ones return
     at once. The editor's `document_turn_authority_stream` (one per open
     document) then ends, and the editor reconnects to the successor. That
     stream used to end only at `should_stop`, which the drain set only after
     the stream closed, so a predecessor on a deleted binary lived until the
     IDE went away;
  3. retires its relay hubs as document authority for its project root. Editor
     replicas re-register with the successor at promotion, so those hubs froze
     at the handoff, yet their `delivery_converged` witness still read `true`.
     After retirement, `embedded_relay_is_available_for_file` is false, so
     in-process callers read through the public socket like any client, and a
     direct hub read reports `editor_attached_model_missing`
     (`crdt_current_text_unavailable ... reason=relay_authority_retired`)
     instead of serving pre-handoff text as current.
  The drain is bounded for open client connections as well as for async
  workers. The budget is the lease length (`ASYNC_EDITOR_PREDECESSOR_DRAIN_BUDGET`),
  because past the lease duplicate reaping no longer protects the process
  anyway. Anything still open when the budget runs out is logged as
  `controller_handoff_drain_abandoned_clients` or
  `controller_handoff_drain_abandoned_async_workers`, and the process exits.
  Retirement leaves the `#handoffsocketownership` contract unchanged: the
  self-watchdog stays disarmed through the drain, and the predecessor never
  unlinks the successor's `controller.sock`.
  `#lightweightadoption`: a client that finds an already-serving Stable
  controller adopts it from `handoff_status`, the lightweight generation and
  binary-identity receipt. That steady path neither requests the full operator
  `status` projection nor runs duplicate-process discovery/reaping. This path is
  paid by every ordinary RPC and every renewed state-plane subscription;
  durable-store diagnostics and all-process scans are reserved for explicit
  status commands and actual promotion/recovery transitions.
  `agent-doc admin recycle --force` is a real flag (no
  `-- ` separator needed), composes with `--all-projects`, and skips only the
  normal controller debounce.
  When NO live controller answers the single-project form and a document path was
  supplied (`agent-doc admin recycle <FILE> --force`), the no-op "nothing to
  recycle" message is replaced by an escalation to the kill+cold-start path
  (`session restart-supervisor`, which carries the same self-ancestor guard as
  `admin kill-supervisor`, so a forced escalation never tears down the caller's own
  ancestor supervisor). In that no-controller fallback only, `--force` also
  permits interrupting a busy pane. Without `--force`, the supervisor fallback
  keeps its busy-pane guard. `--json` adds a `forced: bool` field (and
  `escalated_cold_start: bool` on the single-project arm).
- On `Dispatch` it injects a harness-specific trigger through the same
  `auto_trigger_inject_command` path (capability-proof gated, actor marked
  `busy` before bytes). Claude/OpenCode receive the normal harness trigger
  (`/agent-doc <FILE>`), while Codex receives the bare Codex-compatible trigger
  (`agent-doc <FILE>`). The queue-continuation prompt contract belongs to the
  installed harness instruction surface and runbooks, not to the editor/supervisor
  payload itself. A failed inject is not recorded as dispatched, so it retries on
  the next idle tick. Successful drains log `idle_queue_watch_drain` with
  `payload_kind=trigger` and
  `submit_mode=tmux_hex_text_cr|pty_cr`; failures log
  `idle_queue_watch_drain_failed`.
- **Stale-busy self-heal (`#stale-busy-after-auto-inject-no-clear`).** The
  one-shot busy→ready completion transition on the pty→stdout thread is
  edge-triggered on the latest output chunk. When an injected turn returns but
  its composer redraw lands split so the final chunk carries no detectable
  prompt, the actor can stay wedged `busy` over an idle pane with no further
  bytes to retrigger ready, so the session presents as "truly stuck" and even a
  pane kill + restart re-enters the state. The watch closes this with
  a polling backstop driven by the pure
  `stale_busy_idle_reconcile_decision(actor_busy, pane_has_busy_cue, clear_cooldown_active, ticks)`:
  when the in-memory actor is `busy`/`starting`, a fresh `tmux capture-pane`
  shows no harness `has_busy_cue` (the same direct evidence `route.rs` uses for
  its stale-busy repairs — not the supervisor's edge-triggered pty buffer that
  missed the redraw), no clear cooldown is pausing the loop, and the
  idle-over-busy condition has held for `STALE_BUSY_RECONCILE_TICKS` consecutive
  polls (~2s debounce so a turn still spinning up is never cut short), the watch
  transitions the actor back to `ready` (`caller=supervisor reason=idle_pane_reconcile`,
  persisted through `mark_lifecycle`), resets the prompt latch, and logs
  `idle_queue_watch_stale_busy_reconciled`. The repair is edge-triggered on the
  actor's busy entry, keyed `generation:dispatch-epoch:busy-entry`: an unchanged
  poll of one busy episode writes once, and each landed transition INTO
  `busy`/`starting` (a dispatch, a harness permission prompt resolving, a child
  restart spawn) licenses one more repair (`#drainbusymask`). A repair whose
  write did not land (the in-memory actor is still `busy` after the transition,
  e.g. a retryable controller failure) re-licenses its key and retries after the
  next full debounce, logging `idle_queue_watch_stale_busy_reconcile_unlanded`
  once per episode. Keyed on the dispatch epoch alone, either case stayed `busy`
  until the next dispatch, and `admin queue drain` admits no dispatch to a
  non-`ready` actor (`actor_busy_draining`), so the drain wedged. This repair is
  the only writer that heals a stale `busy` record during a drain: the route's
  own pane-evidence promotion is in-memory (see
  [Session Tmux Commands](07-session-tmux-commands.md)). It must preserve the dispatch dedup
  for the current head: if the injected command returned without consuming the
  same active head, the next idle tick must skip it as `SkipAlreadyDispatched`
  instead of re-injecting the same drain payload in a loop. The dedup clears
  only when there is no active head or the head advances. This recovers the
  wedge with no pane kill and no operator `session status`/`session clear`.

Live end-to-end verification (a real busy Codex/Claude pane returning to idle and
draining the route-appended head with no duplicate injection into the active
turn) stays operator-gated.

## Dependencies to Add

- `portable-pty` — pty allocation. Supports Unix pty + Windows ConPTY under one API, which we need because Windows is a supported target.
- `signal-hook` (`#[cfg(unix)]`) — SIGWINCH handler on Unix.
- `winapi` or `windows-sys` (`#[cfg(windows)]`) — `ReadConsoleInputW` for resize events, only for future non-tmux Windows sessions. Phase 1 can stub the Windows resize watcher since WSL handles the tmux case via Unix SIGWINCH.
- No new async runtime — the IPC socket accept loop runs in a std thread, same pattern as `ipc_socket.rs`.

## Testing Strategy

- **Unit:** CWD resolution priority, env whitelist, crash classifier, flap detection.
- **Integration:** spawn supervisor with a fake claude (a shell script that exits with a configured code after a configured delay), drive it via the IPC socket, assert state transitions.
- **Smoke:** run real claude under supervisor in a tmux pane, verify restart + inject + CWD invariant end-to-end. This is the "manual testing gate" from the release checklist.

## Files to Add / Modify

```
src/agent-doc/
  src/
    supervisor/
      mod.rs          # public entry: run(file, opts)
      pty.rs          # pty allocation + reader/writer handles (portable-pty)
      io_threads.rs   # interruptible stdin forwarding + pty output forwarding
      ipc.rs          # Unix socket accept loop + protocol
      state.rs        # crash classifier + ring buffer + state machine
      cwd.rs          # CWD resolution logic
      resize.rs       # ResizeWatcher trait + unix/windows impls
    start.rs          # thin wrapper: resolve opts, call supervisor::run
    main.rs           # unchanged CLI surface
  Cargo.toml          # + portable-pty, signal-hook (unix), windows-sys (windows)
  specs/
    supervisor.md     # this file
```
