# Plan: stale busy actor after `auto_trigger_inject` + no recovery without a pane kill

`#stale-busy-after-auto-inject-no-clear` · `#admin-recover-stale-busy-no-pane-kill`

## Symptom (live, 2026-06-06)

`src/boost-client/tasks/monsterrodholders.md` is "truly stuck": every dispatch is
refused as busy, even on a fresh session, and a tmux pane kill + restart re-enters
the same state. User: *"We should be able to recover without having to kill the
tmux pane. Better yet, never get into this state."*

## Evidence gathered this cycle

Live pane `%29` (Codex, gpt-5.5) is **idle at the composer** — footer shows
`Worked for 3m 05s` and the last output is the recursion-guard message. The turn
finished. But the actor is wedged busy:

- `agent-doc admin list` → `pane=%29 (alive) gen=292 busy`.
- `admin detect` → **no anomalies** (pane is alive, no cross-doc contention) — it
  has no finding for "busy actor over an idle composer footer" in the
  *supervisor-in-memory* case.
- Cycle `cycle-1780782223209` is `phase: "abandoned"`,
  `last_event: recursive_direct_invocation_blocked`, and that event recorded
  `actor_state=ready` at block time (ts 1780782223).
- `session-actors.json` `last_transition`:
  `caller=dispatch reason=auto_trigger_inject timestamp=1780782316`
  (**93s after** the recursion-abandon) flipped the actor to `busy`.

### Root cause

The auto-queue injected a prompt (`reason=auto_trigger_inject`), which set the
actor `busy`. The injected Codex turn **completed** (pane idle at composer), but
**no busy→ready completion transition ever fired**, so the actor stays busy
indefinitely. The injected prompt was the recursive `agent-doc <FILE>` self-shell
into the owning pane; it hit the recursion guard and abandoned the cycle, but the
abandon path did not restore the actor that the inject had marked busy.

### Recovery DOES exist (corrected) — it was just undiscoverable

A non-destructive recovery already exists: **`agent-doc session status <FILE>`**
(and `session clear` / `session doctor --repair`) calls
`reconcile_idle_projection_from_live_pane` (`src/session_actor_cmd.rs`), which
repairs a stale busy actor back to `ready` from idle pane evidence — **no pane
kill**. Verified live this cycle on the stuck `monsterrodholders.md`:

```
$ agent-doc session status tasks/monsterrodholders.md
actor: generation=292 ... state=ready
actor_last_transition: caller=session_operator reason=live_pane_idle ...
live_pane: state=alive-idle ... prompt_ready=true
# admin list before: gen=292 busy  → after: gen=292 ready
```

The actor store is a **SQLite state DB** (`open_state_db`); `session-actors.json`
is only a re-emitted *projection* — that is why a direct file edit had no effect
(`admin list` reads the DB). Commands that don't help: `repair` (no pending
response → no-op), `cancel` (the cycle is already terminal), `admin` (read-only),
`reset` (would orphan the live Codex session).

So the real defects are **discoverability** and **auto-application**, not a missing
command.

#### Defect 1 — discoverability (FIXED, `be7f5f06`)

The recursion-block diagnostics (`run.rs` no-change verdict, `session_check.rs`
INTERRUPTED) only pointed at the heavy `agent-doc start <FILE>` restart, never at
`session status` / `session clear`. Operators therefore reached for kill+restart.
Fixed: both diagnostics now surface the idle-reconcile recovery (no pane kill)
ahead of restart, with a regression test.

#### Defect 2 — auto-reconcile gap (still open — the durable "never get stuck")

The supervisor idle-queue watch ran (`idle_queue_watch_drain` in the session log)
but did **not** auto-reconcile the idle pane's stale busy actor → ready. If it
did, the document would self-heal the moment the pane returned to idle and never
present as "truly stuck." `reconcile_idle_projection_from_live_pane` lives in the
main crate (`src/session_actor_cmd.rs`) while the watch lives in
`agent-doc-orchestration::start::spawn_idle_queue_watch_thread`, so wiring it in
requires sharing the reconcile logic across the crate boundary — a focused,
test-backed change in sensitive supervisor code.

#### Note — supervisor lease lag

After the actor reconcile, `session status` still reported
`controller_lease ... runtime_state=busy`. The actor (what dispatch checks) is
`ready`, but the supervisor lease runtime_state lagged. Confirm the lease also
clears so the doc cannot re-wedge from a stale lease.

## Fix directions

### Phase 1 — `#stale-busy-after-auto-inject-no-clear` (never get into the state)

1. The actor state machine must transition `busy → ready` when an
   `auto_trigger_inject` (or any dispatch-injected) turn **terminates** — whether
   it completes normally, errors, or is **abandoned by the recursion guard**. The
   recursion-abandon path in particular must restore the actor it (or the inject)
   marked busy. Today the abandon is terminal for the *cycle* but leaks the *actor*
   busy flag.
2. A cold supervisor restart must re-derive actor state from a live pane probe
   (idle composer footer ⇒ ready) instead of trusting a persisted `busy`.
3. Deterministic SimWorld coverage: inject → turn-complete-without-Stop-signal →
   assert actor returns ready; recursive-abandon → assert actor not left busy.

### Phase 2 — discoverability (DONE, `be7f5f06`) + detect finding (optional)

1. DONE: the recursion-block diagnostics now surface `agent-doc session status
   <FILE>` / `session clear <FILE>` (idle-reconcile, no pane kill) ahead of the
   heavy restart, with a regression test.
2. (Optional) Extend `admin detect` with a `busy_actor_idle_pane` finding (actor
   busy in the DB while the live pane footer is idle) so the dashboard surfaces
   the wedge and names the `session status` recovery. Note: a new `admin recover`
   command is NOT needed — `session status`/`clear` already reconciles via the
   SQLite store (the supervisor reads the same DB and won't clobber a wedged
   actor it isn't transitioning).

### Phase 3 (design, references existing items)

`agent-doc <FILE>` from the owning pane currently abandons a cycle and can leak the
busy flag. The durable design is **pass-back** (treat owner-pane self-invocation as
a no-op success that nudges the already-running session) or **allow recursion**,
rather than deadlock-guard-then-abandon. This overlaps the Option-A MCP server
(`#codex-mcp-server`, `tasks/agent-doc/plans/codex-mcp-server-option-a.md`) and the
interim `#codex-self-reinvoke-prevent`; track the pass-back facet there.

## Related

`#jb-stale-busy-idle-footer` (live-pane probe misclassification),
`#codex-owned-pane-auto-queue-stuck`, `#jb-restart-supervisor-busy-refusal`,
`#recover-ownership-command` (`#ec9k`), `#codex-mcp-server`.
