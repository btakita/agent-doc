# Plan: #fullboundary — convergence-gated inter-queue-item boundary + loud force-disk fallback

## Problem (root cause of this session's drift family)

Almost every failure dogfooding `agent-doc-bugs2.md` this session was a
*concurrent-access race* on the session document between three writers — the
`finalize` client, the live JetBrains editor buffer, and the supervisor — with
**no hard serialization between consuming queue item N and dispatching item
N+1**:

- `content_ours` / `live_prompt_drift_after_preflight` — finalize's snapshot
  races the editor buffer mid-write.
- `postcommit_worktree_check match=false` (`#pcwcdiskfree`) — the commit lands
  but the next cycle starts before the editor converged the buffer to HEAD.
- `inflight=5` → `pending_write … reason=send_failed` — the smoking gun: IPC
  patches **pile up** because the next dispatch never waits for the prior one's
  consumer ACK, so a transient JB-plugin hiccup compounds into a hard wedge that
  blocks every later write.

The drain-owner lease (`#kp5z`) serializes dispatch *ownership* but not editor
*convergence*; today's closeout even DETECTS non-convergence
(`postcommit_worktree_check match=false`) and proceeds anyway (commits, signals
the editor, moves on).

## Fix 1 — convergence-gated full boundary between queue items

Do not inject the next queue item's trigger until the prior turn proves a
**quiescent** close:

1. cycle reached `committed`, AND
2. the editor buffer is proven converged == HEAD (editor-IPC ACK, not just the
   git commit + a fire-and-forget VCS-refresh signal), AND
3. this document's IPC inflight count has drained to 0, AND
4. the authoritative actor is idle (dispatch-ready prompt, no active turn).

This caps inflight at ~1 and removes the windows where two writers touch the doc
at once. Wire it into the supervisor inter-item dispatch path (idle-queue watch /
drain) — the next `do [#id]` / `agent-doc <FILE>` trigger waits on a
`prior_turn_converged(doc)` gate. Compose with the existing drain-owner lease
(`#kp5z`) and `queue_continuation` drainability rather than replacing them.

## Fix 2 — bounded timeout → LOUD force-disk fallback with operation playback

When the convergence gate cannot be satisfied within a bounded timeout (editor
IPC wedged/dead — exactly the `inflight=5` / `send_failed` state), the closeout
**may fall back to a direct `--force-disk` write** so work is never permanently
stuck — BUT this is an error condition, not a routine path:

- Emit an **ERROR-level** ops.log record (not info/best-effort), clearly marking
  that a force-disk fallback occurred because the editor-IPC convergence boundary
  failed.
- Attach an **operation playback**: a replayable structured diagnostic capturing
  the full sequence that led to the wedge so a later agent can root-cause exactly
  what went wrong without guessing — e.g.:
  - the ordered IPC attempt sequence (patch_id, transport socket→file, each ack
    outcome: ok / timeout / `send_failed`), with timestamps;
  - the `inflight` count + per-attempt backpressure;
  - snapshot/baseline/HEAD hashes + candidate vs content_ours lengths/hashes;
  - the cycle/run id, actor generation, supervisor pid + binary freshness;
  - the closeout state-machine transitions (preflight → write → commit →
    postcommit_worktree_check → fallback).
- Persist the playback to a stable, agent-readable location (e.g.
  `.agent-doc/playback/<doc-hash>/<cycle-id>.json`) referenced from the ERROR
  line, so `session-check` / a recovering agent can load it directly.

Rationale (operator directive): force-disk as a fallback is acceptable to avoid
permanent stalls, but the scenario must be **loud + fully diagnosable** — an
agent reading the logs must understand exactly what wedged and why, not just see
a generic `send_failed`.

## Relationship to existing work

- `#pcwcdiskfree` — the boundary's convergence proof replaces "skip disk + signal
  editor + proceed" with "wait for proven convergence (or loud force-disk)".
- `#semmerge` — fewer concurrent writers ⇒ fewer CRDT merge conflicts to resolve.
- `#kp5z` drain lease — composes (lease owns dispatch; boundary owns convergence).
- A wedged JB-plugin *consumer* is a separate plugin-side robustness item; the
  boundary prevents a transient hiccup from compounding into a hard wedge.

## Acceptance

- SimWorld: a queue drain where the editor ACKs slowly never piles inflight > 1;
  item N+1 dispatch waits for item N convergence.
- SimWorld: a wedged editor (no ACK) trips the bounded timeout → force-disk
  fallback emits the ERROR + a loadable operation-playback artifact (assert the
  playback contains the attempt sequence + hashes + state transitions).
- `make check` green; `make tmux-ci` green (live two-pane convergence + wedge).

## Notes

Live-supervisor-critical (edits the supervisor inter-item dispatch + closeout
path) — implement in a focused clean cycle with `make check` + `make tmux-ci`.

## Phasing / Status

Split to land the safe, fully-testable foundation ahead of the live-supervisor-critical
wiring (same pattern the operator accepted for `#agentreloadrestart` Phase 1a/1b).

- **Phase 1 — decision core + loud force-disk playback (SHIPPED).** Pure
  `convergence_gate` module (`ConvergenceFacts` + `convergence_gate_decision` →
  `Dispatch` / `Defer` / `ForceDiskFallback`, no I/O) and `convergence_playback`
  module (`ConvergencePlayback` artifact at `.agent-doc/playback/<doc-hash>/<cycle-id>.json`
  + ERROR-level `convergence_gate_force_disk_fallback severity=error …` ops-log line via
  `record_force_disk_fallback`). 16 unit tests; `make check` green. These touch NO
  live runtime path, so they are safe to land from a drifting session and change no
  supervisor behavior until Phase 2 wires them in. SPEC + VERSIONS updated.

- **Phase 2 — live wiring (CODE SHIPPED, agent-doc submodule `85b173fb`).** Wired
  `convergence_gate_decision` into the supervisor idle-queue-watch drain at the
  `IdleQueueDrainDecision::Dispatch` arm (`start/idle_watch.rs`). Gathers the four
  facts from live state — cycle committed via `cycle_state::load` (defaults `true`
  when no cycle state so a fresh drain is never blocked), editor convergence via a
  disk-worktree-vs-`HEAD` compare reusing the `#pcwc` `normalize_for_replay_hash`
  ground truth (non-git / unreadable `HEAD` defaults `true`),
  `ipc_socket::inflight_connection_handlers()`, and
  `!actor_state_is_busy_or_starting()`. `Defer` starts/keeps a per-boundary timer +
  logs `convergence_gate_defer` and skips the tick (caps inflight at ~1);
  `ForceDiskFallback` (editor IPC wedged past `CONVERGENCE_GATE_TIMEOUT_MS` = 30s/~60
  ticks) persists the `ConvergencePlayback` + emits the LOUD ERROR line, then proceeds
  so the drain is never permanently stuck (disk reconcile stays owned by the existing
  postcommit `disk_after_failed_editor_refresh` path — the idle watch does not author a
  document write). A wrong `editor_converged` fact can at worst cause a bounded 30s
  defer + loud fallback, never a permanent deadlock. 5 new tests
  (editorless/non-git default-true; disk-vs-`HEAD` tracking; slow-ACK never dispatches
  while inflight > 0; wedged-editor trips force-disk after timeout; loud-error +
  loadable-playback artifact). `make check` green (4956 tests), clippy clean. SPEC L36
  already documents this consumer.

  **REMAINING (operator-gated, `#fbwireverify`):** the live two-pane `make tmux-ci`
  acceptance — a real slow-ACK editor drain never piling inflight > 1, and a wedged
  (no-ACK) editor tripping the bounded timeout → force-disk + loadable playback —
  cannot run from the live driving session. Per STOP-cutting-releases the binary is
  committed + pushed but NOT installed; goes live after `make install` +
  `agent-doc admin recycle` at a recycle boundary.
