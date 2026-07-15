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
  editorOpen : Bool
  pendingDisk : Bool
  visibleExact : Bool
  acked : Bool
  deriving DecidableEq, Repr

inductive Action where
  | admitQueueItem
  | saveEditor
  | externalDiskChange (count : Nat)
  | acceptPendingDisk
  | editEditor (count : Nat)
  | closeEditor
  | adoptEditor (structurallyExact unchanged : Bool)
  | projectExact
  | acknowledge
  deriving DecidableEq, Repr

/-- Evidence classes for rule-based authority resolution. CRDT convergence can
merge compatible causal histories, but cannot choose between distinct semantic
replacement intents or manufacture missing lineage. -/
inductive AmbiguityEvidence where
  | sameSemanticOperation
  | editorCausallyNewer
  | replicaCausallyNewer
  | concurrentCrdtCompatible
  | concurrentSemanticConflict
  | missingCausalProof
deriving DecidableEq, Repr

inductive AmbiguityResolution where
  | dedupe
  | chooseEditor
  | chooseReplica
  | mergeCrdt
  | needsOperator
deriving DecidableEq, Repr

def resolveAmbiguity : AmbiguityEvidence → AmbiguityResolution
  | .sameSemanticOperation => .dedupe
  | .editorCausallyNewer => .chooseEditor
  | .replicaCausallyNewer => .chooseReplica
  | .concurrentCrdtCompatible => .mergeCrdt
  | .concurrentSemanticConflict => .needsOperator
  | .missingCausalProof => .needsOperator

theorem semantic_conflict_needs_operator :
    resolveAmbiguity .concurrentSemanticConflict = .needsOperator := by
  rfl

theorem missing_lineage_needs_operator :
    resolveAmbiguity .missingCausalProof = .needsOperator := by
  rfl

theorem causal_rules_are_automatic :
    resolveAmbiguity .sameSemanticOperation = .dedupe ∧
    resolveAmbiguity .editorCausallyNewer = .chooseEditor ∧
    resolveAmbiguity .replicaCausallyNewer = .chooseReplica ∧
    resolveAmbiguity .concurrentCrdtCompatible = .mergeCrdt := by
  decide

/-- A semantic identity is set-like: every write normalizes multiplicity to 0/1. -/
def oneIfPresent (count : Nat) : Nat := if count = 0 then 0 else 1

def step (s : State) : Action → State
  | .admitQueueItem =>
      { s with
        canonicalCount := 1
        editorCount := 1
        replicaCount := 1
        pendingDisk := false
        visibleExact := false
        acked := false }
  | .saveEditor =>
      { s with
        canonicalCount := oneIfPresent s.editorCount
        diskCount := oneIfPresent s.editorCount
        pendingDisk := false }
  | .externalDiskChange count =>
      let disk := oneIfPresent count
      if s.editorOpen then
        { s with diskCount := disk, pendingDisk := true }
      else
        { s with canonicalCount := disk, diskCount := disk, pendingDisk := false }
  | .acceptPendingDisk =>
      if s.editorOpen && s.pendingDisk then
        { s with
          canonicalCount := oneIfPresent s.diskCount
          editorCount := oneIfPresent s.diskCount
          pendingDisk := false }
      else s
  | .editEditor count =>
      let editor := oneIfPresent count
      { s with
        canonicalCount := editor
        editorCount := editor
        pendingDisk := false }
  | .closeEditor =>
      { s with
        canonicalCount := oneIfPresent s.diskCount
        editorOpen := false
        pendingDisk := false }
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

/-- A disk change observed while an editor is open is retained as a pending
candidate and cannot mutate either live authority plane. -/
theorem external_disk_waits_for_editor (s : State) (count : Nat)
  (openEditor : s.editorOpen = true) :
  let changed := step s (.externalDiskChange count)
  changed.canonicalCount = s.canonicalCount ∧
  changed.editorCount = s.editorCount ∧
  changed.diskCount = oneIfPresent count ∧
  changed.pendingDisk = true := by
  simp [step, openEditor]

/-- Once editor bytes are saved to disk they supersede any older pending disk
candidate and align canonical/disk authority to that exact editor cut. -/
theorem editor_save_clears_pending (s : State) :
  let saved := step s .saveEditor
  saved.canonicalCount = oneIfPresent s.editorCount ∧
  saved.diskCount = oneIfPresent s.editorCount ∧
  saved.pendingDisk = false := by
  simp [step]

/-- A new editor mutation supersedes an unresolved external-disk candidate. -/
theorem editor_edit_clears_pending (s : State) (count : Nat) :
  let edited := step s (.editEditor count)
  edited.canonicalCount = oneIfPresent count ∧
  edited.editorCount = oneIfPresent count ∧
  edited.pendingDisk = false := by
  simp [step]

/-- Closing the final editor clears the decision and falls back to disk. -/
theorem close_editor_falls_back_to_disk (s : State) :
  let closed := step s .closeEditor
  closed.canonicalCount = oneIfPresent s.diskCount ∧
  closed.editorOpen = false ∧
  closed.pendingDisk = false := by
  simp [step]

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
  | externalDiskChange count =>
      simp only [step]
      split
      · exact ⟨hc, he, oneIfPresent_le_one _, hr⟩
      · exact ⟨oneIfPresent_le_one _, he, oneIfPresent_le_one _, hr⟩
  | acceptPendingDisk =>
      simp only [step]
      split
      · exact ⟨oneIfPresent_le_one _, oneIfPresent_le_one _, hd, hr⟩
      · exact ⟨hc, he, hd, hr⟩
  | editEditor count =>
      exact ⟨oneIfPresent_le_one _, oneIfPresent_le_one _, hd, hr⟩
  | closeEditor =>
      exact ⟨oneIfPresent_le_one _, he, hd, hr⟩
  | adoptEditor structurallyExact unchanged =>
      simp only [step]
      split
      · exact ⟨oneIfPresent_le_one _, he, hd, oneIfPresent_le_one _⟩
      · exact ⟨hc, he, hd, hr⟩
  | projectExact => exact ⟨hc, he, hd, hr⟩
  | acknowledge => exact ⟨hc, he, hd, hr⟩

end RealtimeCycle
