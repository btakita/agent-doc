/-
! Full Harel/SCXML state chart — formal model.

This is the formal counterpart of `lazily-spec/docs/state-charts.md` and the
executable reference behind the cross-language conformance fixtures. It models
the *full* subset: compound states, orthogonal (parallel) regions, shallow and
deep history, entry/exit/transition actions, named guards, and external +
internal transitions.

`send` is a total, deterministic function of `(chart, history, configuration,
event, guard resolver)` — so **confluence/determinism is by construction**: the
type of `send` is itself the proof that a given inputs tuple yields a unique
result. That is the property no finite conformance fixture suite can establish,
and it is the formal guarantee that every binding's interpreter — replaying the
shared fixtures — agrees on *all* inputs, not just the tested ones.

Proved here:
- `enabled_empty_rejects` — an event with no enabled, guard-passing transition
  leaves the configuration (and history) unchanged.
- Chart immutability holds *by construction* (Lean is pure and `send` returns a
  `StepResult` with no replacement `Chart` field), so the chart definition
  cannot be mutated by a step — not a theorem, but the strongest form of the
  guarantee.
- `sendTaken_eq_enabled_of_pairwise_disjoint` / `parallel_region_confluence` —
  full parallel-region confluence: when the enabled transitions are pairwise
  non-conflicting (their LCA exit sets are disjoint — the defining condition of
  orthogonal regions), the conflict-resolving `keepTrans` fold is transparent
  (every enabled transition is taken) and the resulting configuration is a
  function of the enabled *set* alone, invariant under any reordering of
  `enabled` (the universal confluence property no finite fixture suite can show).
- `single_region_refines_flat_machine` — a single-region chart's `send` refines
  the flat `StateMachine` kernel derived in `flatMachine`: the new active leaf
  equals the flat machine's transition target. Both the reject and take cases
  are proved under `Chart.Coherent` — the structural well-formedness (kinds
  match the tree; default descents resolve to a unique leaf; the root is a
  common ancestor) the loose `WellFormed` does not pin down. The four take-case
  consequences (`defaultLeaf` is a leaf / is in `enterSet`; the only `enterSet`
  leaf is `defaultLeaf`; and the active-leaf resolution) are *derived* from
  `Chart.Coherent`, not assumed.

Modeled (deterministic by construction) with history recording idempotence
(`recordHistory_idempotent`, proved) remaining as the deeper universal
property of that subsystem.
-/

import LazilyFormal.Primitive
import LazilyFormal.StateMachine

namespace LazilyFormal.StateChart

open LazilyFormal.Primitive

/-- Kind of a state node. -/
inductive Kind where
  | atomic
  | compound
  | parallel
  | history (deep : Bool)   -- `true` = deep, `false` = shallow
  | final
  deriving Repr

/-- A transition: a target, an optional named guard, ordered transition actions,
and the internal flag. -/
structure Transition where
  target : StateId
  guard : Option GuardId
  action : List ActionId
  internal : Bool
  deriving Repr

/-- Actions on a state: entry fires outermost-first when the state enters (after
its ancestors' entries); exit fires innermost-first when it exits (before its
ancestors' exits). -/
structure Chart where
  states : List StateId
  parent : StateId → Option StateId
  kind : StateId → Kind
  initial : StateId → Option StateId
  default : StateId → Option StateId
  on : StateId → EventId → Option Transition
  entry : StateId → List ActionId
  exit : StateId → List ActionId

/-- A history recording: history pseudo-state id → recorded leaf configuration. -/
abbrev History := StateId → Option (List StateId)

/-- The outcome of one `send`: new configuration, updated history, ordered
action trace (exit innermost-first → transition → entry outermost-first). -/
structure StepResult where
  cfg : Configuration
  history : History
  actions : List ActionId

-- --------------------------------------------------------------------- helpers

/-- The root state (the unique state with no parent; `0` for a degenerate chart). -/
def root (c : Chart) : StateId :=
  match c.states.find? (fun s => c.parent s = none) with
  | some r => r
  | none => 0

/-- Direct children of `s` in document order. -/
def children (c : Chart) (s : StateId) : List StateId :=
  c.states.filter (fun t => c.parent t = some s)

/-- Ancestors of `s` inclusive, `[s, ..., root]`, fueled by `|states|`. -/
def ancestorsAux (c : Chart) : Nat → StateId → List StateId
  | 0, s => [s]
  | n+1, s => match c.parent s with
    | none => [s]
    | some p => s :: ancestorsAux c n p

def ancestors (c : Chart) (s : StateId) : List StateId :=
  ancestorsAux c c.states.length s

/-- Depth of `s` (root = 0), fueled by `|states|`. -/
def depthAux (c : Chart) : Nat → StateId → Nat
  | 0, _ => 0
  | n+1, s => match c.parent s with
    | none => 0
    | some p => 1 + depthAux c n p

def depth (c : Chart) (s : StateId) : Nat :=
  depthAux c c.states.length s

/-- `true` iff `a` is an ancestor-or-self of `s`. -/
def isAncestorIncl (c : Chart) (a s : StateId) : Bool :=
  (ancestors c s).contains a

/-- `true` iff `desc` is a proper descendant of `anc`. -/
def isProperDescendant (c : Chart) (desc anc : StateId) : Bool :=
  (desc ≠ anc) && isAncestorIncl c anc desc

/-- Lowest common ancestor (inclusive) of `a` and `b`; falls back to the root. -/
def lca (c : Chart) (a b : StateId) : StateId :=
  match (ancestors c a).find? (fun x => isAncestorIncl c x b) with
  | some x => x
  | none => root c

/-- `true` for active-leaf kinds (atomic / final). -/
def isLeaf (c : Chart) (s : StateId) : Bool :=
  match c.kind s with
  | Kind.atomic => true
  | Kind.final => true
  | _ => false

/-- Active atomic leaves of a configuration. -/
def activeLeaves (c : Chart) (cfg : Configuration) : List StateId :=
  cfg.filter (isLeaf c)

/-- A guard passes when absent, or resolves `true` (fail-closed otherwise). -/
def guardPasses (g : GuardResolver) (t : Transition) : Bool :=
  match t.guard with
  | none => true
  | some n => g n

-- ---------------------------------------------- transition selection (enabled)

/-- Walk up from `leaf`; return the innermost passing `(source, transition)`,
or `none`. Fueled by `|states|`. -/
def enabledForLeafAux (c : Chart) (g : GuardResolver) (ev : EventId) :
    Nat → StateId → Option (StateId × Transition)
  | 0, _ => none
  | n+1, s =>
    let up : Option (StateId × Transition) :=
      match c.parent s with
      | none => none
      | some p => enabledForLeafAux c g ev n p
    match c.on s ev with
    | some t => if guardPasses g t then some (s, t) else up
    | none => up

def enabledForLeaf (c : Chart) (g : GuardResolver) (ev : EventId) (leaf : StateId) :
    Option (StateId × Transition) :=
  enabledForLeafAux c g ev c.states.length leaf

/-- All enabled `(source, transition)` pairs across every active leaf. -/
def enabled (c : Chart) (cfg : Configuration) (g : GuardResolver) (ev : EventId) :
    List (StateId × Transition) :=
  (activeLeaves c cfg).filterMap (enabledForLeaf c g ev)

-- ------------------------------------------------------------- enter / descent

/-- Enter `s` and descend to its default leaves, fueled by `|states|`. -/
def enterSubtree (c : Chart) : Nat → StateId → List StateId
  | 0, s => [s]
  | n+1, s => s :: (match c.kind s with
    | Kind.compound => match c.initial s with
      | none => []
      | some i => enterSubtree c n i
    | Kind.parallel => (children c s).flatMap (enterSubtree c n)
    | _ => [])

/-- Path from just-below `lca` down to `target` (exclusive `lca`, inclusive `target`). -/
def pathBelow (c : Chart) (lca target : StateId) : List StateId :=
  ((ancestors c target).takeWhile (fun x => x ≠ lca)).reverse

/-- The first-entry target of a region's history pseudo-state: `default`, else
the region's `initial`, else the root. (Factored out so that "stays in `states`"
is provable by a clean case split.) -/
def restoreStart (c : Chart) (hist region : StateId) : StateId :=
  match c.default hist with
  | some d => d
  | none => match c.initial region with
    | some i => i
    | none => root c

/-- Restore a region via a history target: recorded leaves if present, else the
`default` (or the region's `initial`) descent. -/
def restoreViaHistory (c : Chart) (h : History) (hist region : StateId) : List StateId :=
  match h hist with
  | some leaves => leaves
  | none => pathBelow c region (restoreStart c hist region)
      ++ enterSubtree c c.states.length (restoreStart c hist region)

/-- The owning region of a target: its parent, else the root. -/
def regionOf (c : Chart) (target : StateId) : StateId :=
  match c.parent target with | some p => p | none => root c

/-- The enter set for a transition to `target` entering below `lca`. -/
def enterSet (c : Chart) (h : History) (lca target : StateId) : List StateId :=
  match c.kind target with
  | Kind.history _ => pathBelow c lca (regionOf c target) ++ restoreViaHistory c h target (regionOf c target)
  | _ => pathBelow c lca target ++ enterSubtree c c.states.length target

/-- The exit set: active states that are proper descendants of `lca`. -/
def exitSet (c : Chart) (cfg : Configuration) (lca : StateId) : List StateId :=
  cfg.filter (fun s => isProperDescendant c s lca)

-- --------------------------------------------------------------------- history

/-- The (history-pseudo-state id → recorded leaf configuration) pairs produced by
one exit pass. Determined entirely by `c`, `exiting`, `cfg` — independent of the
prior history — which is exactly what makes [`recordHistory`] idempotent. -/
def historyRecordings (c : Chart) (exiting : List StateId) (cfg : Configuration) :
    List (StateId × List StateId) :=
  c.states.filterMap (fun hs =>
    if match c.kind hs with | Kind.history _ => true | _ => false then
      match c.parent hs with
      | none => none
      | some region =>
        if exiting.contains region then
          let below := cfg.filter (fun s => isProperDescendant c s region)
          let recorded := match c.kind hs with
            | Kind.history true => below
            | _ => below.take 1
          some (hs, recorded)
        else none
    else none)

/-- Record history for every history pseudo-state whose owning region is being
exited. Deep records the full active sub-configuration below the region;
shallow records the first (top-most) recorded descendant as an approximation. -/
def recordHistory (c : Chart) (h : History) (exiting : List StateId)
    (cfg : Configuration) : History :=
  fun s => match (historyRecordings c exiting cfg).lookup s with
    | some v => some v
    | none => h s

-- ----------------------------------------------------------------- dedup / apply

def dedup [BEq α] (xs : List α) : List α :=
  xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) []

/-- Apply one transition: exit set out, enter set in (configuration deduped). -/
def applyTransition (_c : Chart) (cfg : Configuration) (exiting entering : List StateId) :
    Configuration :=
  dedup ((cfg.filter (fun s => ¬ exiting.contains s)) ++ entering)

-- ----------------------------------------------------------- membership invariants

/-- `dedup` never invents elements: every member of `dedup xs` was in `xs`.
Proved by induction on the fold accumulator. -/
theorem dedup_subset [BEq α] (xs : List α) : ∀ x, x ∈ dedup xs → x ∈ xs := by
  suffices h : ∀ (acc : List α) (x : α),
      x ∈ xs.foldl (fun a y => if a.contains y then a else a ++ [y]) acc → x ∈ acc ∨ x ∈ xs by
    simpa [dedup] using h []
  intro acc x
  induction xs generalizing acc x with
  | nil => simp [List.foldl]
  | cons a l ih =>
    simp only [List.foldl_cons]
    intro hx
    split at hx
    · rcases ih acc x hx with h | h
      · exact Or.inl h
      · exact Or.inr (List.mem_cons_of_mem a h)
    · rcases ih (acc ++ [a]) x hx with h | h
      · rcases List.mem_append.mp h with hacc | ha
        · exact Or.inl hacc
        · exact Or.inr (by
            cases ha with
            | head => exact List.mem_cons_self
            | tail _ hp => exact absurd hp List.not_mem_nil)
      · exact Or.inr (List.mem_cons_of_mem a h)

/-- The exit set is always drawn from the active configuration. -/
theorem exitSet_subset_cfg (c : Chart) (cfg : Configuration) (lca : StateId) :
    ∀ x, x ∈ exitSet c cfg lca → x ∈ cfg :=
  fun _ hx => (List.mem_filter.mp hx).1

/-- `applyTransition`'s output is confined to the entering set ∪ the prior
configuration (it can only drop states or re-add states already visible). -/
theorem applyTransition_membership (c : Chart) (cfg : Configuration) (exiting entering : List StateId)
    (x : StateId) (hx : x ∈ applyTransition c cfg exiting entering) :
    x ∈ entering ∨ x ∈ cfg := by
  have := dedup_subset ((cfg.filter (fun s => ¬ exiting.contains s)) ++ entering) x hx
  rcases List.mem_append.mp this with h | h
  · exact Or.inr (List.mem_filter.mp h).1
  · exact Or.inl h

/-- Membership of a singleton list. -/
theorem mem_of_mem_singleton {α : Type} (a x : α) (h : x ∈ [a]) : x = a := by
  cases h with
  | head => rfl
  | tail _ hp => exact absurd hp List.not_mem_nil

/-- A fold step that only ever keeps or appends retains only accumulator or list
elements. -/
theorem foldl_subset {α : Type} (f : List α → α → List α)
    (hf : ∀ acc x y, y ∈ f acc x → y ∈ acc ∨ y = x) :
    ∀ (l : List α) (acc : List α) (y : α), y ∈ l.foldl f acc → y ∈ acc ∨ y ∈ l := by
  intro l
  induction l with
  | nil => intros acc y h; simp only [List.foldl] at h; exact Or.inl h
  | cons a t ih =>
    intros acc y h
    rw [List.foldl_cons] at h
    rcases ih (f acc a) y h with h | h
    · rcases hf acc a y h with hacc | heq
      · exact Or.inl hacc
      · subst heq; exact Or.inr List.mem_cons_self
    · exact Or.inr (List.mem_cons_of_mem a h)

-- ----------------------------------------------------------- well-formedness

/-- A chart is well-formed when every structural pointer — parent, initial,
default, and each transition target — stays inside the declared `states`
universe, and the root is declared. Under `WellFormed`, `send` preserves
"configuration ⊆ `states`" (see `send_cfg_in_states`). -/
structure Chart.WellFormed (c : Chart) : Prop where
  parent_in  : ∀ ⦃s p⦄, s ∈ c.states → c.parent s = some p → p ∈ c.states
  initial_in : ∀ ⦃s i⦄, s ∈ c.states → c.initial s = some i → i ∈ c.states
  default_in : ∀ ⦃s d⦄, s ∈ c.states → c.default s = some d → d ∈ c.states
  target_in  : ∀ ⦃s ev t⦄, s ∈ c.states → c.on s ev = some t → t.target ∈ c.states
  root_in    : root c ∈ c.states

/-- A history is well-formed when every recording it holds is confined to
`states`. `recordHistory` preserves this (it records only active
configuration, which is ⊆ `states` under `WellFormed`). -/
def history_wf (c : Chart) (h : History) : Prop :=
  ∀ s l, h s = some l → (∀ x ∈ l, x ∈ c.states)

/-- Direct children are declared states. -/
theorem children_in_states (c : Chart) (s t : StateId) (ht : t ∈ children c s) :
    t ∈ c.states :=
  (List.mem_filter.mp ht).1

/-- Active leaves are drawn from the configuration. -/
theorem activeLeaves_in_cfg (c : Chart) (cfg : Configuration) (x : StateId)
    (hx : x ∈ activeLeaves c cfg) : x ∈ cfg :=
  (List.mem_filter.mp hx).1

/-- The ancestor chain of a declared state stays inside `states` (fueled). -/
theorem ancestorsAux_in_states (c : Chart) (wf : c.WellFormed) :
    ∀ (n : Nat) (s : StateId), s ∈ c.states →
      ∀ x, x ∈ ancestorsAux c n s → x ∈ c.states := by
  intro n
  induction n with
  | zero =>
    intros s hs x hx
    simp only [ancestorsAux] at hx
    cases hx with
    | head => exact hs
    | tail _ hp => exact absurd hp List.not_mem_nil
  | succ n ih =>
    intros s hs x hx
    simp only [ancestorsAux] at hx
    cases hps : c.parent s with
    | none =>
      simp only [hps] at hx
      cases hx with
      | head => exact hs
      | tail _ hp => exact absurd hp List.not_mem_nil
    | some p =>
      simp only [hps] at hx
      cases hx with
      | head => exact hs
      | tail _ hin => exact ih p (wf.parent_in hs hps) x hin

theorem ancestors_in_states (c : Chart) (wf : c.WellFormed) {s : StateId}
    (hs : s ∈ c.states) {x : StateId} (hx : x ∈ ancestors c s) : x ∈ c.states :=
  ancestorsAux_in_states c wf c.states.length s hs x hx

/-- Entering a declared state descends only through declared states (fueled). -/
theorem enterSubtree_in_states (c : Chart) (wf : c.WellFormed) :
    ∀ (n : Nat) (s : StateId), s ∈ c.states →
      ∀ x, x ∈ enterSubtree c n s → x ∈ c.states := by
  intro n
  induction n with
  | zero =>
    intros s hs x hx
    simp only [enterSubtree] at hx
    cases hx with
    | head => exact hs
    | tail _ hp => exact absurd hp List.not_mem_nil
  | succ n ih =>
    intros s hs x hx
    simp only [enterSubtree] at hx
    cases hx with
    | head => exact hs
    | tail _ hin =>
      cases hkind : c.kind s with
      | atomic => simp only [hkind] at hin; exact absurd hin List.not_mem_nil
      | compound =>
        simp only [hkind] at hin
        cases hinit : c.initial s with
        | none => simp only [hinit] at hin; exact absurd hin List.not_mem_nil
        | some i => simp only [hinit] at hin; exact ih i (wf.initial_in hs hinit) x hin
      | parallel =>
        simp only [hkind] at hin
        rcases List.mem_flatMap.mp hin with ⟨t, ht, hxt⟩
        exact ih t (children_in_states c s t ht) x hxt
      | history deep => simp only [hkind] at hin; exact absurd hin List.not_mem_nil
      | final => simp only [hkind] at hin; exact absurd hin List.not_mem_nil

theorem pathBelow_in_states (c : Chart) (wf : c.WellFormed) {lca target : StateId}
    (htarget : target ∈ c.states) {x : StateId} (hx : x ∈ pathBelow c lca target) :
    x ∈ c.states := by
  simp only [pathBelow, List.mem_reverse] at hx
  have key : ∀ (p : StateId → Bool) (l : List StateId) (y : StateId), y ∈ l.takeWhile p → y ∈ l := by
    intro p l
    induction l with
    | nil => intro y h; exact absurd h List.not_mem_nil
    | cons a t ih =>
      intro y h
      simp only [List.takeWhile_cons] at h
      by_cases pa : p a
      · simp only [pa, if_true] at h
        rcases List.mem_cons.mp h with heq | hin
        · rw [heq]; exact List.mem_cons_self
        · exact List.mem_cons_of_mem a (ih y hin)
      · simp only [pa] at h; exact absurd h List.not_mem_nil
  exact ancestors_in_states c wf htarget (key _ _ x hx)

/-- The history first-entry start is a declared state. -/
theorem restoreStart_in_states (c : Chart) (wf : c.WellFormed) {hist region : StateId}
    (hhist : hist ∈ c.states) (hregion : region ∈ c.states) :
    restoreStart c hist region ∈ c.states := by
  unfold restoreStart
  cases hdd : c.default hist with
  | none =>
    cases hii : c.initial region with
    | none => exact wf.root_in
    | some i => exact wf.initial_in hregion hii
  | some d => exact wf.default_in hhist hdd

theorem restoreViaHistory_in_states (c : Chart) (wf : c.WellFormed) (h : History)
    (hwf : history_wf c h) {hist region : StateId}
    (hhist : hist ∈ c.states) (hregion : region ∈ c.states)
    (x : StateId) (hx : x ∈ restoreViaHistory c h hist region) : x ∈ c.states := by
  cases hh : h hist with
  | some leaves =>
    simp only [restoreViaHistory, hh] at hx
    exact hwf hist leaves hh x hx
  | none =>
    simp only [restoreViaHistory, hh] at hx
    have hstart := restoreStart_in_states c wf hhist hregion
    rcases List.mem_append.mp hx with hpx | hpx
    · exact pathBelow_in_states c wf hstart hpx
    · exact enterSubtree_in_states c wf _ _ hstart x hpx

/-- The owning region of a target is a declared state when the target is. -/
theorem regionOf_in_states (c : Chart) (wf : c.WellFormed) {target : StateId}
    (htarget : target ∈ c.states) : regionOf c target ∈ c.states := by
  unfold regionOf
  cases hp : c.parent target with
  | none => exact wf.root_in
  | some p => exact wf.parent_in htarget hp

/-- The enter set for a transition into a declared target stays in `states`. -/
theorem enterSet_in_states (c : Chart) (wf : c.WellFormed) (h : History) (hwf : history_wf c h)
    {lca target : StateId} (htarget : target ∈ c.states)
    (x : StateId) (hx : x ∈ enterSet c h lca target) : x ∈ c.states := by
  cases hkind : c.kind target with
  | history deep =>
    simp only [enterSet, hkind] at hx
    have hregion := regionOf_in_states c wf htarget
    rcases List.mem_append.mp hx with hpx | hpx
    · exact pathBelow_in_states c wf hregion hpx
    · exact restoreViaHistory_in_states c wf h hwf htarget hregion x hpx
  | atomic =>
    simp only [enterSet, hkind] at hx
    rcases List.mem_append.mp hx with hpx | hpx
    · exact pathBelow_in_states c wf htarget hpx
    · exact enterSubtree_in_states c wf _ _ htarget x hpx
  | compound =>
    simp only [enterSet, hkind] at hx
    rcases List.mem_append.mp hx with hpx | hpx
    · exact pathBelow_in_states c wf htarget hpx
    · exact enterSubtree_in_states c wf _ _ htarget x hpx
  | parallel =>
    simp only [enterSet, hkind] at hx
    rcases List.mem_append.mp hx with hpx | hpx
    · exact pathBelow_in_states c wf htarget hpx
    · exact enterSubtree_in_states c wf _ _ htarget x hpx
  | final =>
    simp only [enterSet, hkind] at hx
    rcases List.mem_append.mp hx with hpx | hpx
    · exact pathBelow_in_states c wf htarget hpx
    · exact enterSubtree_in_states c wf _ _ htarget x hpx

/-- An enabled transition's source and target are declared states (fueled). -/
theorem enabledForLeafAux_in_states (c : Chart) (wf : c.WellFormed) (g : GuardResolver)
    (ev : EventId) :
    ∀ (n : Nat) (s : StateId), s ∈ c.states →
      ∀ st, enabledForLeafAux c g ev n s = some st →
        st.1 ∈ c.states ∧ st.2.target ∈ c.states := by
  intro n
  induction n with
  | zero => intros s hs st h; simp only [enabledForLeafAux] at h; nomatch h
  | succ n ih =>
    intros s hs st h
    simp only [enabledForLeafAux] at h
    cases hps : c.parent s with
    | none =>
      simp only [hps] at h
      cases hon : c.on s ev with
      | none => simp only [hon] at h; nomatch h
      | some t =>
        simp only [hon] at h
        split at h
        · injection h with hst; subst hst
          exact ⟨hs, wf.target_in hs hon⟩
        · nomatch h
    | some p =>
      simp only [hps] at h
      cases hon : c.on s ev with
      | none => simp only [hon] at h; exact ih p (wf.parent_in hs hps) st h
      | some t =>
        simp only [hon] at h
        split at h
        · injection h with hst; subst hst
          exact ⟨hs, wf.target_in hs hon⟩
        · exact ih p (wf.parent_in hs hps) st h

/-- Every enabled `(source, transition)` has declared source and target. -/
theorem enabled_in_states (c : Chart) (wf : c.WellFormed) (cfg : Configuration)
    (g : GuardResolver) (ev : EventId) (hcfg : ∀ x ∈ cfg, x ∈ c.states)
    (st : StateId × Transition) (hst : st ∈ enabled c cfg g ev) :
    st.1 ∈ c.states ∧ st.2.target ∈ c.states := by
  simp only [enabled, List.mem_filterMap] at hst
  rcases hst with ⟨leaf, hleaf, hen⟩
  have hleaf_states : leaf ∈ c.states := hcfg leaf (activeLeaves_in_cfg c cfg leaf hleaf)
  exact enabledForLeafAux_in_states c wf g ev c.states.length leaf hleaf_states st hen

-- --------------------------------------------------------------- action trace

/-- Exit actions for an exit set, fired innermost-first (deepest first):
`exit s` for each exited state `s`, ordered by descending depth. -/
def exitActions (c : Chart) (exiting : List StateId) : List ActionId :=
  (exiting.mergeSort (fun a b => decide (depth c a ≥ depth c b))).flatMap c.exit

/-- Entry actions for an enter set, fired outermost-first (shallowest first):
`entry s` for each entered state `s`, ordered by ascending depth. -/
def entryActions (c : Chart) (entering : List StateId) : List ActionId :=
  (entering.mergeSort (fun a b => decide (depth c a ≤ depth c b))).flatMap c.entry

/-- Transition actions for the taken set, in the deterministic taken order. -/
def transitionActions (taken : List (StateId × Transition)) : List ActionId :=
  taken.flatMap (fun st => st.2.action)

/-- The full step action trace: exit (innermost-first) → transition → entry
(outermost-first), per `lazily-spec/docs/state-charts.md`. -/
def stepActions (c : Chart) (exiting : List StateId) (taken : List (StateId × Transition))
    (entering : List StateId) : List ActionId :=
  exitActions c exiting ++ transitionActions taken ++ entryActions c entering

-- ------------------------------------------------------------------------- send

/-- The LCA used by a transition: the source itself for an internal-to-source
transition, else the LCA of the active leaf and the target. -/
def lcaOf (c : Chart) (cfg : Configuration) (st : StateId × Transition) : StateId :=
  if st.2.internal && (st.2.target = st.1 ∨ isProperDescendant c st.2.target st.1)
  then st.1 else lca c (activeLeaves c cfg).head! st.2.target

/-- Conflict-resolving fold step: drop `st` if its exit set intersects the prior
taken exit sets, else append it. -/
def keepTrans (c : Chart) (cfg : Configuration)
    (acc : List (StateId × Transition)) (st : StateId × Transition) :
    List (StateId × Transition) :=
  if (exitSet c cfg (lcaOf c cfg st)).any
      (fun x => (acc.flatMap (fun s => exitSet c cfg (lcaOf c cfg s))).contains x)
  then acc else acc ++ [st]

/-- The transitions taken in one `send`, after conflict resolution. -/
def sendTaken (c : Chart) (cfg : Configuration) (g : GuardResolver) (ev : EventId) :
    List (StateId × Transition) :=
  (enabled c cfg g ev).foldl (keepTrans c cfg) []

/-- The total exit set of one `send`. -/
def sendExiting (c : Chart) (cfg : Configuration) (g : GuardResolver) (ev : EventId) :
    List StateId :=
  (sendTaken c cfg g ev).map (lcaOf c cfg) |>.flatMap (exitSet c cfg)

/-- The total enter set of one `send`. -/
def sendEntering (c : Chart) (cfg : Configuration) (h : History) (g : GuardResolver) (ev : EventId) :
    List StateId :=
  (sendTaken c cfg g ev).zip ((sendTaken c cfg g ev).map (lcaOf c cfg)) |>.flatMap
    (fun pair => enterSet c h pair.2 pair.1.2.target)

/-- Run-to-completion `send`. Computes enabled transitions, resolves conflicts
(exit sets must be disjoint — taken greedily in the deterministic enabled
order), records history for exited regions, and applies the unioned exit/enter
sets. Deterministic by construction: a total function of `(chart, history,
configuration, event, guards)`, so a given input tuple yields a unique
`StepResult` — the confluence guarantee every binding inherits by replaying the
shared fixtures. -/
def send (c : Chart) (cfg : Configuration) (h : History) (g : GuardResolver)
    (ev : EventId) : StepResult :=
  if (enabled c cfg g ev).isEmpty then
    { cfg := cfg, history := h, actions := [] }
  else
    let taken := sendTaken c cfg g ev
    let exiting := sendExiting c cfg g ev
    let entering := sendEntering c cfg h g ev
    let newHistory := recordHistory c h exiting cfg
    let newCfg := applyTransition c cfg exiting entering
    let actions := stepActions c exiting taken entering
    { cfg := newCfg, history := newHistory, actions := actions }

-- -------------------------------------------------------------- theorems

/-- An event with no enabled, guard-passing transition is rejected: `send`
leaves the configuration and history untouched. -/
theorem enabled_empty_rejects
    (c : Chart) (cfg : Configuration) (h : History) (g : GuardResolver) (ev : EventId)
    (hempty : enabled c cfg g ev = []) :
    (send c cfg h g ev).cfg = cfg ∧ (send c cfg h g ev).history = h := by
  simp [send, hempty]

/-- Every retained taken transition is still an enabled one (the conflict-resolving
`keepTrans` only drops elements, never invents them). -/
theorem sendTaken_subset_enabled (c : Chart) (cfg : Configuration) (g : GuardResolver) (ev : EventId)
    (st : StateId × Transition) (h : st ∈ sendTaken c cfg g ev) :
    st ∈ enabled c cfg g ev := by
  have keep_mem (acc : List (StateId × Transition)) (s : StateId × Transition) (y : StateId × Transition)
      (hy : y ∈ keepTrans c cfg acc s) : y ∈ acc ∨ y = s := by
    unfold keepTrans at hy
    split at hy
    · exact Or.inl hy
    · rcases List.mem_append.mp hy with h | h
      · exact Or.inl h
      · exact Or.inr (mem_of_mem_singleton _ _ h)
  unfold sendTaken at h
  exact (foldl_subset _ keep_mem _ _ _ h).resolve_left (by simp)

/-- `pair.fst` of a zip element is in the first list. -/
theorem mem_zip_fst {α β : Type} (l₁ : List α) (l₂ : List β) (p : α × β)
    (h : p ∈ l₁.zip l₂) : p.1 ∈ l₁ := by
  induction l₁ generalizing l₂ with
  | nil => simp [List.zip] at h
  | cons a t ih =>
    cases l₂ with
    | nil => simp [List.zip] at h
    | cons b u =>
      simp only [List.zip] at h
      rcases List.mem_cons.mp h with heq | hin
      · subst heq; exact List.mem_cons_self
      · exact List.mem_cons_of_mem a (ih u hin)

/-- A `send` over a `WellFormed` chart keeps the configuration inside `states`. -/
theorem send_cfg_in_states (c : Chart) (cfg : Configuration) (h : History) (g : GuardResolver)
    (ev : EventId) (wf : c.WellFormed) (hcfg : ∀ x ∈ cfg, x ∈ c.states)
    (hwf : history_wf c h) :
    ∀ x ∈ (send c cfg h g ev).cfg, x ∈ c.states := by
  intro x hx
  by_cases hen : enabled c cfg g ev = []
  · have rej := enabled_empty_rejects c cfg h g ev hen
    rw [rej.1] at hx
    exact hcfg x hx
  · -- at least one transition taken: the else-branch fires.
    have hne : (enabled c cfg g ev).isEmpty = false := by
      cases h : enabled c cfg g ev with
      | nil => exact absurd h hen
      | cons hd tl => rfl
    simp only [send, hne] at hx
    have mem := applyTransition_membership c cfg (sendExiting c cfg g ev) (sendEntering c cfg h g ev) x hx
    rcases mem with henter | hcfgx
    · -- x ∈ sendEntering c cfg h g ev
      have hx' : x ∈ sendEntering c cfg h g ev := henter
      rw [sendEntering] at hx'
      rcases (List.mem_flatMap.mp hx') with ⟨pair, hpair, hpx⟩
      have htk := mem_zip_fst _ _ _ hpair
      have hen := sendTaken_subset_enabled c cfg g ev pair.1 htk
      have htarget := (enabled_in_states c wf cfg g ev hcfg pair.1 hen).2
      exact enterSet_in_states c wf h hwf htarget x hpx
    · exact hcfg x hcfgx

/-- Recording the same exit pass twice is a no-op. `historyRecordings` depends
only on `c`/`exiting`/`cfg`, so the second pass overwrites each recorded entry
with the identical value and falls back to the already-recorded value
(equivalently the prior history) everywhere else. -/
theorem recordHistory_idempotent (c : Chart) (h : History) (exiting : List StateId)
    (cfg : Configuration) :
    recordHistory c (recordHistory c h exiting cfg) exiting cfg = recordHistory c h exiting cfg := by
  funext s
  simp only [recordHistory]
  cases hh : (historyRecordings c exiting cfg).lookup s <;> simp

/-- The action trace is empty precisely when an event is rejected (no enabled,
guard-passing transition). On the taken branch, [`stepActions_sourcing`] gives
the sourcing invariant. -/
theorem send_actions_empty_when_rejected (c : Chart) (cfg : Configuration) (h : History)
    (g : GuardResolver) (ev : EventId)
    (hempty : enabled c cfg g ev = []) : (send c cfg h g ev).actions = [] := by
  simp [send, hempty]

/-- Sourcing invariant for the computed action trace: every fired action is the
`exit` of an exited state, the `action` of a taken transition, or the `entry`
of an entered state — the exit → transition → entry shape fixed by
`lazily-spec/docs/state-charts.md`. Sorting only reorders within a part, so it
preserves sourcing (via `List.mem_mergeSort`). -/
theorem stepActions_sourcing (c : Chart) (exiting : List StateId) (taken : List (StateId × Transition))
    (entering : List StateId) (a : ActionId)
    (ha : a ∈ stepActions c exiting taken entering) :
    (∃ s ∈ exiting, a ∈ c.exit s) ∨
    (∃ st ∈ taken, a ∈ st.2.action) ∨
    (∃ s ∈ entering, a ∈ c.entry s) := by
  have hexit : a ∈ exitActions c exiting ↔ ∃ s ∈ exiting, a ∈ c.exit s := by
    unfold exitActions
    simp only [List.mem_flatMap, List.mem_mergeSort]
  have htrans : a ∈ transitionActions taken ↔ ∃ st ∈ taken, a ∈ st.2.action := by
    simp only [transitionActions, List.mem_flatMap]
  have henter : a ∈ entryActions c entering ↔ ∃ s ∈ entering, a ∈ c.entry s := by
    unfold entryActions
    simp only [List.mem_flatMap, List.mem_mergeSort]
  unfold stepActions at ha
  rcases List.mem_append.mp ha with h | h
  · rcases List.mem_append.mp h with h | h
    · exact Or.inl (hexit.mp h)
    · exact Or.inr (Or.inl (htrans.mp h))
  · exact Or.inr (Or.inr (henter.mp h))

/-- Single-region confluence precursor: with exactly one active leaf, the
enabled set has length ≤ 1, so `send` takes at most one transition — the chart
is deterministic on a single region. This is the key step toward
`single_region_refines_flat_machine` (a single leaf ⇒ at most one enabled ⇒ the
greedy `keep` keeps at most one ⇒ the result is independent of `enabled` order).
The full leaf-equivalence proof (the new active leaf equals the flat
`LazilyFormal.StateMachine` transition's target) additionally requires unfolding
`send`'s LCA/exit/enter machinery under the single-region hypotheses and is
staged as follow-up. -/
theorem single_region_enabled_at_most_one
    (c : Chart) (cfg : Configuration) (_h : History) (g : GuardResolver) (ev : EventId)
    (leaf : StateId)
    (_singleRegion : ∀ s ∈ c.states,
       c.kind s ≠ Kind.parallel ∧ c.kind s ≠ Kind.history true ∧
       c.kind s ≠ Kind.history false)
    (oneLeaf : activeLeaves c cfg = [leaf]) :
    (enabled c cfg g ev).length ≤ 1 := by
  rw [enabled, oneLeaf]
  cases he : enabledForLeaf c g ev leaf <;> simp [he]

-- ============================================================ confluence helpers

/-- The exit set of one taken transition: the active states below its LCA. -/
def transExitSet (c : Chart) (cfg : Configuration) (st : StateId × Transition) : List StateId :=
  exitSet c cfg (lcaOf c cfg st)

/-- The enter set of one taken transition, into its target below its LCA. -/
def transEnterSet (c : Chart) (cfg : Configuration) (h : History) (st : StateId × Transition) :
    List StateId :=
  enterSet c h (lcaOf c cfg st) st.2.target

/-- Union of exit sets for a list of taken transitions — exactly the combined
exit set the `keepTrans` fold consults. -/
def combinedExitSet (c : Chart) (cfg : Configuration) (acc : List (StateId × Transition)) :
    List StateId :=
  acc.flatMap (transExitSet c cfg)

/-- A `dedup` fold step either keeps the accumulator (when the element is already
present) or appends; in both cases an accumulator member is retained, and the
folded element itself lands in the result. -/
theorem foldl_dedupStep_mem (xs : List StateId) (acc : List StateId) (x : StateId)
    (h : x ∈ acc ∨ x ∈ xs) : x ∈
      xs.foldl (fun a y => if a.contains y then a else a ++ [y]) acc := by
  induction xs generalizing acc with
  | nil =>
    rcases h with ha | haxs
    · exact ha
    · exact absurd haxs (by simp)
  | cons b m ih =>
    simp only [List.foldl_cons]
    have step_mem_acc (hxa : x ∈ acc) :
        x ∈ (if acc.contains b then acc else acc ++ [b]) := by
      by_cases hc : acc.contains b = true
      · rw [if_pos hc]; exact hxa
      · rw [if_neg hc, List.mem_append]; exact Or.inl hxa
    have step_mem_self :
        b ∈ (if acc.contains b then acc else acc ++ [b]) := by
      by_cases hc : acc.contains b = true
      · rw [if_pos hc]; exact List.contains_iff_mem.mp hc
      · rw [if_neg hc, List.mem_append]; exact Or.inr List.mem_cons_self
    refine ih _ ?_
    rcases h with ha | hbmx
    · exact Or.inl (step_mem_acc ha)
    · rcases List.mem_cons.mp hbmx with heq | hm
      · subst heq; exact Or.inl step_mem_self
      · exact Or.inr hm

/-- `dedup` keeps every member of its input (the reverse direction of
[`dedup_subset`]); combined with [`dedup_subset`] this gives membership
equivalence, the order-free view of a configuration. -/
theorem mem_dedup_iff (xs : List StateId) (x : StateId) : x ∈ dedup xs ↔ x ∈ xs := by
  constructor
  · exact dedup_subset xs x
  · intro hx
    simp only [dedup]
    exact foldl_dedupStep_mem xs [] x (Or.inr hx)

/-- `keepTrans` appends `st` (rather than dropping it) whenever `st`'s exit set
is disjoint from the accumulator's combined exit set. -/
theorem keepTrans_appends_of_disjoint
    (c : Chart) (cfg : Configuration) (acc : List (StateId × Transition)) (st : StateId × Transition)
    (hdis : ∀ x, x ∈ transExitSet c cfg st → x ∉ combinedExitSet c cfg acc) :
    keepTrans c cfg acc st = acc ++ [st] := by
  simp only [keepTrans]
  by_cases hc : (exitSet c cfg (lcaOf c cfg st)).any
      (fun x => (acc.flatMap (fun s => exitSet c cfg (lcaOf c cfg s))).contains x)
  · have := List.any_eq_true.mp hc
    rcases this with ⟨y, hy_mem, hy_in⟩
    exact absurd (List.contains_iff_mem.mp hy_in) (hdis y hy_mem)
  · rw [if_neg hc]

/-- The conflict-resolving fold retains every transition when (i) the input list
is pairwise non-conflicting and (ii) the seed accumulator's combined exit set is
disjoint from each input transition's exit set. All hypotheses live in the goal
so induction leaves a clean, fully-general `ih`. -/
theorem foldl_keepTrans_appends_all
    (c : Chart) (cfg : Configuration) (ts : List (StateId × Transition))
    (hpair : ts.Pairwise (fun a b => ∀ x, x ∈ transExitSet c cfg a → x ∉ transExitSet c cfg b))
    (acc : List (StateId × Transition))
    (hacc : ∀ b ∈ ts, ∀ x, x ∈ combinedExitSet c cfg acc → x ∉ transExitSet c cfg b) :
    ts.foldl (keepTrans c cfg) acc = acc ++ ts := by
  induction ts generalizing acc with
  | nil => simp [List.foldl]
  | cons b m ih =>
    have hpair_m : m.Pairwise (fun a b => ∀ x, x ∈ transExitSet c cfg a → x ∉ transExitSet c cfg b) :=
      (List.pairwise_cons.mp hpair).2
    have hb_dis : ∀ x, x ∈ transExitSet c cfg b → x ∉ combinedExitSet c cfg acc := by
      intro x hx_in hx_acc
      exact hacc b List.mem_cons_self x hx_acc hx_in
    have hb : keepTrans c cfg acc b = acc ++ [b] :=
      keepTrans_appends_of_disjoint c cfg acc b hb_dis
    rw [List.foldl_cons, hb]
    have hacc_m : ∀ b' ∈ m, ∀ x,
        x ∈ combinedExitSet c cfg (acc ++ [b]) → x ∉ transExitSet c cfg b' := by
      intro b' hb'm x hx
      have hflat : combinedExitSet c cfg (acc ++ [b]) =
          combinedExitSet c cfg acc ++ transExitSet c cfg b := by
        simp [combinedExitSet, List.flatMap_append]
      rw [hflat] at hx
      rcases List.mem_append.mp hx with ha | hbx
      · exact hacc b' (List.mem_cons_of_mem _ hb'm) x ha
      · exact (List.pairwise_cons.mp hpair).1 b' hb'm x hbx
    have := @ih hpair_m (acc ++ [b]) hacc_m
    rw [this, List.append_assoc, List.cons_append, List.nil_append]

/-- **Parallel-region confluence, part 1 — order-transparent conflict
resolution.** When every pair of enabled transitions is non-conflicting (their
LCA exit sets are pairwise disjoint — precisely the condition that holds for
transitions sourced in distinct orthogonal regions), the greedy `keepTrans`
fold is the identity: it takes *every* enabled transition and drops none. Hence
no enabled transition can be lost or won by reordering `enabled`; the taken set
is the whole enabled set. -/
theorem sendTaken_eq_enabled_of_pairwise_disjoint
    (c : Chart) (cfg : Configuration) (g : GuardResolver) (ev : EventId)
    (hpair : (enabled c cfg g ev).Pairwise
        (fun a b => ∀ x, x ∈ transExitSet c cfg a → x ∉ transExitSet c cfg b)) :
    sendTaken c cfg g ev = enabled c cfg g ev := by
  have := foldl_keepTrans_appends_all c cfg (enabled c cfg g ev) hpair []
    (by simp [combinedExitSet])
  simpa [sendTaken] using this

-- ---------------------------------------------------------- take-all semantics

/-- The configuration produced by applying a given taken set — factored out of
[`send`] (and matching its `cfg` computation definitionally) so confluence can
be stated over an explicit list. -/
def applyTakenCfg (c : Chart) (cfg : Configuration) (h : History)
    (taken : List (StateId × Transition)) : Configuration :=
  applyTransition c cfg
    (taken.map (lcaOf c cfg) |>.flatMap (exitSet c cfg))
    (taken.zip (taken.map (lcaOf c cfg)) |>.flatMap
      (fun p => enterSet c h p.2 p.1.2.target))

/-- `l.zip (l.map f)` is exactly the list of `(x, f x)` pairs. -/
theorem zip_map_eq {α β : Type} (l : List α) (f : α → β) :
    l.zip (l.map f) = l.map (fun x => (x, f x)) := by
  induction l with
  | nil => rfl
  | cons a m ih =>
    show (a, f a) :: m.zip (List.map f m) = (a, f a) :: List.map (fun x => (x, f x)) m
    rw [ih]

/-- Membership of a `cfg.filter (fun s => ¬ exiting.contains s)`: the decidable
filter on the `¬ Bool` predicate is exactly "in cfg and not in exiting". -/
theorem filter_notContains_mem (cfg exiting : List StateId) (x : StateId) :
    x ∈ cfg.filter (fun s => ¬ exiting.contains s) ↔ x ∈ cfg ∧ x ∉ exiting := by
  simp only [List.mem_filter, List.contains_iff_mem, decide_eq_true_eq]

/-- Membership of [`applyTakenCfg`] is characterized purely in terms of the
*set* of taken transitions — nowhere does it depend on list order. This
order-free view is what makes parallel-region confluence a corollary. -/
theorem applyTakenCfg_mem_iff
    (c : Chart) (cfg : Configuration) (h : History)
    (taken : List (StateId × Transition)) (x : StateId) :
    x ∈ applyTakenCfg c cfg h taken ↔
      (x ∈ cfg ∧ x ∉ taken.flatMap (transExitSet c cfg)) ∨
      ∃ st ∈ taken, x ∈ transEnterSet c cfg h st := by
  have hexit_eq : (taken.map (lcaOf c cfg)).flatMap (exitSet c cfg) =
      taken.flatMap (transExitSet c cfg) := by rw [List.flatMap_map]; rfl
  have henter_eq : (taken.zip (taken.map (lcaOf c cfg))).flatMap
      (fun p => enterSet c h p.2 p.1.2.target) = taken.flatMap (transEnterSet c cfg h) := by
    rw [zip_map_eq, List.flatMap_map]; rfl
  simp only [applyTakenCfg, applyTransition, hexit_eq, henter_eq, mem_dedup_iff, List.mem_append]
  constructor
  · rintro (hf | he)
    · exact Or.inl ((filter_notContains_mem _ _ _).mp hf)
    · rw [List.mem_flatMap] at he
      rcases he with ⟨p, hp, hpx⟩
      exact Or.inr ⟨p, hp, hpx⟩
  · rintro (⟨hxcfg, hxnex⟩ | ⟨st, hst, hxenter⟩)
    · exact Or.inl ((filter_notContains_mem _ _ _).mpr ⟨hxcfg, hxnex⟩)
    · refine Or.inr ?_
      rw [List.mem_flatMap]
      exact ⟨st, hst, hxenter⟩

/-- `flatMap` membership is invariant under permutation of the source list. -/
theorem perm_flatMap_mem {α β : Type} (f : α → List β) {l₁ l₂ : List α}
    (hp : l₁.Perm l₂) (x : β) : x ∈ l₁.flatMap f ↔ x ∈ l₂.flatMap f := by
  rw [List.mem_flatMap, List.mem_flatMap]
  exact exists_congr (fun y => and_congr hp.mem_iff Iff.rfl)

/-- In the take-branch, `send`'s configuration equals [`applyTakenCfg`] applied
to the taken set (definitionally, after unfolding the `send` `let`s). -/
theorem send_cfg_eq_applyTakenCfg_take
    (c : Chart) (cfg : Configuration) (h : History) (g : GuardResolver) (ev : EventId)
    (hne : (enabled c cfg g ev).isEmpty = false) :
    (send c cfg h g ev).cfg = applyTakenCfg c cfg h (sendTaken c cfg g ev) := by
  simp only [send, hne, applyTakenCfg]
  rfl

/-- **Parallel-region confluence — result independent of enabled order.**
Under the pairwise-disjoint hypothesis (the defining condition of orthogonal
regions: the LCA exit sets of enabled transitions are mutually disjoint), (i)
the conflict resolver is transparent — every enabled transition is taken
([`sendTaken_eq_enabled_of_pairwise_disjoint`]); and (ii) the resulting
configuration depends only on the *set* of enabled transitions, not their
order. Concretely, for *any* permutation `ts` of `enabled`, `send`'s
configuration has exactly the same elements as the take-all configuration
computed from `ts`. This is the universal confluence property no finite fixture
suite can establish: for parallel regions the active set after `send` is a
function of the enabled *set* alone. -/
theorem parallel_region_confluence
    (c : Chart) (cfg : Configuration) (h : History) (g : GuardResolver) (ev : EventId)
    (hpair : (enabled c cfg g ev).Pairwise
        (fun a b => ∀ x, x ∈ transExitSet c cfg a → x ∉ transExitSet c cfg b)) :
    sendTaken c cfg g ev = enabled c cfg g ev ∧
    ∀ (ts : List (StateId × Transition)) (_hperm : ts.Perm (enabled c cfg g ev)) (x : StateId),
      x ∈ (send c cfg h g ev).cfg ↔ x ∈ applyTakenCfg c cfg h ts := by
  refine ⟨sendTaken_eq_enabled_of_pairwise_disjoint c cfg g ev hpair, ?_⟩
  intro ts hperm x
  by_cases hen : enabled c cfg g ev = []
  · -- reject branch: `send` leaves cfg unchanged; `ts` permutes `[]` ⇒ `ts = []`.
    have hts : ts = [] := by
      have h0 : (enabled c cfg g ev).length = 0 := by rw [hen]; rfl
      have hlen : ts.length = 0 := by rw [hperm.length_eq, h0]
      cases ts with
      | nil => rfl
      | cons hd tl => simp only [List.length_cons] at hlen; omega
    subst hts
    have hsend : (send c cfg h g ev).cfg = cfg := (enabled_empty_rejects c cfg h g ev hen).1
    rw [hsend, applyTakenCfg_mem_iff]
    simp
  · -- take branch: send.cfg = applyTakenCfg (sendTaken) = applyTakenCfg (enabled).
    have hne : (enabled c cfg g ev).isEmpty = false := by
      cases hcase : (enabled c cfg g ev) with
      | nil => exact absurd hcase hen
      | cons hd tl => simp
    have hst : sendTaken c cfg g ev = enabled c cfg g ev :=
      sendTaken_eq_enabled_of_pairwise_disjoint c cfg g ev hpair
    have hsend : (send c cfg h g ev).cfg = applyTakenCfg c cfg h (enabled c cfg g ev) := by
      rw [send_cfg_eq_applyTakenCfg_take c cfg h g ev hne, hst]
    rw [hsend, applyTakenCfg_mem_iff]
    rw [applyTakenCfg_mem_iff]
    -- both sides now in order-free form; bridge the `ts ↔ enabled` set equality.
    constructor
    · rintro (⟨hxcfg, hxnex⟩ | ⟨st, hst_in, hxenter⟩)
      · exact Or.inl ⟨hxcfg, fun hx => hxnex ((perm_flatMap_mem (transExitSet c cfg) hperm x).mp hx)⟩
      · exact Or.inr ⟨st, hperm.symm.subset hst_in, hxenter⟩
    · rintro (⟨hxcfg, hxnex⟩ | ⟨st, hst_in, hxenter⟩)
      · exact Or.inl ⟨hxcfg, fun hx => hxnex ((perm_flatMap_mem (transExitSet c cfg) hperm x).mpr hx)⟩
      · exact Or.inr ⟨st, hperm.subset hst_in, hxenter⟩

-- ------------------------------------------------ single-region refinement

/-- The default-descent leaf of `s`: the deepest state a fresh entry into `s`
lands on. Atomic/final → itself; compound → the `initial` chain's leaf. Under
the single-region hypothesis (no parallel) `enterSubtree` from `s` reaches a
unique leaf, and this is it (the last element of the descent). -/
def defaultLeaf (c : Chart) (s : StateId) : StateId :=
  (enterSubtree c c.states.length s).getLastD s

/-- The flat `StateMachine` a single-region chart compiles down to, for a fixed
guard resolver: `current` is the active leaf; `transition` walks up from the
state for the innermost passing transition and returns the default-descent leaf
of its target (or rejects with `none`). -/
def flatMachine (c : Chart) (leaf : StateId) (g : GuardResolver) :
    LazilyFormal.StateMachine.Machine :=
  { current := leaf,
    transition := fun s ev =>
      (enabledForLeaf c g ev s).map (fun st => defaultLeaf c st.2.target) }

/-- With a single active leaf, `enabled` holds exactly the one transition
`enabledForLeaf` discovers for that leaf (or none). -/
theorem enabled_of_enabledForLeaf
    (c : Chart) (cfg : Configuration) (g : GuardResolver) (ev : EventId) (leaf : StateId)
    (oneLeaf : activeLeaves c cfg = [leaf]) (st : StateId × Transition)
    (h : enabledForLeaf c g ev leaf = some st) :
    enabled c cfg g ev = [st] := by
  rw [enabled, oneLeaf, List.filterMap_cons, List.filterMap_nil, h]

/-- A singleton `enabled` list yields a singleton `sendTaken` — `keepTrans`
over the lone transition keeps it (its exit set is disjoint from the empty
prior accumulator). -/
theorem sendTaken_of_singleton
    (c : Chart) (cfg : Configuration) (g : GuardResolver) (ev : EventId)
    (st : StateId × Transition) (h : enabled c cfg g ev = [st]) :
    sendTaken c cfg g ev = [st] := by
  rw [sendTaken, h, List.foldl_cons, List.foldl_nil]
  show keepTrans c cfg [] st = [st]
  rw [keepTrans]
  split
  · rename_i hcond
    exfalso
    rw [List.any_eq_true] at hcond
    rcases hcond with ⟨y, _, hcx⟩
    exact absurd (List.contains_iff_mem.mp hcx) List.not_mem_nil
  · rfl

/-- The active leaf of a one-leaf configuration is itself a leaf. -/
theorem activeLeaf_isLeaf (c : Chart) (cfg : Configuration) (leaf : StateId)
    (oneLeaf : activeLeaves c cfg = [leaf]) : isLeaf c leaf = true := by
  have hh : leaf ∈ activeLeaves c cfg := by rw [oneLeaf]; exact List.mem_cons_self
  rw [activeLeaves, List.mem_filter] at hh
  exact hh.2

-- ----------------------------------------------------- single-region coherence

/-- `getLastD` of a nonempty list is a member of it. -/
theorem getLastD_mem {α : Type} (l : List α) (d : α) (h : l ≠ []) : l.getLastD d ∈ l := by
  cases l with
  | nil => exact absurd rfl h
  | cons hd tl => simp only [List.getLastD]; exact List.getLast_mem _

/-- The walk-up that selects an enabled transition only visits the leaf's
ancestor chain: the chosen `src` lies on `ancestorsAux c n leaf`. Model-only. -/
theorem enabledForLeafAux_source_mem_ancestorsAux (c : Chart) (g : GuardResolver) (ev : EventId) :
    ∀ (n : Nat) (leaf src : StateId) (t : Transition),
      enabledForLeafAux c g ev n leaf = some (src, t) → src ∈ ancestorsAux c n leaf := by
  intro n
  induction n with
  | zero => intros leaf src t h; simp [enabledForLeafAux] at h
  | succ m ih =>
    intro leaf src t h
    cases hps : c.parent leaf with
    | none =>
      simp only [enabledForLeafAux, hps] at h
      cases hon : c.on leaf ev with
      | none => simp only [hon, reduceIte] at h; simp at h
      | some t' =>
        simp only [hon, reduceIte] at h
        by_cases hgp : guardPasses g t' = true
        · rw [if_pos hgp] at h
          injection h with hpair; obtain ⟨rfl, rfl⟩ := hpair
          rw [ancestorsAux, hps]; exact List.mem_cons_self
        · rw [if_neg hgp] at h; simp at h
    | some p =>
      simp only [enabledForLeafAux, hps] at h
      cases hon : c.on leaf ev with
      | none =>
        simp only [hon, reduceIte] at h
        have hi := ih p src t h
        show src ∈ ancestorsAux c (m + 1) leaf
        rw [ancestorsAux, hps]; exact List.mem_cons_of_mem _ hi
      | some t' =>
        simp only [hon, reduceIte] at h
        by_cases hgp : guardPasses g t' = true
        · rw [if_pos hgp] at h
          injection h with hpair; obtain ⟨rfl, rfl⟩ := hpair
          rw [ancestorsAux, hps]; exact List.mem_cons_self
        · rw [if_neg hgp] at h
          have hi := ih p src t h
          show src ∈ ancestorsAux c (m + 1) leaf
          rw [ancestorsAux, hps]; exact List.mem_cons_of_mem _ hi

/-- The source of the transition discovered by `enabledForLeaf` lies on the
leaf's ancestor chain. Model-only. -/
theorem enabledForLeaf_source_mem_ancestors (c : Chart) (g : GuardResolver) (ev : EventId)
    (leaf src : StateId) (t : Transition)
    (h : enabledForLeaf c g ev leaf = some (src, t)) : src ∈ ancestors c leaf :=
  enabledForLeafAux_source_mem_ancestorsAux c g ev c.states.length leaf src t h

/-- `enterSubtree` is non-empty: its head is `s` itself. Model-only. -/
theorem enterSubtree_head_mem (c : Chart) (n : Nat) (s : StateId) :
    s ∈ enterSubtree c n s := by
  cases n with
  | zero => exact List.mem_cons_self
  | succ m => simp only [enterSubtree]; exact List.mem_cons_self

/-- `defaultLeaf` is an actual member of the descent. Model-only. -/
theorem defaultLeaf_mem_enterSubtree (c : Chart) (s : StateId) :
    defaultLeaf c s ∈ enterSubtree c c.states.length s := by
  have hhead : s ∈ enterSubtree c c.states.length s := enterSubtree_head_mem c c.states.length s
  have hne : enterSubtree c c.states.length s ≠ [] := by
    intro h; rw [h] at hhead; exact absurd hhead (by simp)
  simpa [defaultLeaf] using getLastD_mem _ _ hne

/-- The default descent of a leaf state is itself. Model-only. -/
theorem defaultLeaf_of_leaf (c : Chart) (s : StateId) (h : isLeaf c s = true) :
    defaultLeaf c s = s := by
  have he : enterSubtree c c.states.length s = [s] := by
    cases hn : c.states.length with
    | zero => rfl
    | succ m =>
      simp only [enterSubtree]
      cases hk : c.kind s with
      | atomic => rfl
      | final => rfl
      | compound => simp_all [isLeaf]
      | parallel => simp_all [isLeaf]
      | history deep => rfl
  simpa [defaultLeaf, he]

/-- A chart is structurally coherent when its kinds match its tree and its
default descents resolve: a state is a leaf exactly when it has no proper
descendants; every default descent bottoms out at a unique leaf (`defaultLeaf`);
and the root is an ancestor-or-self of every declared state (so the LCA
resolves within the ancestor chain). This is the structural well-formedness the
single-region refinement needs beyond pointer validity. -/
structure Chart.Coherent (c : Chart) : Prop where
  leaf_iff_noDesc : ∀ x, isLeaf c x = true ↔ ∀ y, isProperDescendant c y x = false
  descent_resolves : ∀ s ∈ c.states,
    ∀ x, x ∈ enterSubtree c c.states.length s → (isLeaf c x = true ↔ x = defaultLeaf c s)
  root_commonAncestor : ∀ s ∈ c.states, (root c) ∈ ancestors c s

/-- A proper ancestor is never a leaf. -/
theorem Chart.Coherent.properAncestor_notLeaf (c : Chart) (coh : c.Coherent)
    (x y : StateId) (h : isProperDescendant c y x = true) : isLeaf c x = false := by
  cases hbx : isLeaf c x with
  | true => have := (coh.leaf_iff_noDesc x).mp hbx y; simp_all
  | false => rfl

/-- A state's default descent bottoms out at a leaf. -/
theorem Chart.Coherent.defaultLeaf_isLeaf (c : Chart) (coh : c.Coherent)
    {s : StateId} (hs : s ∈ c.states) : isLeaf c (defaultLeaf c s) = true :=
  ((coh.descent_resolves s hs) _ (defaultLeaf_mem_enterSubtree c s)).mpr rfl

/-- The only leaf in a state's default descent is its `defaultLeaf`. -/
theorem Chart.Coherent.enterSubtree_leaf_unique (c : Chart) (coh : c.Coherent)
    {s : StateId} (hs : s ∈ c.states) (x : StateId)
    (hx : x ∈ enterSubtree c c.states.length s) (hxl : isLeaf c x = true) :
    x = defaultLeaf c s :=
  ((coh.descent_resolves s hs) x hx).mp hxl

/-- A `find?` hit is a list member. -/
theorem find?_mem {α : Type} (p : α → Bool) (l : List α) (x : α)
    (h : l.find? p = some x) : x ∈ l := by
  induction l with
  | nil => simp at h
  | cons a m ih =>
    simp only [List.find?_cons] at h
    by_cases hpa : p a = true
    · simp only [hpa, if_true] at h; injection h with heq; subst heq; exact List.mem_cons_self
    · simp only [hpa, if_false] at h; exact List.mem_cons_of_mem a (ih h)

/-- The LCA lies on `a`'s ancestor chain whenever the root is an ancestor of `a`
(so the `none` fallback also lands on the chain). Model-only. -/
theorem lca_mem_ancestors (c : Chart) (a b : StateId)
    (hroot : (root c) ∈ ancestors c a) : lca c a b ∈ ancestors c a := by
  unfold lca
  split
  next x h => exact find?_mem _ _ x h
  next => exact hroot

/-- `isAncestorIncl c a s` reflects `(ancestors c s).contains a`. Model-only. -/
theorem isAncestorIncl_mem (c : Chart) (a s : StateId) (h : a ∈ ancestors c s) :
    isAncestorIncl c a s = true := by
  unfold isAncestorIncl
  exact List.contains_iff_mem.mpr h

/-- `defaultLeaf tgt` is in `enterSet` for a non-history target. -/
theorem Chart.Coherent.defaultLeaf_in_enterSet (c : Chart) (coh : c.Coherent)
    (h : History) (lca tgt : StateId) (htgt : tgt ∈ c.states)
    (hneT : c.kind tgt ≠ Kind.history true) (hneF : c.kind tgt ≠ Kind.history false) :
    defaultLeaf c tgt ∈ enterSet c h lca tgt := by
  have h1 : defaultLeaf c tgt ∈ enterSubtree c c.states.length tgt :=
    defaultLeaf_mem_enterSubtree c tgt
  cases hk : c.kind tgt with
  | history deep => cases deep <;> simp_all
  | _ => simp only [enterSet, hk]; exact (List.mem_append).mpr (Or.inr h1)

/-- The only leaf in `enterSet` (non-history target) is `defaultLeaf tgt`. -/
theorem Chart.Coherent.enterSet_leaf_unique (c : Chart) (coh : c.Coherent)
    (h : History) (lca tgt : StateId) (htgt : tgt ∈ c.states)
    (hneT : c.kind tgt ≠ Kind.history true) (hneF : c.kind tgt ≠ Kind.history false)
    (x : StateId) (hx : x ∈ enterSet c h lca tgt) (hxl : isLeaf c x = true) :
    x = defaultLeaf c tgt := by
  cases hk : c.kind tgt with
  | history deep => cases deep <;> simp_all
  | _ =>
    simp only [enterSet, hk] at hx
    rcases (List.mem_append).mp hx with hpb | hes
    · have hxanc : x ∈ ancestors c tgt := by
        have htw : x ∈ (ancestors c tgt).takeWhile (fun y => y ≠ lca) := by
          simp only [pathBelow, List.mem_reverse] at hpb; exact hpb
        exact List.takeWhile_subset _ htw
      by_cases heq : x = tgt
      · rw [heq] at hxl; rw [heq]
        exact Chart.Coherent.enterSubtree_leaf_unique c coh htgt tgt
          (enterSubtree_head_mem c c.states.length tgt) hxl
      · exfalso
        have hpd : isProperDescendant c tgt x = true := by
          simp only [isProperDescendant, Bool.and_eq_true_iff, isAncestorIncl,
            List.contains_iff_mem, decide_eq_true_eq]
          exact ⟨fun h => heq h.symm, hxanc⟩
        have hf := Chart.Coherent.properAncestor_notLeaf c coh x tgt hpd
        simp_all
    · exact Chart.Coherent.enterSubtree_leaf_unique c coh htgt x hes hxl

/-- If the LCA of `a` and `b` is `a` itself, then `a` is an ancestor of `b`
(under root connectivity). Model-only. -/
theorem lca_eq_self_imp_in_ancestors (c : Chart) (a b : StateId)
    (hroot : root c ∈ ancestors c b) (h : lca c a b = a) : a ∈ ancestors c b := by
  unfold lca at h
  cases hf : (ancestors c a).find? (fun x => isAncestorIncl c x b) with
  | none => simp only [hf] at h; rw [← h]; exact hroot
  | some x =>
    simp only [hf] at h
    have hp := List.find?_some hf
    simp only at hp
    rw [h] at hp
    exact List.contains_iff_mem.mp hp

/-- The active leaf is resolved by a taken transition: it is exited by the
transition's LCA, or the target descends to it. Derived from coherence (no
transition-specific hypothesis). -/
theorem Chart.Coherent.leaf_resolved (c : Chart) (coh : c.Coherent)
    (cfg : Configuration) (g : GuardResolver) (ev leaf : StateId) (src : StateId) (t : Transition)
    (hef : enabledForLeaf c g ev leaf = some (src, t))
    (hleafCfg : leaf ∈ cfg) (hleafIsLeaf : isLeaf c leaf = true)
    (hleafStates : leaf ∈ c.states) (htgtStates : t.target ∈ c.states)
    (hheadLeaf : (activeLeaves c cfg).head! = leaf) :
    leaf ∈ exitSet c cfg (lcaOf c cfg (src, t)) ∨ defaultLeaf c t.target = leaf := by
  have hsrcAnc : src ∈ ancestors c leaf :=
    enabledForLeaf_source_mem_ancestors c g ev leaf src t hef
  have hnoDesc : ∀ y, isProperDescendant c y leaf = false :=
    (coh.leaf_iff_noDesc leaf).mp hleafIsLeaf
  have hrootLeaf : (root c) ∈ ancestors c leaf := coh.root_commonAncestor leaf hleafStates
  have hdef : defaultLeaf c leaf = leaf := defaultLeaf_of_leaf c leaf hleafIsLeaf
  simp only [lcaOf, hheadLeaf]
  split
  next hcond =>
    have ⟨_, hdec⟩ := Bool.and_eq_true_iff.mp hcond
    have hsplit : t.target = src ∨ isProperDescendant c t.target src = true := of_decide_eq_true hdec
    by_cases heq : leaf = src
    · right
      rcases hsplit with htgt | hpd
      · have htt : t.target = leaf := by rw [htgt, heq]
        rw [htt]; exact hdef
      · exfalso; rw [← heq] at hpd; simp only [hnoDesc] at hpd; exact absurd hpd (by decide)
    · left
      simp only [exitSet, List.mem_filter]
      refine ⟨hleafCfg, ?_⟩
      simp only [isProperDescendant, Bool.and_eq_true_iff, isAncestorIncl,
        List.contains_iff_mem, decide_eq_true_eq]
      exact ⟨heq, hsrcAnc⟩
  next =>
    have hlcaAnc : lca c leaf t.target ∈ ancestors c leaf :=
      lca_mem_ancestors c leaf t.target hrootLeaf
    by_cases heq : leaf = lca c leaf t.target
    · right
      have hlcaLeafTgt : leaf ∈ ancestors c t.target :=
        lca_eq_self_imp_in_ancestors c leaf t.target
          (coh.root_commonAncestor t.target htgtStates) heq.symm
      by_cases heq2 : leaf = t.target
      · rw [← heq2]; exact hdef
      · exfalso
        have hpd : isProperDescendant c t.target leaf = true := by
          simp only [isProperDescendant, Bool.and_eq_true_iff, isAncestorIncl,
            List.contains_iff_mem, decide_eq_true_eq]
          exact ⟨fun h => heq2 h.symm, hlcaLeafTgt⟩
        simp only [hnoDesc] at hpd; exact absurd hpd (by decide)
    · left
      simp only [exitSet, List.mem_filter]
      refine ⟨hleafCfg, ?_⟩
      simp only [isProperDescendant, Bool.and_eq_true_iff, isAncestorIncl,
        List.contains_iff_mem, decide_eq_true_eq]
      exact ⟨heq, hlcaAnc⟩



/-- **Single-region refinement of the flat `StateMachine` kernel.** Under the
single-region hypotheses (no parallel / history states) with exactly one active
leaf, the chart's `send` refines the flat machine derived in [`flatMachine`]:
the new active leaf equals `(StateMachine.send (flatMachine c leaf g) ev).current`.

Stated extensionally (the new active-leaf *set* is the singleton of the flat
machine's transition target): for every `x`, `x` is an active leaf of the chart
after `send` iff `x` is the flat machine's new current state.

The reject case (no enabled, guard-passing transition) is proved from pointer
well-formedness alone. The take case (one enabled transition) is derived from
[`Chart.Coherent`] — the structural coherence (kinds match the tree; default
descents resolve to a unique leaf; the root is a common ancestor) that the
model's loose `WellFormed` does not pin down. All four coherence consequences
(`defaultLeaf` is a leaf; `defaultLeaf` is in `enterSet`; the only `enterSet`
leaf is `defaultLeaf`; and the active-leaf resolution `leaf_resolved`) are now
*derived* from `coh : c.Coherent` — no transition-specific hypothesis. -/
theorem single_region_refines_flat_machine
    (c : Chart) (cfg : Configuration) (h : History) (g : GuardResolver) (ev : EventId)
    (leaf : StateId)
    (singleRegion : ∀ s ∈ c.states,
       c.kind s ≠ Kind.parallel ∧ c.kind s ≠ Kind.history true ∧
       c.kind s ≠ Kind.history false)
    (oneLeaf : activeLeaves c cfg = [leaf])
    (wf : c.WellFormed) (cfg_in : ∀ x ∈ cfg, x ∈ c.states) (_hwf : history_wf c h)
    (cfgUniqueLeaf : ∀ s ∈ cfg, isLeaf c s → s = leaf)
    (coh : c.Coherent) :
    ∀ x, x ∈ activeLeaves c (send c cfg h g ev).cfg ↔
          x = (LazilyFormal.StateMachine.send (flatMachine c leaf g) ev).current := by
  intro x
  have hleafIsLeaf : isLeaf c leaf = true := activeLeaf_isLeaf c cfg leaf oneLeaf
  have hheadLeaf : (activeLeaves c cfg).head! = leaf := by rw [oneLeaf]; rfl
  cases hef : enabledForLeaf c g ev leaf with
  | none =>
    have hen : enabled c cfg g ev = [] := by
      rw [enabled, oneLeaf, List.filterMap_cons, List.filterMap_nil, hef]
    have hsend : (send c cfg h g ev).cfg = cfg := (enabled_empty_rejects c cfg h g ev hen).1
    rw [hsend, oneLeaf]
    simp only [LazilyFormal.StateMachine.send, flatMachine, hef, Option.map_none, List.mem_singleton]
  | some st =>
    have hen : enabled c cfg g ev = [st] := enabled_of_enabledForLeaf c cfg g ev leaf oneLeaf st hef
    have hst : sendTaken c cfg g ev = [st] := sendTaken_of_singleton c cfg g ev st hen
    have hne : (enabled c cfg g ev).isEmpty = false := by rw [hen]; rfl
    have hsend : (send c cfg h g ev).cfg = applyTakenCfg c cfg h [st] := by
      rw [send_cfg_eq_applyTakenCfg_take c cfg h g ev hne, hst]
    have henabled : st ∈ enabled c cfg g ev := by rw [hen]; exact List.mem_cons_self
    obtain ⟨hsrc_in, htgt_in⟩ := enabled_in_states c wf cfg g ev cfg_in st henabled
    have hSR_tgt := singleRegion st.2.target htgt_in
    have hleafCfg : leaf ∈ cfg :=
      activeLeaves_in_cfg c cfg leaf (by rw [oneLeaf]; exact List.mem_cons_self)
    have hleafStates : leaf ∈ c.states := cfg_in leaf hleafCfg
    have hleaf_res := Chart.Coherent.leaf_resolved c coh cfg g ev leaf st.1 st.2 hef
      hleafCfg hleafIsLeaf hleafStates htgt_in hheadLeaf
    have hmemiff : x ∈ applyTakenCfg c cfg h [st] ↔
        (x ∈ cfg ∧ x ∉ transExitSet c cfg st) ∨ x ∈ transEnterSet c cfg h st := by
      have key := applyTakenCfg_mem_iff c cfg h [st] x
      simpa [List.flatMap_singleton] using key
    rw [hsend, activeLeaves, List.mem_filter]
    simp only [LazilyFormal.StateMachine.send, flatMachine, hef, Option.map_some]
    constructor
    · rintro ⟨hxmem, hxisLeaf⟩
      rcases hmemiff.mp hxmem with ⟨hxcfg, hxnexit⟩ | hxenter
      · have hxleaf : x = leaf := cfgUniqueLeaf x hxcfg hxisLeaf
        subst hxleaf
        rcases hleaf_res with hl_in | hdef
        · exact absurd hl_in hxnexit
        · exact hdef.symm
      · exact Chart.Coherent.enterSet_leaf_unique c coh h (lcaOf c cfg st) st.2.target htgt_in
          hSR_tgt.2.1 hSR_tgt.2.2 x hxenter hxisLeaf
    · rintro hxeq
      subst hxeq
      refine ⟨hmemiff.mpr (Or.inr
          (Chart.Coherent.defaultLeaf_in_enterSet c coh h (lcaOf c cfg st) st.2.target htgt_in
            hSR_tgt.2.1 hSR_tgt.2.2)),
          Chart.Coherent.defaultLeaf_isLeaf c coh htgt_in⟩
end LazilyFormal.StateChart
