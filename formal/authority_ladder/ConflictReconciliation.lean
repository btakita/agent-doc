/-
  Conflict reconciliation between the operator's live edit and the agent's
  incoming merge (`#editorauth1`, plan: tasks/agent-doc/plan-editor-authority-ladder.md).

  `EditorAuthorityLadder.lean` models content as a set of atoms and proves no
  step rolls an open editor back. That abstraction cannot say WHERE text lands.
  Here content is a sequence and the operator has a cursor, so the ordering
  rules the operator specified become theorems:

    1. Both append at the same point → the agent's content goes BEFORE the
       operator's in-progress append, and the cursor stays at the end of the
       operator's edit.
    2. The operator edits existing text while the agent edits elsewhere → both
       apply; the regions are independent.
    3. Both edit the same existing text → a true conflict. It is surfaced in the
       buffer, never dropped on either side, and resolving it to either side
       reproduces that side exactly. The rendering is compact: text common to
       both sides stays outside the conflict (operator decision 2026-09-29;
       implementation `agent-doc-merge/src/conflict_render.rs`, whose inline
       CriticMarkup and block markers are two notations for `Seg.conflict`).

  An edit replaces the span `[pos, pos + len)` of the shared base with `ins`;
  an append is an edit with `len = 0`. Everything here holds for every base,
  every element type, and every pair of edits.

  `formal/tla/ConflictReconciliation.tla` model-checks bounded instances of the
  same merge, including the operator typing on at the cursor afterwards, with
  wedge configs for the two shapes these theorems rule out.
-/

namespace ConflictReconciliation

set_option linter.unusedSectionVars false

variable {α : Type} [DecidableEq α]

/-- Replace `[pos, pos + len)` of the base with `ins`. -/
structure Edit (α : Type) where
  pos : Nat
  len : Nat
  ins : List α

def Edit.stop (e : Edit α) : Nat := e.pos + e.len

/-- Apply one edit to the base. -/
def apply (b : List α) (e : Edit α) : List α :=
  b.take e.pos ++ e.ins ++ b.drop e.stop

/-- Buffer content: ordinary text, or a conflict carrying both sides. -/
inductive Seg (α : Type) where
  | plain (a : α)
  | conflict (yours agent : List α)
  deriving DecidableEq

inductive Keep where
  | yours
  | agent

def plains (l : List α) : List (Seg α) := l.map Seg.plain

/-- Resolve every conflict to one side (`resolve_conflicts`). -/
def resolve (k : Keep) : List (Seg α) → List α
  | [] => []
  | .plain a :: r => a :: resolve k r
  | .conflict y g :: r => (match k with | .yours => y | .agent => g) ++ resolve k r

def conflictFree : List (Seg α) → Prop
  | [] => True
  | .plain _ :: r => conflictFree r
  | .conflict _ _ :: _ => False

/-- Split off the longest common prefix. -/
def splitPrefix : List α → List α → List α × List α × List α
  | x :: xs, y :: ys =>
      if x = y then
        let r := splitPrefix xs ys
        (x :: r.1, r.2.1, r.2.2)
      else ([], x :: xs, y :: ys)
  | xs, ys => ([], xs, ys)

/-- Render a conflict compactly: the common prefix and suffix stay plain text,
and only the differing middle is marked. Equal sides are not a conflict. -/
def render (y g : List α) : List (Seg α) :=
  let p := splitPrefix y g
  let s := splitPrefix p.2.1.reverse p.2.2.reverse
  if s.2.1 = [] ∧ s.2.2 = [] then plains y
  else plains p.1 ++ [.conflict s.2.1.reverse s.2.2.reverse] ++ plains s.1.reverse

/-- The merge. The operator's edit is `o`, the agent's is `g`; both are
relative to the same base. The cursor is `none` only for a conflict. -/
def merge (b : List α) (o g : Edit α) : List (Seg α) × Option Nat :=
  if o.len = 0 ∧ g.len = 0 ∧ o.pos = g.pos then
    -- Rule 1: agent content first, operator's append after it.
    (plains (b.take o.pos ++ g.ins ++ o.ins ++ b.drop o.pos),
     some (o.pos + g.ins.length + o.ins.length))
  else if g.stop ≤ o.pos then
    -- Rule 2, agent's region before the operator's.
    (plains (b.take g.pos ++ g.ins ++ (b.drop g.stop).take (o.pos - g.stop) ++ o.ins
              ++ b.drop o.stop),
     some (g.pos + g.ins.length + (o.pos - g.stop) + o.ins.length))
  else if o.stop ≤ g.pos then
    -- Rule 2, operator's region before the agent's.
    (plains (b.take o.pos ++ o.ins ++ (b.drop o.stop).take (g.pos - o.stop) ++ g.ins
              ++ b.drop g.stop),
     some (o.pos + o.ins.length))
  else
    -- Rule 3: overlapping spans. Surface both sides over their union.
    let lo := min o.pos g.pos
    let hi := max o.stop g.stop
    let side := fun (e : Edit α) =>
      (b.drop lo).take (e.pos - lo) ++ e.ins ++ (b.drop e.stop).take (hi - e.stop)
    (plains (b.take lo) ++ render (side o) (side g) ++ plains (b.drop hi), none)

def overlaps (o g : Edit α) : Prop :=
  ¬ (o.len = 0 ∧ g.len = 0 ∧ o.pos = g.pos) ∧ ¬ g.stop ≤ o.pos ∧ ¬ o.stop ≤ g.pos

/-! ### Lemmas -/

@[simp] theorem resolve_nil (k : Keep) : resolve k ([] : List (Seg α)) = [] := rfl

theorem resolve_append (k : Keep) (l r : List (Seg α)) :
    resolve k (l ++ r) = resolve k l ++ resolve k r := by
  induction l with
  | nil => rfl
  | cons s l ih =>
      cases s <;> simp [resolve, ih, List.append_assoc]

@[simp] theorem resolve_plains (k : Keep) (l : List α) : resolve k (plains l) = l := by
  induction l with
  | nil => rfl
  | cons a l ih => simp [plains, resolve] at *; exact ih

theorem conflictFree_plains (l : List α) : conflictFree (plains l) := by
  induction l with
  | nil => trivial
  | cons a l ih => exact ih

theorem conflictFree_append {l r : List (Seg α)} :
    conflictFree (l ++ r) ↔ conflictFree l ∧ conflictFree r := by
  induction l with
  | nil => simp [conflictFree]
  | cons s l ih => cases s <;> simp [conflictFree, ih]

theorem splitPrefix_spec : ∀ (y g : List α),
    y = (splitPrefix y g).1 ++ (splitPrefix y g).2.1 ∧
    g = (splitPrefix y g).1 ++ (splitPrefix y g).2.2 ∧
    ∀ x, ¬ ((splitPrefix y g).2.1.head? = some x ∧ (splitPrefix y g).2.2.head? = some x)
  | [], g => by simp [splitPrefix]
  | _ :: _, [] => by simp [splitPrefix]
  | x :: xs, z :: zs => by
      by_cases h : x = z
      · subst h
        have ih := splitPrefix_spec xs zs
        simp only [splitPrefix, if_true]
        exact ⟨by simp [← ih.1], by simp [← ih.2.1], ih.2.2⟩
      · simp only [splitPrefix, h, if_false]
        refine ⟨rfl, rfl, ?_⟩
        intro w ⟨h1, h2⟩
        simp at h1 h2
        exact h (h1.trans h2.symm)

/-- Rendering is lossless in both directions. -/
theorem resolve_render (y g : List α) :
    resolve .yours (render y g) = y ∧ resolve .agent (render y g) = g := by
  have hs := splitPrefix_spec y g
  dsimp only [render]
  generalize splitPrefix y g = P at hs ⊢
  obtain ⟨p, r1, r2⟩ := P
  obtain ⟨hy, hg, _⟩ := hs
  have hs' := splitPrefix_spec r1.reverse r2.reverse
  generalize splitPrefix r1.reverse r2.reverse = S at hs' ⊢
  obtain ⟨sfx, m1, m2⟩ := S
  obtain ⟨hy', hg', _⟩ := hs'
  have ry : r1 = m1.reverse ++ sfx.reverse := by
    rw [← List.reverse_append, ← hy', List.reverse_reverse]
  have rg : r2 = m2.reverse ++ sfx.reverse := by
    rw [← List.reverse_append, ← hg', List.reverse_reverse]
  subst hy hg ry rg
  split
  · rename_i hmid
    obtain ⟨h1, h2⟩ := hmid
    subst h1 h2
    simp
  · simp [resolve_append, resolve, List.append_assoc]

/-- Compactness: text the two sides share at either end is never inside the
conflict marks, and a conflict always has a nonempty side. -/
theorem render_compact (y g yc gc : List α) (pre suf : List (Seg α))
    (h : render y g = pre ++ [.conflict yc gc] ++ suf) :
    (∀ x, ¬ (yc.head? = some x ∧ gc.head? = some x)) ∧
    (∀ x, ¬ (yc.getLast? = some x ∧ gc.getLast? = some x)) ∧
    ¬ (yc = [] ∧ gc = []) := by
  have hs := splitPrefix_spec y g
  dsimp only [render] at h
  generalize splitPrefix y g = P at hs h
  obtain ⟨p, r1, r2⟩ := P
  obtain ⟨_, _, hhead⟩ := hs
  have hs' := splitPrefix_spec r1.reverse r2.reverse
  generalize splitPrefix r1.reverse r2.reverse = S at hs' h
  obtain ⟨sfx, m1, m2⟩ := S
  obtain ⟨hy', hg', hlast⟩ := hs'
  have ry : r1 = m1.reverse ++ sfx.reverse := by
    rw [← List.reverse_append, ← hy', List.reverse_reverse]
  have rg : r2 = m2.reverse ++ sfx.reverse := by
    rw [← List.reverse_append, ← hg', List.reverse_reverse]
  simp only at h hhead
  split at h
  · -- all-plain rendering has no conflict node
    have hcf : conflictFree (pre ++ [.conflict yc gc] ++ suf) := h ▸ conflictFree_plains y
    simp [conflictFree_append, conflictFree] at hcf
  · rename_i hne
    -- the only conflict node in `plains p ++ [c] ++ plains s` is `c`
    have key : ∀ (p s : List α) (c : Seg α) (pre suf : List (Seg α)),
        plains p ++ [c] ++ plains s = pre ++ [.conflict yc gc] ++ suf →
        c = .conflict yc gc := by
      intro p s c pre suf heq
      induction p generalizing pre with
      | nil =>
          cases pre with
          | nil => simp [plains] at heq; exact heq.1
          | cons s0 pre =>
              simp [plains] at heq
              obtain ⟨_, htail⟩ := heq
              have hcf : conflictFree (pre ++ .conflict yc gc :: suf) := by
                rw [← htail]; exact conflictFree_plains s
              simp [conflictFree_append, conflictFree] at hcf
      | cons a p ih =>
          cases pre with
          | nil =>
              simp [plains] at heq
          | cons s0 pre =>
              simp [plains] at heq
              exact ih pre (by simpa [plains] using heq.2)
    have hc := key _ _ _ _ _ h
    simp only [Seg.conflict.injEq] at hc
    obtain ⟨rfl, rfl⟩ := hc
    refine ⟨?_, ?_, ?_⟩
    · -- a shared first element would have joined the common prefix
      intro x ⟨h1, h2⟩
      apply hhead x
      rw [ry, rg]
      simp [List.head?_append, h1, h2]
    · intro x ⟨h1, h2⟩
      apply hlast x
      simp only [List.getLast?_reverse] at h1 h2
      exact ⟨h1, h2⟩
    · intro ⟨h1, h2⟩
      apply hne
      simpa using And.intro h1 h2

/-- `b.take j` splits at any earlier point. -/
theorem take_split (b : List α) {i j : Nat} (h : i ≤ j) :
    b.take j = b.take i ++ (b.drop i).take (j - i) := by
  rw [← List.take_add]
  congr 1
  omega

/-- `b.drop j` splits at any later point. -/
theorem drop_split (b : List α) {j k : Nat} (h : j ≤ k) :
    b.drop j = (b.drop j).take (k - j) ++ b.drop k := by
  conv => lhs; rw [← List.take_append_drop (k - j) (b.drop j)]
  congr 1
  rw [List.drop_drop]
  congr 1
  omega

/-! ### Rule 1: same-point appends -/

/-- **Rule 1.** Both append at the same point: the agent's content lands first,
the operator's append follows it, and the cursor sits at the end of the
operator's append (so continued typing extends it). -/
theorem same_point_agent_first (b : List α) (o g : Edit α)
    (ho : o.len = 0) (hg : g.len = 0) (hp : o.pos = g.pos) (hvalid : o.pos ≤ b.length) :
    let m := merge b o g
    conflictFree m.1 ∧
    resolve .yours m.1 = b.take o.pos ++ g.ins ++ o.ins ++ b.drop o.pos ∧
    m.2 = some (o.pos + g.ins.length + o.ins.length) ∧
    (resolve .yours m.1).take (o.pos + g.ins.length + o.ins.length) =
      b.take o.pos ++ g.ins ++ o.ins := by
  simp only [merge, ho, hg, hp, and_self, if_true]
  refine ⟨conflictFree_plains _, resolve_plains _ _, ?_, ?_⟩
  · simp
  rw [resolve_plains, ← hp]
  have hlen : (b.take o.pos ++ g.ins ++ o.ins).length = o.pos + g.ins.length + o.ins.length := by
    simp [List.length_take, Nat.min_eq_left hvalid]; omega
  rw [← hlen, List.take_left']
  rfl

/-! ### Rule 2: independent regions -/

/-- **Rule 2 (agent first).** The agent's region ends at or before the
operator's: both apply, and the cursor stays at the end of the operator's
edit, shifted by the agent's change in length. -/
theorem agent_before_applies_both (b : List α) (o g : Edit α)
    (hsame : ¬ (o.len = 0 ∧ g.len = 0 ∧ o.pos = g.pos)) (hbefore : g.stop ≤ o.pos)
    (hvalid : o.pos ≤ b.length) :
    let m := merge b o g
    conflictFree m.1 ∧
    ∃ mid, resolve .yours m.1 = b.take g.pos ++ g.ins ++ mid ++ o.ins ++ b.drop o.stop ∧
      apply b g = b.take g.pos ++ g.ins ++ mid ++ b.drop o.pos ∧
      m.2 = some ((b.take g.pos ++ g.ins ++ mid ++ o.ins).length) := by
  simp only [merge, hsame, if_false, hbefore, if_true]
  have hgp : g.pos ≤ b.length := by unfold Edit.stop at hbefore; omega
  refine ⟨conflictFree_plains _, (b.drop g.stop).take (o.pos - g.stop), resolve_plains _ _,
    ?_, ?_⟩
  · simp only [apply, List.append_assoc]
    congr 2
    exact drop_split b hbefore
  · congr 1
    have hmid : ((b.drop g.stop).take (o.pos - g.stop)).length = o.pos - g.stop := by
      simp [List.length_take, List.length_drop]; unfold Edit.stop at hbefore; omega
    simp [List.length_take, Nat.min_eq_left hgp, hmid]
    omega

/-- **Rule 2 (operator first).** The operator's region ends at or before the
agent's: both apply and the cursor is untouched by the agent. -/
theorem operator_before_applies_both (b : List α) (o g : Edit α)
    (hsame : ¬ (o.len = 0 ∧ g.len = 0 ∧ o.pos = g.pos)) (hnot : ¬ g.stop ≤ o.pos)
    (hbefore : o.stop ≤ g.pos) (hvalid : o.pos ≤ b.length) :
    let m := merge b o g
    conflictFree m.1 ∧
    ∃ mid, resolve .yours m.1 = b.take o.pos ++ o.ins ++ mid ++ g.ins ++ b.drop g.stop ∧
      apply b o = b.take o.pos ++ o.ins ++ mid ++ b.drop g.pos ∧
      m.2 = some ((b.take o.pos ++ o.ins).length) := by
  simp only [merge, hsame, if_false, hnot, hbefore, if_true]
  refine ⟨conflictFree_plains _, (b.drop o.stop).take (g.pos - o.stop), resolve_plains _ _,
    ?_, ?_⟩
  · simp only [apply, List.append_assoc]
    congr 2
    exact drop_split b hbefore
  · simp [List.length_take, Nat.min_eq_left hvalid]

/-! ### Rule 3: same-span conflicts -/

/-- **Rule 3.** Overlapping edits are a conflict that is never dropped:
resolving to the operator's side reproduces the operator's edit exactly, and
resolving to the agent's side reproduces the agent's edit exactly. -/
theorem conflict_resolves_to_either_side (b : List α) (o g : Edit α) (hov : overlaps o g) :
    resolve .yours (merge b o g).1 = apply b o ∧
    resolve .agent (merge b o g).1 = apply b g := by
  obtain ⟨hsame, hn1, hn2⟩ := hov
  simp only [merge, hsame, hn1, hn2, if_false]
  unfold Edit.stop at hn1 hn2
  have side_ok : ∀ (e : Edit α), min o.pos g.pos ≤ e.pos → e.stop ≤ max o.stop g.stop →
      b.take (min o.pos g.pos) ++
        ((b.drop (min o.pos g.pos)).take (e.pos - min o.pos g.pos) ++ e.ins ++
          (b.drop e.stop).take (max o.stop g.stop - e.stop)) ++
        b.drop (max o.stop g.stop) = apply b e := by
    intro e hlo hhi
    conv => rhs; unfold apply; rw [take_split b hlo, drop_split b hhi]
    simp [List.append_assoc]
  obtain ⟨ry, rg⟩ := resolve_render
    ((b.drop (min o.pos g.pos)).take (o.pos - min o.pos g.pos) ++ o.ins ++
      (b.drop o.stop).take (max o.stop g.stop - o.stop))
    ((b.drop (min o.pos g.pos)).take (g.pos - min o.pos g.pos) ++ g.ins ++
      (b.drop g.stop).take (max o.stop g.stop - g.stop))
  constructor
  · rw [resolve_append, resolve_append, resolve_plains, resolve_plains, ry]
    exact side_ok o (Nat.min_le_left _ _) (Nat.le_max_left _ _)
  · rw [resolve_append, resolve_append, resolve_plains, resolve_plains, rg]
    exact side_ok g (Nat.min_le_right _ _) (Nat.le_max_right _ _)

/-- **Rule 3, surfaced.** When the two edits disagree, the buffer carries a
conflict node: an overlap is never silently resolved to one side. -/
theorem conflict_is_surfaced (b : List α) (o g : Edit α) (hov : overlaps o g)
    (hdiff : apply b o ≠ apply b g) :
    ¬ conflictFree (merge b o g).1 := by
  intro hcf
  have ⟨ry, rg⟩ := conflict_resolves_to_either_side b o g hov
  apply hdiff
  rw [← ry, ← rg]
  -- a conflict-free buffer resolves the same way to both sides
  have same : ∀ l : List (Seg α), conflictFree l → resolve .yours l = resolve .agent l := by
    intro l
    induction l with
    | nil => intro _; rfl
    | cons s l ih =>
        cases s with
        | plain a => intro h; simp [resolve, ih h]
        | conflict => intro h; exact absurd h id
  exact same _ hcf

/-! ### Every case: nothing is lost -/

/-- **Operator text is never lost**, in every case. Outside a conflict, the
operator's inserted text sits intact immediately before the cursor; in a
conflict, the operator's side resolves to exactly the operator's edit. -/
theorem operator_text_never_lost (b : List α) (o g : Edit α)
    (hvalid : o.pos ≤ b.length) :
    (∃ c pre post, (merge b o g).2 = some c ∧
        resolve .yours (merge b o g).1 = pre ++ o.ins ++ post ∧
        (pre ++ o.ins).length = c) ∨
      resolve .yours (merge b o g).1 = apply b o := by
  by_cases h1 : o.len = 0 ∧ g.len = 0 ∧ o.pos = g.pos
  · left
    obtain ⟨cf, hr, hc, _⟩ := same_point_agent_first b o g h1.1 h1.2.1 h1.2.2 hvalid
    refine ⟨_, b.take o.pos ++ g.ins, b.drop o.pos, hc, by simpa [List.append_assoc] using hr, ?_⟩
    simp [List.length_take, Nat.min_eq_left hvalid]; omega
  by_cases h2 : g.stop ≤ o.pos
  · left
    obtain ⟨_, mid, hr, _, hc⟩ := agent_before_applies_both b o g h1 h2 hvalid
    exact ⟨_, _, _, hc, hr, rfl⟩
  by_cases h3 : o.stop ≤ g.pos
  · left
    obtain ⟨_, mid, hr, _, hc⟩ := operator_before_applies_both b o g h1 h2 h3 hvalid
    refine ⟨_, b.take o.pos, mid ++ g.ins ++ b.drop g.stop, hc, ?_, rfl⟩
    simpa [List.append_assoc] using hr
  · right
    exact (conflict_resolves_to_either_side b o g ⟨h1, h2, h3⟩).1

/-- **Agent inserts are preserved**, in every case: outside a conflict the
agent's inserted text is in the buffer; in a conflict the agent's side
resolves to exactly the agent's edit. -/
theorem agent_insert_preserved (b : List α) (o g : Edit α) :
    (conflictFree (merge b o g).1 ∧ g.ins <:+: resolve .yours (merge b o g).1) ∨
      resolve .agent (merge b o g).1 = apply b g := by
  by_cases h1 : o.len = 0 ∧ g.len = 0 ∧ o.pos = g.pos
  · left
    simp only [merge, h1, and_self, if_true]
    refine ⟨conflictFree_plains _, ?_⟩
    rw [resolve_plains]
    exact ⟨b.take o.pos, o.ins ++ b.drop o.pos, by simp [List.append_assoc, h1.2.2]⟩
  by_cases h2 : g.stop ≤ o.pos
  · left
    simp only [merge, h1, h2, if_false, if_true]
    refine ⟨conflictFree_plains _, ?_⟩
    rw [resolve_plains]
    exact ⟨b.take g.pos, (b.drop g.stop).take (o.pos - g.stop) ++ o.ins ++ b.drop o.stop,
      by simp [List.append_assoc]⟩
  by_cases h3 : o.stop ≤ g.pos
  · left
    simp only [merge, h1, h2, h3, if_false, if_true]
    refine ⟨conflictFree_plains _, ?_⟩
    rw [resolve_plains]
    exact ⟨b.take o.pos ++ o.ins ++ (b.drop o.stop).take (g.pos - o.stop), b.drop g.stop,
      by simp [List.append_assoc]⟩
  · right
    exact (conflict_resolves_to_either_side b o g ⟨h1, h2, h3⟩).2

/-! ### Non-vacuity -/

/-- The operator-first ordering (what a naive append at the cursor gives)
breaks rule 1: the agent's content would land after the operator's text. -/
theorem operator_first_violates_rule_one :
    let b : List Nat := []
    let o : Edit Nat := ⟨0, 0, [1]⟩
    let g : Edit Nat := ⟨0, 0, [2]⟩
    (merge b o g).1 ≠ plains (b ++ o.ins ++ g.ins) := by
  decide

/-- Rule 3 is reachable: two edits of the same word produce a conflict node,
and a last-writer-wins merge would have dropped the agent's word. -/
theorem conflict_reachable :
    let b : List Nat := [1, 2, 3]
    let o : Edit Nat := ⟨1, 1, [7]⟩
    let g : Edit Nat := ⟨1, 1, [8]⟩
    (merge b o g).1 = [.plain 1, .conflict [7] [8], .plain 3] ∧
      ¬ (g.ins <:+: apply b o) := by
  decide

end ConflictReconciliation
