/-
  Stable safety kernel shared by the agent-doc binary and editor adapters.

  The Rust `realtime_ipc_cycle_model` owns schedule exploration, fault injection,
  and liveness. This Lean model intentionally proves only the small laws that
  must stay true for JetBrains, VS Code, legacy, and Lazily-backed adapters.
-/

namespace RealtimeCycle

/-- Multiplicity for one semantic queue identity on each authority plane. -/
structure State where
  canonicalCount : Nat
  editorCount : Nat
  diskCount : Nat
  replicaCount : Nat
  authorityEpoch : Nat
  visibleExact : Bool
  acked : Bool
  deriving DecidableEq, Repr

inductive Action where
  | admitQueueItem
  | saveEditor
  | adoptEditor (structurallyExact unchanged : Bool)
  | projectExact
  | acknowledge
  deriving DecidableEq, Repr

/-- A semantic identity is set-like: every write normalizes multiplicity to 0/1. -/
def oneIfPresent (count : Nat) : Nat := if count = 0 then 0 else 1

def step (s : State) : Action → State
  | .admitQueueItem =>
      { s with
        canonicalCount := 1
        editorCount := 1
        replicaCount := 1
        visibleExact := false
        acked := false }
  | .saveEditor =>
      { s with
        canonicalCount := oneIfPresent s.editorCount
        diskCount := oneIfPresent s.editorCount }
  | .adoptEditor structurallyExact unchanged =>
      if structurallyExact && unchanged then
        { s with
          canonicalCount := oneIfPresent s.editorCount
          replicaCount := oneIfPresent s.editorCount
          authorityEpoch := s.authorityEpoch + 1
          visibleExact := false
          acked := false }
      else s
  | .projectExact =>
      { s with
        visibleExact := true
        acked := false }
  | .acknowledge => { s with acked := s.visibleExact }

def multiplicitySafe (s : State) : Prop :=
  s.canonicalCount ≤ 1 ∧ s.editorCount ≤ 1 ∧
  s.diskCount ≤ 1 ∧ s.replicaCount ≤ 1

theorem oneIfPresent_le_one (count : Nat) : oneIfPresent count ≤ 1 := by
  unfold oneIfPresent
  split <;> simp_all

/-- Saving twice is one durability transition, never a second logical mutation. -/
theorem save_idempotent (s : State) :
    step (step s .saveEditor) .saveEditor = step s .saveEditor := by
  simp [step, oneIfPresent]

/-- Re-adding/saving one semantic queue identity cannot create multiplicity two. -/
theorem admit_save_once (s : State) :
    let settled := step (step (step s .admitQueueItem) .saveEditor) .saveEditor
    settled.canonicalCount = 1 ∧ settled.editorCount = 1 ∧ settled.diskCount = 1 := by
  simp [step, oneIfPresent]

/-- A failed editor-adoption proof is fail-closed and changes no authority plane. -/
theorem adopt_without_proof_is_noop (s : State) :
    step s (.adoptEditor false true) = s ∧
    step s (.adoptEditor true false) = s := by
  simp [step]

/-- A proven adoption makes canonical and replica equal the exact editor cut. -/
theorem proven_adopt_aligns_editor (s : State) :
    let adopted := step s (.adoptEditor true true)
    adopted.canonicalCount = oneIfPresent s.editorCount ∧
    adopted.replicaCount = oneIfPresent s.editorCount ∧
    adopted.authorityEpoch = s.authorityEpoch + 1 := by
  simp [step]

/-- ACK cannot become true without an exact visible projection. -/
theorem ack_requires_exact_visible (s : State) :
    (step s .acknowledge).acked = true → s.visibleExact = true := by
  simp [step]

/-- Every transition preserves the set-like multiplicity invariant. -/
theorem step_preserves_multiplicity (s : State) (a : Action)
    (safe : multiplicitySafe s) : multiplicitySafe (step s a) := by
  rcases safe with ⟨hc, he, hd, hr⟩
  cases a with
  | admitQueueItem =>
      exact ⟨by simp [step], by simp [step], hd, by simp [step]⟩
  | saveEditor =>
      simp [multiplicitySafe, step, oneIfPresent_le_one, he, hr]
  | adoptEditor structurallyExact unchanged =>
      simp only [step]
      split
      · exact ⟨oneIfPresent_le_one _, he, hd, oneIfPresent_le_one _⟩
      · exact ⟨hc, he, hd, hr⟩
  | projectExact => exact ⟨hc, he, hd, hr⟩
  | acknowledge => exact ⟨hc, he, hd, hr⟩

end RealtimeCycle
