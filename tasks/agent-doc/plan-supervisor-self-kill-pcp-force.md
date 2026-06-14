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

## Verification
- `make check` + `make tmux-ci`; deterministic SimWorld for the wedged-supervisor
  (non-cooperating) force-kill path — model the M1-dead failure mode so this can't
  regress to "self-kill present but never fires".
