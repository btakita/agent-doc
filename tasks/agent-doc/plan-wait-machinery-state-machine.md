# Plan: Unified wait-machinery state machine + bounded-hang proof (`#waitmachine`)

## Problem

The "wait machinery" — every place agent-doc blocks for an external condition to
become true — is scattered across `route.rs`, `route/startup.rs`, `start.rs`, and
the IPC paths, each with its own ad-hoc `Instant::now()` / `while elapsed < timeout`
loop and its own duration constant. Observed budgets today are inconsistent and
several exceed the operator's stated ceiling:

| Site | Const / value | Notes |
|------|---------------|-------|
| `start.rs` `AUTO_TRIGGER_TIMEOUT` | **30s** | startup dispatch-ready wait (`#startupdeadline` made it a hard deadline, but at 30s) |
| `start.rs` clear-cooldown / capability-proof | 30s monitor | same monitor |
| `route/startup.rs` `wait_for_agent_ready` | **30s** | fresh-route ready wait |
| `route/startup.rs` `AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT` | 5s | reverify |
| `route.rs` busy reroute / shell waits | 20s, 15s, 10s, 8s, 5s, 3s, 2s | many distinct |
| IPC socket ack | 6s socket + 2s file | `write.rs` finalize path |

Two problems: (1) no single place enforces a global "never hang > 10s" bound, and
(2) nothing *proves* that bound holds on every path. The operator wants a single
state machine for the wait machinery and a proof (tests + Lean formal logic) that
no non-exempt path can dwell longer than 10s. The **only** sanctioned exception is
the dogfood reinstall pause (supervisor auto-install/recycle onto a fresh binary).

## Goals

1. One canonical `WaitMachine` abstraction that every blocking wait routes through.
2. A single, configurable global hang ceiling (default **10s**) enforced in one place.
3. An explicit, *typed* exemption for the reinstall pause — the only state allowed
   to dwell past the global ceiling, and even it is bounded by a separate
   (longer, explicit) reinstall budget rather than truly unbounded.
4. Deterministic tests proving the bound on every transition path.
5. A Lean model proving the bounded-dwell invariant as a theorem.

## Design

### Phase 1 — `WaitMachine` core (`#waitmachine1`)

New module `agent-doc-orchestration/src/wait_machine.rs`:

```rust
enum WaitState {
    Idle,
    AwaitingShell,            // pane shell prompt
    AwaitingDispatchReady,    // harness dispatch-ready prompt
    AwaitingClearCooldown,
    AwaitingCapabilityProof,  // managed Codex proof
    AwaitingIpcAck,           // editor IPC patch ack
    ReinstallPause,           // EXEMPT from the global ceiling
    FailedClosed { reason: WaitFailReason },
    Ready,
}
```

- Each non-exempt state carries a `max_dwell: Duration` ≤ `GLOBAL_HANG_CEILING` (10s).
- A `tick(now)` transition function is **pure** (no I/O): it takes current state +
  elapsed + an observed-signal enum and returns the next state. All clock/PTY/socket
  reads happen in a thin driver; the transition relation is total and side-effect-free
  (this is what makes it testable and Lean-modelable).
- On any non-exempt state reaching `max_dwell`, `tick` returns
  `FailedClosed { reason }` — never "keep polling". This generalizes the
  `#startupdeadline` fix (`auto_trigger_no_prompt_action`) to every wait.
- `ReinstallPause` is entered only from the explicit auto-install/recycle path and is
  the sole state whose dwell is checked against `REINSTALL_BUDGET` (e.g. 120s), logged
  distinctly so a pause is never mistaken for a hang.

### Phase 2 — migrate call sites (`#waitmachine2`)

Replace the ad-hoc loops with `WaitMachine` drivers, lowering the 30s budgets to fit
the 10s ceiling (or justifying any per-state budget that must differ, documented in
the spec). Order: `start.rs` AutoTriggerMonitor → `route/startup.rs`
`wait_for_agent_ready` → `route.rs` busy/shell waits → IPC ack. Keep behavior parity
on the already-bounded paths; only the 30s startup waits change semantics (10s ceiling).

### Phase 3 — deterministic proof in tests (`#waitmachine3`)

- Per-state transition unit tests (continue-before-deadline → fail-closed-once),
  reusing the `#startupdeadline` eval shape.
- A property/exhaustive test over the (state × signal) space asserting:
  for every path with no `ReinstallPause` state, `Σ dwell ≤ 10s`.
- SimWorld coverage for the live startup/route hang classes so the bound is exercised
  end-to-end, not just in the pure transition function.

### Phase 4 — Lean model (`#waitmachine4`)

New `formal/wait_machine/` Lean project (kept out of the cargo build; CI-optional):

- Encode `WaitState` as an inductive type and `tick` as a transition relation
  `Step : WaitState → Signal → Elapsed → WaitState`, mirroring the Rust `tick` 1:1.
- Define `Reachable` paths from `Idle` and `dwell : Path → Duration`.
- **Theorem** `no_hang : ∀ p, Reachable p → ExemptFree p → dwell p ≤ 10s`.
- A second theorem that `ReinstallPause` is the *only* state that can violate the
  10s bound, and is itself bounded by `REINSTALL_BUDGET`.
- A doc note mapping each Lean constructor to its Rust counterpart so the model and
  the implementation are kept in lockstep on edits (the Rust `tick` is the source of
  truth; the Lean `Step` must match it, enforced by review + the parity test).

## Risks / open questions

- **Lean ↔ Rust drift**: the proof only covers the *model*; a mismatch between the
  Lean `Step` and the Rust `tick` would make the proof vacuous. Mitigate with a small
  parity test that enumerates the Rust transition table and a hand-checked Lean table.
- **10s may be too tight** for some legitimate slow paths (cold harness boot, large
  resume). Phase 2 must surface any state that genuinely needs a longer budget; those
  become explicit, named, spec-documented per-state budgets rather than silent 30s.
- This is a deep, multi-phase refactor of live supervisor-critical wait paths; per the
  loop-pacing rule it should run as focused cycles per phase, not inline in a drain.

## Out of scope

- Replacing tmux/PTY mechanics; only the *wait/deadline* layer is unified.
- The reinstall pause itself (kept as the one sanctioned long pause).
