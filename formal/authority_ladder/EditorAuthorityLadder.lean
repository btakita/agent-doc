/-
  Editor authority ladder: which state is canonical for a session document,
  and a proof that forward merging never rolls an open editor back.

  Observed 2026-09-29 on src/haiven-dev/tasks/api.md: the controller
  acknowledged the operator's typed state, quarantined it, lost it across an
  install handoff, and the editor's re-registration adopted the stale CRDT
  text over the buffer. The operator's text was gone.

  The ladder (operator-specified):
    1. two or more editors open → canonical = reconciliation of their states;
    2. one editor open          → canonical = its buffer;
    3. no editor open           → canonical = disk.
  The in-memory CRDT is a merge engine and copy, never an authority over an
  open editor: editors only merge FORWARD, registration publishes the editor
  state and merges forward, and a handoff reseeds the CRDT from the ladder.

  Content is modelled as a set of atoms (`Atom → Prop`). `deleted` is the set
  of atoms some actor intentionally deleted (tombstones). An editor rolls back
  when it loses an atom that nobody deleted.

  `formal/tla/EditorAuthorityLadder.tla` model-checks bounded instances of the
  same actions, including the shipped adopt-canonical behaviour as a violating
  Wedge config. This file proves the safety property for EVERY instance.
-/

namespace EditorAuthorityLadder

open Classical

variable {E A : Type}

/-- Document state: which editors are open, their buffers, the in-memory CRDT,
disk, and the tombstones. -/
structure State (E A : Type) where
  isOpen  : E → Prop
  buf     : E → A → Prop
  crdt    : A → Prop
  disk    : A → Prop
  deleted : A → Prop

/-- The canonical state for the current rung of the ladder: the reconciliation
of the open editors, or disk when none is open. -/
noncomputable def canonical (s : State E A) : A → Prop :=
  if ∃ e, s.isOpen e then
    fun a => (∃ e, s.isOpen e ∧ s.buf e a) ∧ ¬ s.deleted a
  else s.disk

/-- One step of the editor-authority design. Every constructor fixes the whole
successor state. -/
inductive Step : State E A → State E A → Prop
  | openEditor (s : State E A) (e : E) (h : ¬ s.isOpen e) :
      Step s { s with
        isOpen := fun x => s.isOpen x ∨ x = e
        buf := fun x a => if x = e then canonical s a else s.buf x a }
  | closeEditor (s : State E A) (e : E) :
      Step s { s with
        isOpen := fun x => s.isOpen x ∧ x ≠ e
        disk := if (∀ x, s.isOpen x → x = e) then s.buf e else s.disk }
  | operatorTypes (s : State E A) (e : E) (a₀ : A) :
      Step s { s with buf := fun x a => if x = e then s.buf x a ∨ a = a₀ else s.buf x a }
  | operatorDeletes (s : State E A) (e : E) (a₀ : A) :
      Step s { s with
        buf := fun x a => if x = e then s.buf x a ∧ a ≠ a₀ else s.buf x a
        deleted := fun a => s.deleted a ∨ a = a₀ }
  | forward (s : State E A) (e : E) :
      Step s { s with crdt := fun a => (s.crdt a ∨ s.buf e a) ∧ ¬ s.deleted a }
  | agentWrites (s : State E A) (a₀ : A) :
      Step s { s with crdt := fun a => s.crdt a ∨ a = a₀ }
  | agentDeletes (s : State E A) (a₀ : A) :
      Step s { s with
        crdt := fun a => s.crdt a ∧ a ≠ a₀
        deleted := fun a => s.deleted a ∨ a = a₀ }
  | deliver (s : State E A) (e : E) :
      Step s { s with
        buf := fun x a => if x = e then (s.buf x a ∨ s.crdt a) ∧ ¬ s.deleted a else s.buf x a }
  | registerForward (s : State E A) (e : E) :
      Step s { s with
        crdt := fun a => (s.crdt a ∨ s.buf e a) ∧ ¬ s.deleted a
        buf := fun x a => if x = e then (s.buf x a ∨ s.crdt a) ∧ ¬ s.deleted a else s.buf x a }
  | handoff (s : State E A) :
      Step s { s with crdt := canonical s }

/-- Tombstones are permanent: no step forgets that something was deleted. -/
theorem deleted_monotone {s t : State E A} (h : Step s t) (a : A) :
    s.deleted a → t.deleted a := by
  intro hd
  cases h <;> simp_all

/-- **No rollback.** Across every step, an editor that stays open keeps every
atom it held unless that atom was deleted. This holds for any editors and any
content: the editor state only ever merges forward. -/
theorem no_rollback {s t : State E A} (h : Step s t) (e : E) (a : A)
    (hopen : s.isOpen e) (hheld : s.buf e a) (hnotdel : ¬ t.deleted a) :
    t.buf e a := by
  cases h with
  | openEditor e₀ hclosed =>
      have hne : e ≠ e₀ := fun heq => hclosed (heq ▸ hopen)
      simp [hne, hheld]
  | closeEditor => simpa using hheld
  | operatorTypes e₀ a₀ =>
      by_cases he : e = e₀
      · subst he; simp [hheld]
      · simp [he, hheld]
  | operatorDeletes e₀ a₀ =>
      by_cases he : e = e₀
      · subst he
        have ha : a ≠ a₀ := fun heq => hnotdel (by simp [heq])
        simp [hheld, ha]
      · simp [he, hheld]
  | forward => simpa using hheld
  | agentWrites => simpa using hheld
  | agentDeletes => simpa using hheld
  | deliver e₀ =>
      by_cases he : e = e₀
      · subst he
        have hn : ¬ s.deleted a := by simpa using hnotdel
        simp [hheld, hn]
      · simp [he, hheld]
  | registerForward e₀ =>
      by_cases he : e = e₀
      · subst he
        have hn : ¬ s.deleted a := by simpa using hnotdel
        simp [hheld, hn]
      · simp [he, hheld]
  | handoff => simpa using hheld

/-- A controller handoff never regresses the CRDT below an open editor: it
reseeds from the ladder, so every open editor's undeleted text is in it. -/
theorem handoff_never_regresses (s : State E A) (e : E) (a : A)
    (hopen : s.isOpen e) (hheld : s.buf e a) (hnotdel : ¬ s.deleted a) :
    canonical s a := by
  have hany : ∃ x, s.isOpen x := ⟨e, hopen⟩
  simp only [canonical, hany, if_true]
  exact ⟨⟨e, hopen, hheld⟩, hnotdel⟩

/-- Multi-editor reconciliation: every open editor's undeleted text is part of
the canonical state, so no editor's work is dropped in favour of another's. -/
theorem canonical_reconciles_all_open_editors (s : State E A) :
    ∀ e a, s.isOpen e → s.buf e a → ¬ s.deleted a → canonical s a :=
  fun e a hopen hheld hnotdel => handoff_never_regresses s e a hopen hheld hnotdel

/-- With no editor open, disk is canonical. -/
theorem canonical_is_disk_when_none_open (s : State E A) (h : ∀ e, ¬ s.isOpen e) :
    canonical s = s.disk := by
  have hnone : ¬ ∃ e, s.isOpen e := fun ⟨e, he⟩ => h e he
  simp [canonical, hnone]

/-- The shipped registration: adopt the CRDT over the editor buffer. -/
def adoptRegistration (s : State E A) (e : E) : State E A :=
  { s with buf := fun x a => if x = e then s.crdt a else s.buf x a }

/-- **Non-vacuity.** Adopting the CRDT over an open editor can roll it back:
the api.md shape, where the CRDT lacks text the editor holds and nobody
deleted. So `no_rollback` is carried by the forward-merge design. -/
theorem adopt_can_roll_back :
    ∃ (s : State Unit Bool),
      s.isOpen () ∧ s.buf () true ∧ ¬ s.deleted true ∧
      ¬ (adoptRegistration s ()).buf () true :=
  ⟨{ isOpen := fun _ => True, buf := fun _ a => a = true, crdt := fun _ => False,
     disk := fun _ => False, deleted := fun _ => False },
   trivial, rfl, id, by simp [adoptRegistration]⟩

end EditorAuthorityLadder
