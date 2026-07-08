# Plan — supervisor crash resilience: fix the crash, auto-restart, auto-spawn

**Pending:** `#supresilience`
**Status:** A+B+C IMPLEMENTED, VERIFIED, and INSTALLED live 2026-07-02. Work is on
submodule branch `supervisor-crash-resilience` (commit `ef5063c6`, forked from HEAD
`5dc7c322`), built + `make install`ed from a worktree to bypass the concurrent
session's churning main tree. Backoff = 3 restarts / 5-min window. panic=abort
isolation kept separate (`#cdylibpanic`).

**Follow-up (not yet done):** merge branch `supervisor-crash-resilience` into the
submodule main once the concurrent crate-extraction refactor lands (rebase/merge,
resolve against the settled crate boundaries), then version bump + `VERSIONS.md` +
publish per the normal release process. The installed `~/.cargo/bin` binary is live
now; running controllers/supervisors were marked to recycle onto it at their idle
boundary (`lib-install` auto-recycle: 2 controllers + 4 supervisors).

## Problem (diagnosed 2026-07-02)

The **route-owned supervisor** (`agent-doc start --route-owned <FILE>`, the process
that hosts the agent TUI in a tmux pane and performs response writes) crashes and
is never autonomously restarted. Evidence: 5 `agent-doc` SIGABRT coredumps across
2026-07-01/02, **all** the same class:

```
thread 'main' panicked at std/src/io/stdio.rs:1165:9:
failed printing to stdout/stderr: Broken pipe (os error 32)
thread panicked while processing panic. aborting.
```

Rust's default `println!`/`eprintln!` panics on EPIPE. When the supervisor's pane
/ terminal goes away (e.g. when the IDE crashed and tore down its panes), the next
print panics, and the panic handler's own stderr print panics again → double-fault
→ SIGABRT. Under `panic = "abort"` (`Cargo.toml:281`, inherited by `release-local`)
with **no panic hook and no `catch_unwind` anywhere in the tree**, the process dies
instantly, leaving a stale `controller.sock`/`controller-handoff-*.sock` and an
orphaned pane (kept visible by `enable_remain_on_exit`). Nothing restarts it.

Architecture (verified):
- **Project controller** (`controller serve`, detached daemon) self-heals lazily —
  a dead controller is relaunched on the next `connect_or_launch` client demand
  (`project_controller/rpc.rs:1730-1837`).
- **Route-owned supervisor** does NOT self-heal. Watchdog machinery
  (`heartbeat.rs`, `session_liveness.rs`, `gc-io`) only detects/reaps records. The
  one revive path — `drive_supervisor_replacement_background` →
  `cold_start_supervisor_replacement` (`rpc.rs:4280-4378,4528`) — has a single
  caller, `session_actor_cmd::restart` (`rpc.rs:392`), reachable only via operator
  commands. The passive route fallback refuses after a crash window
  (`route.rs:1435-1443`).
- JB is a thin CLI wrapper. `Run Agent Doc` = `SubmitAction` →
  `agent-doc route --dispatch-only`, which cold-starts via `route::auto_start`. The
  `session *` action family (status/clear/stop-agent/cancel-turn/doctor) fails
  closed when the supervisor is dead instead of ensuring one.

## Part A — Fix the crash cause (broken-pipe double-fault) — IMPLEMENTED 2026-07-02

Goal: a broken output pipe must never abort the supervisor.

Implemented in `src/crash_resilience.rs` (new module), wired into `main.rs` as the
first line of `main()` via `crash_resilience::install()`.

- [x] **A1 — panic hook**: installed early in `main()`; writes the panic
      thread/location/message to `.agent-doc/logs/panic.log` (best-effort) and to
      stderr with an error-*ignoring* `write_all`, so the hook can never re-panic
      on a broken stderr (this is the double-fault that produced SIGABRT).
- [x] **A2 — SIGPIPE reset**: `libc::signal(SIGPIPE, SIG_DFL)` at startup
      (`#[cfg(unix)]`). A broken output pipe now terminates the process cleanly via
      the default SIGPIPE action instead of an EPIPE panic.
- [x] **A3 — tests + behavioral proof**: 3 unit tests (payload/logs-dir/file-append)
      pass in isolation. Behavioral proof via a scratch binary printing into a
      closed pipe: **without** the fix → crash (exit 101 panic / 134 SIGABRT under
      `panic=abort`); **with** the fix → clean exit 141 (SIGPIPE). No abort.

**Verification status:** module compiles + tests green in isolation; full-binary
green build + `make install` is BLOCKED by the concurrent session's in-progress
crate-extraction refactor, which currently leaves the submodule non-compiling
(cyclic dep in `agent-doc-supervisor-io`; unresolved `agent_doc_supervisor*` in
`agent-doc-session-registry-io`). Part A is staged in the working tree
(`src/crash_resilience.rs`, `src/main.rs`), ready to build + install the moment the
tree compiles cleanly. Isolated `/tmp` worktree verification does not work for this
submodule (relative `../tmux-router` / `../.cargo/config.toml` path deps).

Note: Part A neutralizes the *actual* recurring crash without touching the
`panic = "abort"` profile. The broader "any cdylib/thread panic aborts the host"
hardening is the separate [`plan-cdylib-panic-isolation.md`](plan-cdylib-panic-isolation.md) (`#cdylibpanic`).

## Part B — Auto-restart a crashed supervisor (watchdog) — IMPLEMENTED 2026-07-02

> **Status:** implemented in a worktree forked from submodule HEAD `5dc7c322`
> (branch `supervisor-crash-resilience`, commit `ef5063c6`) to avoid the concurrent
> session's churning main working tree. Verified: `agent-doc-supervisor` 246 tests,
> `-supervisor-io` 73, `agent-doc-orchestration` 1483, bin 628 — all green.

Goal: the controller autonomously restarts a route-owned supervisor that died,
with backoff so a deterministically-crashing supervisor cannot spawn-storm.

**Implemented:**
- Watchdog tick in the controller serve idle loop (`project_controller/rpc.rs`,
  `controller_supervisor_watchdog_tick`), throttled (default 10s,
  `AGENT_DOC_SUPERVISOR_WATCHDOG_INTERVAL_SECS`), skipped on a recycling iteration.
- Reuses the operator replacement path (`handle_supervisor_replacement` →
  `cold_start_supervisor_replacement`) — no parallel spawn logic.
- Restarts only when ALL hold: recorded `supervisor_pid` dead (`process_is_alive`),
  pid ≠ controller (self-ancestor guard), actor state ≠ Closed, live tmux pane,
  supervisor session log open (crash-detect + in-flight dedup), within budget.
- Backoff = 3 restarts / rolling 5-min window per document (pure
  `crash_policy::watchdog_restart_decision`), sourced from the shared session-loss
  ledger; on exhaustion emits a once-per-document operator diagnostic.

- [ ] **B1 — health probe**: in the controller serve idle loop (`rpc.rs:2030-2113`),
      periodically probe each registered actor's `supervisor_pid`
      (`libc::kill(pid,0)`) + supervisor socket liveness.
- [ ] **B2 — autonomous revive**: when a supervisor is dead but its doc still has a
      live tmux pane / active (non-closed) session, invoke the EXISTING
      `drive_supervisor_replacement_background` → `cold_start_supervisor_replacement`
      path (no new spawn logic — reuse the operator path). Carry the self-ancestor
      guard.
- [ ] **B3 — backoff / storm guard** (pure policy in `agent-doc-supervisor`,
      unit-tested): exponential backoff, cap restarts per rolling window, and after
      the cap STOP and emit an operator-visible diagnostic instead of looping.
      Reuse / align with the existing `fail_if_recent_session_loss_window` accounting
      so the watchdog and the route fallback share one crash-window ledger. Part A
      must land with B so the common (broken-pipe) crash no longer recurs under the
      watchdog.

## Part C — Auto-spawn from JB actions that need a supervisor — IMPLEMENTED 2026-07-02

> **Status:** implemented (same branch/commit as Part B). Empirically scoped: of the
> `session *` family, only `session clear` genuinely fails closed needing a
> supervisor; `status`/`doctor` report the dead supervisor as state/issues and
> return Ok (left unwrapped); `stop-agent`/`cancel-turn` stay fail-closed.
> `Run Agent Doc` (`route --dispatch-only`) already cold-starts.
>
> `session clear` now escalates on a real supervisor-missing refusal
> (`session_error_is_missing_supervisor` matches the verbatim production strings:
> "no live supervisor socket for", "failed to contact supervisor for", "supervisor
> does not support clear IPC and no live pane is available") through the existing
> `recycle_should_escalate_dead_supervisor` gate + `session_actor_cmd::restart`
> (continue mode), waits for the socket, retries once. It deliberately does NOT match
> the controller-connect timeout (escalation needs the controller too, so a retry
> can't help) nor the turn-scoped no-op reports. Test: classifier vs verbatim real
> strings (3 positive, 3 negative).

Goal: any action that requires a supervisor ensures one exists instead of failing
closed. Keep the logic in the binary (JB stays thin).

- [ ] **C1**: confirm `Run Agent Doc` (route `auto_start`) cold-starts; keep.
- [ ] **C2 — ensure-supervisor preflight** for the `session *` family: when the
      command finds no live supervisor, escalate through the existing
      `admin recycle <FILE>` dead→cold-start path (`recycle_should_escalate_dead_supervisor`,
      `main.rs:695`) and retry once, rather than returning the fail-closed refusal.
      Implement CLI-side so all harnesses benefit.
- [ ] **C3 — genuine no-op guard**: actions that require a *live agent turn*
      (`stop-agent`, `cancel-turn`) may still fail closed when there is literally no
      agent to act on — ensure-supervisor should cold-start the supervisor but not
      fabricate a turn. Decide per-command (see Decision 4).

## Verification

- [ ] Full `cargo test` + the new deterministic tests (broken-pipe, backoff policy,
      ensure-supervisor).
- [ ] tmux/SimWorld coverage for the watchdog restart where modelable.
- [ ] `make install-full` + supervisor recycle to take effect; coordinate around the
      live concurrent agent-doc session in the submodule.

## Open decisions (need operator input)

1. **Restart aggressiveness (B3):** proposed exponential backoff, max ~3 restarts
   per 5-min window, then stop + notify. OK, or different caps?
2. **Watchdog home (B):** project controller (recommended — already the self-healing
   daemon) vs a new dedicated watchdog. Recommend controller.
3. **panic=abort:** keep it and rely on Part A + hook (recommended), or also do the
   `#cdylibpanic` profile/`catch_unwind` isolation in the same effort?
4. **Auto-spawn scope (C):** all `session *` commands ensure-and-proceed, or only
   the read/repair ones (status/doctor/clear) while stop-agent/cancel-turn stay
   fail-closed when no agent is live?

## Exit criteria

- A broken output pipe (pane/terminal closed) never SIGABRTs the supervisor.
- A crashed route-owned supervisor with a live session is autonomously restarted,
  with backoff that refuses to spawn-storm a persistently-crashing build.
- JB actions that need a supervisor cold-start it instead of failing closed.
