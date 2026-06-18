# Plan: `#supselfheal` — stale-binary route-owned supervisor has no non-disruptive self-heal

**Status:** design / not started
**Filed:** 2026-06-18 (boost-client/monsterrodholders.md dogfooding session)
**Builds on:** `#proactive-recycle-on-install` (R3 supervisor idle self-recycle — landed but
default-OFF), `plan-pcp-recycle-no-restart-sync-block.md` (Q1 re-exec ENOENT → kill+relaunch
fallback), `plan-supervisor-self-kill-pcp-force.md` (PCP force-kill backstop).
**Sibling of:** `#fcc0`/`#ipcdrift`, `#semmerge` (the fail-closed-refuse write policy this wedge
rides on).

## Problem (firsthand repro this session, agent-doc 0.34.4)

The route-owned host supervisor (`pid 3218321`) serving `tasks/monsterrodholders.md` was mapping a
**stale agent-doc binary** (older than the installed 0.34.4). That wedged every editor IPC write:

```
[write] socket IPC failed: IPC ack timeout (6000ms) — falling back to file IPC
[write] IPC socket degraded ... falling back to file-IPC patch queue (no automatic disk fallback)
[write] IPC timeout (2s) — leaving patch for editor retry; refusing direct document write
[write] IPC proof insufficient ... source=file_ipc invariant=no_ack recovery=retry_without_disk_write
Error: apply_template: refused direct disk write ... while editor IPC listener is active (reason=send_failed)
```

Every `finalize` / `write --commit` was refused; an orphaned 2398-byte response could not land. The
session could not close out, across **four** invocations. None of the documented recoveries worked
non-disruptively:

1. **`agent-doc admin recycle` does not reach the supervisor.** It only marks **controllers** to
   recycle at the next idle boundary (`main.rs` → `recycle_controller` → controller `recycle` RPC sets
   `recycle_requested`). It sends nothing to the `start --route-owned` supervisor process. So it cannot
   clear a stale-**supervisor** wedge. There is **no CLI command to recycle a supervisor.**
2. **Supervisor idle self-recycle is default-OFF.** The code exists
   (`start/idle_watch.rs` → `supervisor_recycle_action` → `supervisor_perform_reexec` via
   `execve_preserve_child`, logging `supervisor_binary_stale_self_recycled`), but
   `supervisor_recycle_action(stale, auto_recycle=false, ...)` returns `Detect` — log
   `supervisor_binary_stale_detected` once and **keep running the stale binary**. It only re-execs when
   `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE=1` (`resolve_supervisor_auto_recycle`), which is off here.
3. **Even enabled, the trigger is idle-boundary only.** It fires at a turn/idle boundary. When the
   queue is paused/wedged and the agent keeps retrying, that boundary may never arrive — the **wedge
   itself is never used as a recycle trigger**, although it is the clearest possible signal that the
   live binary is bad.
4. **Re-exec failure has no escalation.** On `current_exe()` ENOENT (inode deleted by a fresh
   `make install`) or other syscall error, `supervisor_perform_reexec` falls back to
   `supervisor_reexec_failed fallback=continue_current_binary` — i.e. it keeps the stale binary and just
   re-logs every tick. The kill+relaunch fallback from `plan-pcp-recycle-no-restart-sync-block.md` Q1 is
   not wired to this path.
5. **The closeout guidance is false for this case.** SKILL.md / `queue_continuation_guidance` assert "a
   stale/wedged supervisor is the supervisor's own self-heal / recycle concern (it resumes on its own
   when the short-TTL drain lease expires)." For a stale **binary** that claim does not hold — nothing
   automatically replaces the supervisor binary, so the wedge is indefinite. The only working escapes
   are operator-disruptive (`session restart-supervisor`, refuses busy panes / `--force` disrupts the
   live JetBrains editor) or rule-violating (`--force-disk`, risks clobbering the live buffer).

Net: a stale-binary route-owned supervisor is an **indefinite, operator-only wedge**, and the agent
correctly refuses every non-disruptive automated escape, so the session cannot close.

## Fix

Make a stale-binary route-owned supervisor converge to the fresh binary **without operator
disruption**, and make the closeout path stop asserting a self-heal that does not occur.

### Required Contract (architecture-first)

- **Invariant:** A route-owned supervisor detected running a stale binary must either (a) self-refresh
  to the installed binary within a bounded number of cycles via a non-disruptive trigger, or (b) when it
  provably cannot (re-exec fails), escalate through one deterministic, bounded recovery — never sit in an
  indefinite wedge while the closeout path simultaneously refuses every automated escape and claims it
  will self-heal.
- **Policy owner:** supervisor recycle policy stays in `start/decisions.rs::supervisor_recycle_action`
  (extend it; do not add parallel deciders). `start/idle_watch.rs` and the **write/converge closeout
  path** are adapters that feed it typed evidence (`stale`, `auto_recycle`, `turn_boundary`,
  `head_pending`, **new:** `write_wedged`) and apply the returned action. `admin recycle`
  (`project_controller/rpc.rs`) gains a supervisor-directed branch that routes through the same owner.
- **Transition table** (extend `supervisor_recycle_action`):

  | stale | auto_recycle | trigger | re-exec result | decision |
  |-------|--------------|---------|----------------|----------|
  | false | * | * | — | `None` |
  | true | true | turn_boundary + head_pending | ok | `RecycleImmediate` |
  | true | true | turn_boundary | ok | `RecycleDebounced` |
  | true | true | **write_wedged** (new) | ok | `RecycleImmediate` (wedge ⇒ don't wait for idle) |
  | true | * | any | **re-exec fails (ENOENT/syscall)** | `EscalateKillRelaunch` (new; bounded, ≤N attempts) |
  | true | false | any | — | `Detect` **only if non-wedged**; a `write_wedged` stale supervisor must NOT stay `Detect` |
  | true | n/a | `admin recycle` (operator/agent explicit) | ok | `RecycleImmediate` (explicit intent overrides default-OFF) |

- **Evidence inputs:** `process_binary_is_stale(recycle_launch_identity)` (exists); a new
  `write_wedged` fact derived in the converge path from repeated `send_failed`/`no_ack`/
  `retry_without_disk_write` against an editor whose listener is nominally active; the resolved
  `auto_recycle` policy; re-exec outcome (`ok` / `ENOENT` / `other`). No error-string matching in the
  owner — adapters classify into these typed inputs.
- **Allowed edit surfaces:** `start/decisions.rs` (extend the decision + add
  `EscalateKillRelaunch`/`write_wedged` rows), `start/idle_watch.rs` (feed the new trigger; wire the
  kill+relaunch escalation already designed in `plan-pcp-recycle-no-restart-sync-block.md` Q1),
  `write/converge.rs` (emit the `write_wedged` evidence + request a supervisor recycle through the owner
  instead of looping refusals), `project_controller/rpc.rs` (`admin recycle` supervisor branch +
  `resolve_supervisor_auto_recycle` default decision), and the SKILL.md / `queue_continuation_guidance`
  text. Do **not** add a second recycle decider or a CLI that bypasses the owner.
- **Verification:** pure decision-table tests for every new row in `supervisor_recycle_action`
  (write_wedged ⇒ immediate, re-exec-fail ⇒ escalate, explicit admin recycle ⇒ override default-OFF,
  non-stale ⇒ none). A SimWorld/integration test reproducing this session: stale-binary supervisor +
  wedged editor write ⇒ within a bounded number of cycles the supervisor re-execs (or kill-relaunches on
  forced re-exec failure) and the previously-orphaned response commits with `session-check` OK — **no**
  operator `restart-supervisor` and **no** `--force-disk`. A test asserting `admin recycle` clears a
  stale **supervisor** (not just controllers).
- **Out of scope:** the broader fail-closed write-merge policy (that is `#semmerge`); controller recycle
  semantics (unchanged); the `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` global default flip for the
  non-wedged idle case (track under `#proactive-recycle-on-install` decision #4) — this plan only forces
  recycle for the **wedged** stale-binary case + explicit `admin recycle`, which is safe regardless of
  that global default.

## Phases (one backlog id per actionable phase)

1. **`#supselfheal-adminrecycle`** — `agent-doc admin recycle` also recycles the route-owned
   supervisor (new supervisor branch routed through `supervisor_recycle_action` with an explicit-intent
   override of default-OFF). Gives the agent/operator a non-disruptive supervisor-recycle trigger that
   does **not** refuse busy panes. Smallest, highest-leverage fix — would have cleared this session.
2. **`#supselfheal-wedgetrigger`** — derive a typed `write_wedged` fact in `write/converge.rs` from
   repeated `send_failed`/`no_ack` against a nominally-active listener; when `write_wedged && stale`,
   request a supervisor recycle through the owner instead of looping refusals. Wedge becomes a recycle
   trigger (no waiting for an idle boundary that never comes).
3. **`#supselfheal-reexecescalate`** — wire `EscalateKillRelaunch` (bounded ≤N) for
   `supervisor_perform_reexec` ENOENT/syscall failure, reusing the kill+relaunch fallback from
   `plan-pcp-recycle-no-restart-sync-block.md` Q1, instead of `continue_current_binary` forever.
4. **`#supselfheal-guidance`** — correct the SKILL.md / `queue_continuation_guidance` claim: a stale
   **binary** supervisor does not self-heal via drain-lease expiry; document the new
   wedge-triggered/admin-recycle recovery as the actual non-disruptive path so the agent stops asserting
   a self-heal that will not happen.
5. **`#supselfheal-simworld`** — SimWorld coverage for the stale-binary-wedge → self-recycle/escalate →
   orphaned-response-commits sequence, reproducing this session deterministically.

## Quick-win stopgap (independent of the rewrite)

Land phase 1 (`#supselfheal-adminrecycle`) first: today `admin recycle` is the documented gentle fix the
session-check itself recommends, yet it provably cannot clear a stale supervisor. Making it actually
recycle the supervisor is a small, contained change that removes the "no non-disruptive escape" dead end
on its own.

## Bugs this subsumes / overlaps

`#fcc0`/`#ipcdrift` (the stale-binary wedge symptom), the false self-heal guidance, and the
`reset --from-current` / `restart-supervisor` / `--force-disk` manual-recovery dance. Depends on the
landed `#proactive-recycle-on-install` machinery and the `plan-pcp-recycle-no-restart-sync-block.md` Q1
kill+relaunch fallback. Distinct from `#semmerge` (write-merge policy) — this is supervisor lifecycle.
