# Plan: JB `Run Agent Doc` should auto-restart a stale/churn-stopped supervisor

**Backlog id:** `#jbrestale`
**Class:** clean-session (touches the route/dispatch + supervisor-recycle path)
**Related:** `#recyclerestart` (recycle does not restart the agent session / sync-lock coupling),
`#aazp` / `#4myd` (suprecyclestall self-recycle — already CODE-COMPLETE, operator-verify),
`#qchurn` (go-mode no-op churn that paused this queue), `#fcc0` / `#ipcdrift` (stale-supervisor root).

## Progress

- **2026-06-13 — seam-isolated classifier landed** (agent-doc submodule `9d43172b`, on top of
  `ce339055`). Added two pure, unit-tested helpers in `project_controller/rpc.rs`, **not yet wired**
  into the dispatch bail (`#[allow(dead_code)]`):
  - `pause_reason_is_stale_supervisor_churn_stop(reason)` — classifies a stored `queue_paused`
    reason as a stale-supervisor churn-stop vs a deliberate operator/spent-preset pause or a
    genuinely-wedged queue.
  - `stale_supervisor_pid_from_pause_reason(reason)` — best-effort PID extraction for the
    `route_dispatch_recovery … stale_pid=<pid>` proof line.
  Tests `pause_reason_stale_supervisor_churn_stop_classification` + `stale_supervisor_pid_extraction`;
  `make check` green. Matches this repo's seam-isolation discipline (land the primitive + tests,
  do not re-wire the live dispatch hot path without operator-go on a clean baseline).
- **Remaining (clean session):** Fix steps 1–4 below — wire the classifier into the `queue_paused`
  bail at `rpc.rs` ~2448 (confirm live staleness, branch to restart-supervisor, lift the pause,
  re-dispatch once, fail closed + keep pane alive on `reexec_failed`), plus the three SimWorld cases.
  Needs a clean session because the wiring modifies the live dispatch path the churning stale
  supervisor is actively running, and the final verification is operator-gated.

## Operator report (2026-06-13)

Operator invoked JB `Run Agent Doc` expecting it to restart the supervisor. Instead the dispatch
failed closed:

```
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for
tasks/agent-doc/agent-doc-bugs2.md: failed_stage=queue_paused reason=churn-stop:
do[#c2b6] operator-verify head re-injected by stale supervisor pid1368698 (pre-0.34.0);
needs operator recycle, not agent drain receipt_id=3420
```

The churn-stop reason text itself prescribes the remedy ("needs operator recycle"), but the
dispatch path surfaces it as a terminal error and takes no recovery action. The operator must then
manually run `agent-doc session restart-supervisor <FILE> --force`.

## Root cause

Two coupled facts:

1. **A stale supervisor re-injects an operator-verify head**, tripping the churn detector, which
   PAUSES the queue (`failed_stage=queue_paused reason=churn-stop`). This is the same stale-binary
   root as `#fcc0`/`#ipcdrift` — the supervisor predates the installed binary
   (`supervisor_binary_stale`).
2. **The JB `Run Agent Doc` dispatch path treats `queue_paused`+`churn-stop` as terminal.** When the
   churn-stop cause is specifically a *stale supervisor* (`pre-<ver>` / `supervisor_binary_stale`),
   the dispatch knows the fix is a recycle — yet returns the error instead of performing it.

So the self-recycle machinery (`supervisor_binary_stale_self_recycled`, landed for `#aazp`/`#4myd`)
exists, but it is gated on the **idle / next-queue-item** boundary, not reachable from a
**dispatch-blocked-by-churn-stop** entry. A churn-stopped queue never reaches the idle boundary that
would trigger self-recycle, so it wedges until the operator recycles by hand.

## Fix

Make the JB `Run Agent Doc` dispatch path recover from a stale-supervisor churn-stop instead of
failing closed:

1. **Classify the churn-stop cause.** In the dispatch handler, when `failed_stage=queue_paused` and
   the churn-stop `reason` carries a stale-supervisor signature (`stale supervisor pid…`,
   `pre-<ver>`, or the `supervisor_binary_stale` discriminator), branch to recovery rather than
   returning the error.
2. **Auto-restart the supervisor once.** Trigger the same recycle path as
   `session restart-supervisor --force` (in-process supervisor host → re-exec the current installed
   binary; the `supervisor_binary_stale_self_recycled` path). Emit a proof line, e.g.
   `route_dispatch_recovery action=restart_supervisor cause=churn_stop_stale_supervisor
   stale_pid=<pid> result=restarted|reexec_failed`.
3. **Clear the churn-stop and re-dispatch once.** After a successful restart, lift the
   `queue_paused`/`churn-stop` pause that was caused solely by the stale supervisor's re-injection,
   then retry the dispatch a single time. Bound to one attempt so a genuinely wedged queue still
   fails closed (no restart loop). On `reexec_failed`, keep the current fail-closed behavior but keep
   the pane alive and show the manual `session restart-supervisor --force` hint (same fallback shape
   as `#aazp`).
4. **Idempotent/operator-verify-safe.** Do NOT auto-drain operator-verify heads as a side effect —
   restart + clear the stale-injected churn pause only; operator-verify heads stay deferred
   (`#goqueuestall` semantics). The re-injected `do[#c2b6]` head should be re-deferred, not consumed.

## Proof / tests (SimWorld)

- New SimWorld case: dispatch into a `queue_paused`+`churn-stop` whose reason is a stale supervisor →
  assert one `route_dispatch_recovery action=restart_supervisor` proof line, the pause is lifted, and
  exactly one re-dispatch occurs (no loop).
- Negative case: `queue_paused` with a NON-stale-supervisor churn reason → still fails closed, no
  restart attempt.
- Fallback case: restart re-exec fails → `result=reexec_failed`, pane stays alive, manual hint
  printed, no hang.
- `make check` green; tmux-CI review for the route/dispatch surface.

## Operator-drive verification (cannot be agent-done)

With a freshly `make install`-ed agent-doc, deliberately leave a stale supervisor running, churn the
queue into `queue_paused`, then press JB `Run Agent Doc`. Confirm ops.log shows
`route_dispatch_recovery action=restart_supervisor … result=restarted` followed by a successful
re-dispatch — WITHOUT the operator manually running `session restart-supervisor`.

## Immediate operator workaround (this session)

The live supervisor (`pid 2825163`) is stale (predates the installed binary). Until `#jbrestale`
lands, clear the churn-stop by recycling manually:

```
agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force
```

(If `target/release/agent-doc` is also stale — preflight's `stale_install` warning — run
`make install` in `src/agent-doc` first so the restarted supervisor runs current code.)
