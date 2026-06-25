/-
  Lean model of the agent-doc unified wait-machinery state machine (`#waitmachine4`).

  Mirrors the Rust `wait_machine::tick` in
  `agent-doc-orchestration/src/wait_machine.rs` 1:1. The Rust `tick` is the
  SOURCE OF TRUTH; this `Step` relation must match it (kept in lockstep by review
  + the Rust↔Lean parity test `lean_parity_transition_table`).

  Durations are modeled as `Nat` milliseconds so all arithmetic is decidable and
  the bound proof is by `omega`.

  Constructor mapping (see the Rust module doc table):

    Rust                                  Lean
    ----                                  ----
    GLOBAL_HANG_CEILING (10s)             globalHangCeiling = 10000   (ms)
    REINSTALL_BUDGET    (120s)            reinstallBudget   = 120000  (ms)
    WaitState::Idle                       WaitState.idle
    WaitState::AwaitingShell {d}          WaitState.awaitingShell d
    WaitState::AwaitingDispatchReady {d}  WaitState.awaitingDispatch d
    WaitState::AwaitingClearCooldown {d}  WaitState.awaitingCooldown d
    WaitState::AwaitingCapabilityProof{d} WaitState.awaitingProof d
    WaitState::AwaitingIpcAck {d}         WaitState.awaitingIpcAck d
    WaitState::ReinstallPause             WaitState.reinstallPause
    WaitState::Ready                      WaitState.ready
    WaitState::FailedClosed {reason}      WaitState.failedClosed reason
    Signal::Satisfied                     Signal.satisfied
    Signal::StillWaiting                  Signal.stillWaiting
    Signal::Blocked                       Signal.blocked
    tick                                  step
-/

namespace WaitMachine

-- Durations are modeled as plain `Nat` milliseconds (a type *alias* `abbrev Ms :=
-- Nat` is deliberately avoided: omega does not see through such an alias to the
-- underlying `Nat`, which silently defeats the bound proofs).

/-- The single global hang ceiling: 10s = 10000ms. Operator hard constraint:
    never hang > 10s on any exempt-free path. -/
def globalHangCeiling : Nat := 10000

/-- The explicit, longer budget for the sole sanctioned long pause (reinstall). -/
def reinstallBudget : Nat := 120000

/-- Why a wait failed closed (mirrors Rust `WaitFailReason`). -/
inductive FailReason where
  | deadlineExceeded
  | blockerObserved
  | reinstallBudgetExceeded
  deriving DecidableEq, Repr

/-- The wait state. Each non-exempt awaiting variant carries the `maxDwell` it is
    enforced against; `reinstallPause` is the sole exempt state. -/
inductive WaitState where
  | idle
  | awaitingShell (maxDwell : Nat)
  | awaitingDispatch (maxDwell : Nat)
  | awaitingCooldown (maxDwell : Nat)
  | awaitingProof (maxDwell : Nat)
  | awaitingIpcAck (maxDwell : Nat)
  | reinstallPause
  | ready
  | failedClosed (reason : FailReason)
  deriving DecidableEq, Repr

/-- Observed signal a `step` consumes (mirrors Rust `Signal`). -/
inductive Signal where
  | satisfied
  | stillWaiting
  | blocked
  deriving DecidableEq, Repr

open WaitState Signal FailReason

/-- The single place the global ceiling is enforced for non-exempt states
    (mirrors Rust `clamp_to_ceiling`). -/
def clampToCeiling (requested : Nat) : Nat :=
  if requested > globalHangCeiling then globalHangCeiling else requested

/-- Is this state exempt from the global ceiling? Only `reinstallPause` is. -/
def isExempt : WaitState → Bool
  | reinstallPause => true
  | _ => false

/-- Is this state terminal (absorbing)? -/
def isTerminal : WaitState → Bool
  | ready => true
  | failedClosed _ => true
  | _ => false

/-- The dwell budget enforced against a state (mirrors Rust `WaitState::budget`). -/
def budget : WaitState → Nat
  | idle => 0
  | ready => 0
  | failedClosed _ => 0
  | awaitingShell d => d
  | awaitingDispatch d => d
  | awaitingCooldown d => d
  | awaitingProof d => d
  | awaitingIpcAck d => d
  | reinstallPause => reinstallBudget

/-- The pure transition function, mirroring Rust `tick(state, elapsed, signal)`. -/
def step (s : WaitState) (elapsed : Nat) (sig : Signal) : WaitState :=
  match s with
  -- Fixpoints: idle does not advance via step; terminals are absorbing.
  | idle => idle
  | ready => ready
  | failedClosed r => failedClosed r
  -- The sole exempt state.
  | reinstallPause =>
    match sig with
    | satisfied => ready
    | blocked => failedClosed blockerObserved
    | stillWaiting =>
      if elapsed < reinstallBudget then reinstallPause
      else failedClosed reinstallBudgetExceeded
  -- Non-exempt awaiting states share one rule, parameterized by maxDwell.
  | awaitingShell d => stepAwaiting (awaitingShell d) d elapsed sig
  | awaitingDispatch d => stepAwaiting (awaitingDispatch d) d elapsed sig
  | awaitingCooldown d => stepAwaiting (awaitingCooldown d) d elapsed sig
  | awaitingProof d => stepAwaiting (awaitingProof d) d elapsed sig
  | awaitingIpcAck d => stepAwaiting (awaitingIpcAck d) d elapsed sig
where
  /-- Shared non-exempt transition: stay in `self` while `elapsed < maxDwell`,
      else fail closed with `deadlineExceeded`. -/
  stepAwaiting (self : WaitState) (maxDwell elapsed : Nat) (sig : Signal) : WaitState :=
    match sig with
    | satisfied => ready
    | blocked => failedClosed blockerObserved
    | stillWaiting =>
      if elapsed < maxDwell then self
      else failedClosed deadlineExceeded

/-! ## A well-formed non-exempt awaiting state has its budget clamped to the ceiling. -/

/-- A state is *ceiling-bounded* when its enforced budget is `≤ globalHangCeiling`.
    Every non-exempt awaiting state constructed through `clampToCeiling` is. -/
def ceilingBounded (s : WaitState) : Prop := budget s ≤ globalHangCeiling

theorem clampToCeiling_le (requested : Nat) :
    clampToCeiling requested ≤ globalHangCeiling := by
  unfold clampToCeiling
  by_cases h : requested > globalHangCeiling <;> simp [h] <;> omega

/-! ## Core bound: a non-exempt awaiting state cannot dwell past its budget.

    If a non-exempt awaiting state is still in an awaiting state after a `step`,
    then `elapsed < maxDwell`. Contrapositive: once `elapsed ≥ maxDwell`, the
    only `stillWaiting` outcome is `failedClosed`. With a ceiling-bounded budget
    this means the machine fails closed by `globalHangCeiling` — it never hangs. -/

/-- The exhaustive transition characterization for non-exempt awaiting states. -/
theorem stepAwaiting_cases (self : WaitState) (maxDwell elapsed : Nat) (sig : Signal) :
    step.stepAwaiting self maxDwell elapsed sig = ready
    ∨ step.stepAwaiting self maxDwell elapsed sig = failedClosed blockerObserved
    ∨ (step.stepAwaiting self maxDwell elapsed sig = self ∧ elapsed < maxDwell)
    ∨ (step.stepAwaiting self maxDwell elapsed sig = failedClosed deadlineExceeded
        ∧ ¬ elapsed < maxDwell) := by
  unfold step.stepAwaiting
  cases sig with
  | satisfied => left; rfl
  | blocked => right; left; rfl
  | stillWaiting =>
    by_cases h : elapsed < maxDwell
    · right; right; left; exact ⟨by simp [h], h⟩
    · right; right; right; exact ⟨by simp [h], h⟩

/-- **`no_hang` (the headline theorem).** For any ceiling-bounded non-exempt
    awaiting state, if a `stillWaiting` `step` does NOT advance to a terminal
    state, then the elapsed time is strictly below the global hang ceiling. I.e.
    the machine can only keep waiting while under 10s; past 10s it must terminate.
    This is the exempt-free ⇒ dwell ≤ 10s invariant. -/
theorem no_hang
    (s : WaitState) (elapsed : Nat)
    (hne : isExempt s = false)
    (hawait : isTerminal s = false ∧ s ≠ idle)
    (hbound : ceilingBounded s)
    (hstay : step s elapsed stillWaiting = s) :
    elapsed < globalHangCeiling := by
  -- `s` is a non-exempt, non-idle, non-terminal state ⇒ it is an awaiting state.
  unfold ceilingBounded at hbound
  unfold isExempt isTerminal at *
  cases s with
  | idle => exact absurd rfl hawait.2
  | ready => simp at hawait
  | failedClosed r => simp at hawait
  | reinstallPause => simp at hne
  | awaitingShell d =>
    -- step stays in `awaitingShell d` only when `elapsed < d ≤ ceiling`.
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting] at hstay
    by_cases h : elapsed < d
    · omega
    · simp [h] at hstay
  | awaitingDispatch d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting] at hstay
    by_cases h : elapsed < d
    · omega
    · simp [h] at hstay
  | awaitingCooldown d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting] at hstay
    by_cases h : elapsed < d
    · omega
    · simp [h] at hstay
  | awaitingProof d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting] at hstay
    by_cases h : elapsed < d
    · omega
    · simp [h] at hstay
  | awaitingIpcAck d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting] at hstay
    by_cases h : elapsed < d
    · omega
    · simp [h] at hstay

/-- Once a non-exempt ceiling-bounded awaiting state's elapsed reaches the global
    ceiling, a `stillWaiting` `step` fails closed (deadline). The dual of
    `no_hang`: at/after 10s the machine never keeps polling. -/
theorem fail_closed_at_ceiling
    (s : WaitState) (elapsed : Nat)
    (hne : isExempt s = false)
    (hawait : isTerminal s = false ∧ s ≠ idle)
    (hbound : ceilingBounded s)
    (hpast : globalHangCeiling ≤ elapsed) :
    step s elapsed stillWaiting = failedClosed deadlineExceeded := by
  unfold ceilingBounded at hbound
  unfold isExempt isTerminal at *
  cases s with
  | idle => exact absurd rfl hawait.2
  | ready => simp at hawait
  | failedClosed r => simp at hawait
  | reinstallPause => simp at hne
  | awaitingShell d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting]
    have : ¬ elapsed < d := by omega
    simp [this]
  | awaitingDispatch d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting]
    have : ¬ elapsed < d := by omega
    simp [this]
  | awaitingCooldown d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting]
    have : ¬ elapsed < d := by omega
    simp [this]
  | awaitingProof d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting]
    have : ¬ elapsed < d := by omega
    simp [this]
  | awaitingIpcAck d =>
    have hb : d ≤ globalHangCeiling := hbound
    simp only [step, step.stepAwaiting]
    have : ¬ elapsed < d := by omega
    simp [this]

/-! ## Reinstall-pause is the SOLE exemption. -/

/-- `reinstallPause` is the only state for which `isExempt` is `true`. -/
theorem reinstall_pause_is_sole_exemption (s : WaitState) :
    isExempt s = true ↔ s = reinstallPause := by
  cases s <;> simp [isExempt]

/-- The exemption is REAL: `reinstallPause` may keep waiting strictly past the
    global ceiling (so it is genuinely exempt, unlike every non-exempt state which
    `fail_closed_at_ceiling` forces to terminate). -/
theorem reinstall_pause_may_exceed_ceiling
    (elapsed : Nat)
    (_hpast : globalHangCeiling ≤ elapsed)
    (hunder : elapsed < reinstallBudget) :
    step reinstallPause elapsed stillWaiting = reinstallPause := by
  simp [step, hunder]

/-- The exemption is BOUNDED: even `reinstallPause` fails closed once it overruns
    `reinstallBudget` — it is never truly unbounded (mirrors the Rust
    `ReinstallBudgetExceeded` branch). -/
theorem reinstall_pause_is_bounded
    (elapsed : Nat)
    (hpast : reinstallBudget ≤ elapsed) :
    step reinstallPause elapsed stillWaiting = failedClosed reinstallBudgetExceeded := by
  have : ¬ elapsed < reinstallBudget := by omega
  simp [step, this]

/-! ## Parity table: the exhaustive (state × signal × deadline-position) rows.

    These `example`s reproduce, as machine-checked equalities, exactly the rows
    the Rust parity test `lean_parity_transition_table` spot-checks. Any drift
    between Rust `tick` and Lean `step` breaks one of these (or the Rust test). -/

-- A concrete under-ceiling budget (8s), matching the Rust parity table.
private def d8 : Nat := 8000

-- awaitingDispatch | stillWaiting | beforeDeadline => awaitingDispatch
example : step (awaitingDispatch d8) (d8 - 1) stillWaiting = awaitingDispatch d8 := by
  simp [step, step.stepAwaiting, d8]

-- awaitingDispatch | stillWaiting | atDeadline => failedClosed(deadlineExceeded)
example : step (awaitingDispatch d8) d8 stillWaiting = failedClosed deadlineExceeded := by
  simp [step, step.stepAwaiting, d8]

-- awaitingShell | satisfied | beforeDeadline => ready
example : step (awaitingShell d8) (d8 - 1) satisfied = ready := by
  simp [step, step.stepAwaiting]

-- awaitingIpcAck | blocked | beforeDeadline => failedClosed(blockerObserved)
example : step (awaitingIpcAck d8) (d8 - 1) blocked = failedClosed blockerObserved := by
  simp [step, step.stepAwaiting]

-- reinstallPause | stillWaiting | beforeDeadline => reinstallPause
example : step reinstallPause (reinstallBudget - 1) stillWaiting = reinstallPause := by
  simp [step, reinstallBudget]

-- reinstallPause | stillWaiting | atDeadline => failedClosed(reinstallBudgetExceeded)
example : step reinstallPause reinstallBudget stillWaiting = failedClosed reinstallBudgetExceeded := by
  simp [step, reinstallBudget]

end WaitMachine
