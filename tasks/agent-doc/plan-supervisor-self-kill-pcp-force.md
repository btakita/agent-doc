# Plan: supervisor self-kill + PCP-driven timeout force-kill (#supkill)

## Question

Can the `start --route-owned` **supervisor** have a self-kill command, and can the
**PCP** (project controller) drive that self-kill with a timeout that escalates to a
force-kill? Answer: **yes to both** — and the PCP-driven timeout force-kill is the
*correct* design precisely because the analogous controller self-kill (M1
`controller_self_watchdog_suicide`) is **dead in production** (never fires at
runtime — a truly wedged process can't be trusted to self-reap). External
enforcement by a separate process is the robust backstop.

## Architecture facts (grounded)

- **Supervisor** = `agent-doc start --route-owned <FILE>` — the PARENT of the harness
  child: `tmux → zsh → agent-doc(sup) → claude(agent) → shell`. A naive supervisor
  `exit()` orphans the live agent to init (agent keeps running, loses routing); a
  `kill` of the supervisor PID from *inside* the agent session kills the session.
- **PCP** = `agent-doc controller serve` — a SEPARATE process tree from the supervisor
  (not its child). It already tracks supervisor identity (`bootstrap.pid`,
  heartbeats, `controller_binary`) and already force-kills wedged *controller* procs
  by verified `/proc` cmdline (`reap-stale-controllers`, M3/M5,
  `orphaned_preparing_controller_reaped_cross_project caller=gc`, "never self").
- **Existing controller primitives to mirror for the supervisor:**
  - `controller_self_watchdog_should_suicide` / `controller_self_watchdog_suicide`
    (`rpc.rs:1745`/`:1764`) — idle-loop polls, flips `should_stop`, exits. **Observed
    dead at runtime** (M1) — the wedged proc never polls.
  - `controller_wants_recycle` / `controller_recycle_idle` / `controller_self_recycle`
    (`rpc.rs:1798`–`1852`) — recycle request flag + idle proof + graceful exit.
  - Supervisor R3 execve recycle (`start.rs`, `ReexecState`, `PtySession::adopt`),
    gated `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE=1` — already an idle-gated self-action
    on the supervisor; a self-kill is strictly simpler than re-exec.

## Design

Two rungs, mirroring controller machinery onto the supervisor:

### Rung A — supervisor self-kill (graceful, idle-gated)
- Add a `supervisor self-kill` request the supervisor's idle loop checks each tick
  (mirror `controller_wants_recycle`): set via an IPC RPC (`admin recycle` already
  reaches controllers; extend to mark the supervisor) and/or `SIGTERM`.
- Idle gate (mirror `controller_recycle_idle`): self-kill only when no dispatch /
  turn is in flight for the owned doc, so a *healthy* supervisor never tears down a
  live turn. Fail-closed on idle uncertainty.
- On fire: choose hand-off vs teardown. Default = **clean teardown** (exit, agent
  re-routes on next bind) since the operator use case is "this session is wedged,
  replace it". (Hand-off/re-adopt is the R3 execve path; out of scope for kill.)
- Log `supervisor_self_killed pid=… generation=… reason=… caller=self`.

### Rung B — PCP-driven timeout force-kill (external backstop, the real fix)
- The PCP, on `recycle`/`reap` for a doc whose supervisor is stale/wedged:
  1. Send the graceful self-kill request (Rung A) + record `requested_at`.
  2. Wait `AGENT_DOC_SUPERVISOR_SELFKILL_GRACE_SECS` (default 10s).
  3. If the supervisor PID is still alive with a stale heartbeat, **force-kill by
     verified `/proc` cmdline** (`start --route-owned <FILE>`, never self) — reuse
     the M3/M5 reaper's verify+kill primitive. SIGTERM → short grace → SIGKILL.
  4. Log `supervisor_force_killed pid=… age_secs=… grace_secs=… caller=pcp`.
- This does NOT depend on the wedged supervisor cooperating — the exact failure mode
  that makes M1 useless. The PCP is the supervisor-of-the-supervisor.

### Constraint to decide
- **In-session safety:** an operator triggering this from inside the doomed session
  must not kill their own shell first. The PCP path is safe (separate tree). The
  in-session `admin` command must dispatch the request to the PCP and return, NOT
  signal the supervisor PID directly (which is the caller's ancestor).

## Rungs / IDs
- `#supkill-a` — supervisor graceful self-kill (idle-gated request + idle proof + exit).
- `#supkill-b` — PCP timeout force-kill backstop (grace timer + verified `/proc` kill).
- `#supkill-test` — SimWorld coverage: (1) graceful self-kill on idle, (2) force-kill
  fires when self-kill is suppressed (simulate the wedged/non-polling supervisor),
  (3) in-session caller never signals its own ancestor PID.

## Operator refinement — blue/green (zero-downtime) restart model (#supkill-bg)

Operator (2026-06-14): "treat the restart like a no-downtime server deployment —
spawn the new version, the old version self-kills as it finishes its last job, the
PCP routes requests to the new process, and new jobs racing to the old process fail
with instructions to route to the new process."

**This is the correct framing and it is more reliable than the kill-first model** —
it is the direct fix for the failure that motivated this plan: `session_restart`
was *rejected* (`generation 354 is closed receipt_id=3429`) because the current
restart tries to mutate a closed actor generation and fails closed. Blue/green never
needs the old supervisor to accept a kill; it spawns a new generation and lets the
old one drain.

### Why the substrate already exists
- The PCP **is the router** and already owns the generation/authorization layer:
  `authorize_operator_command` / `handle_operator_command`
  (`project_controller/rpc.rs:835`/`:3160`). The `generation N is closed` error is
  the PCP *already* rejecting work aimed at a superseded actor — that rejection IS
  the "route to the new process" signal the operator describes, it just currently
  reads as a hard failure instead of a redirect.
- Generation bump + close-old is how the actor store already supersedes records;
  blue/green is "allocate a new generation, mark the old `draining`, route new
  dispatch to the new generation, let the old finish its in-flight turn."

### How it folds into the two rungs (not a replacement)
- Rung A (graceful self-kill, idle-gated) **becomes the drain trigger**: the old
  supervisor self-kills *after* its last in-flight turn completes (idle proof),
  rather than tearing down a live turn. Same idle gate, framed as "drain then exit."
- Rung B (PCP force-kill backstop) **stays mandatory and unchanged**: blue/green
  assumes the old supervisor *cooperates with draining*. A wedged/non-polling
  supervisor (the M1-dead failure mode) will never drain or self-exit — the PCP
  grace-timeout `/proc` force-kill is the only thing that reclaims it. Blue/green is
  the healthy path; force-kill is the unhealthy-path floor.

### Caveat — the supervisor owns a PTY/pane, not a network port
A stateless HTTP blue/green flips a load balancer between two interchangeable
listeners. The supervisor is the **parent of the live harness child**
(`tmux → zsh → agent-doc(sup) → claude(agent)`), so "the new process serves traffic"
means one of:
- **(a) In-place re-exec (preferred for binary upgrade):** the existing R3 execve
  recycle (`ReexecState`, `PtySession::adopt`, gated `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE=1`)
  already swaps the binary in the same process slot and re-adopts the child — this is
  the truest zero-downtime path for the stale-build case (#fcc0) and keeps the same
  pane. Make it the default restart action when the supervisor is *healthy*.
- **(b) New pane + reroute (true two-process blue/green):** new supervisor takes a
  fresh pane, PCP repoints the route, old drains and exits. Heavier (pane churn,
  brief two-pane window) but required when in-place adopt is unsafe.

Recommendation: default healthy restart = in-place re-exec drain (a); keep
PCP force-kill (Rung B) as the floor; reserve two-process reroute (b) for cases
where adopt can't be trusted. Turn the `generation N is closed` rejection into a
structured "superseded → retry against generation N+1" redirect so racing dispatch
self-heals instead of erroring.

### Added IDs
- `#supkill-bg` — reframe restart as drain-and-supersede: new generation routed,
  old generation `draining` then idle-exit (Rung A), `generation closed` becomes a
  retryable redirect rather than a hard reject.

## Verification
- `make check` + `make tmux-ci`; deterministic SimWorld for the wedged-supervisor
  (non-cooperating) force-kill path — model the M1-dead failure mode so this can't
  regress to "self-kill present but never fires".
- SimWorld for blue/green: (1) healthy drain — old supervisor finishes its in-flight
  turn then exits, new generation serves the next dispatch with no dropped turn;
  (2) racing dispatch to the old (closed) generation receives a redirect and retries
  against the new generation instead of failing.
