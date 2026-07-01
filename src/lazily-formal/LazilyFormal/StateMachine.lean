/-
! Flat finite state machine kernel.

The pure transition core that a single-region state chart compiles down to
(`State → Event → Option State`). Ported from `lazily-spec`'s formal model so
`lazily-formal` is the canonical home for the kernel; `StateChart` proves the
single-region chart refines this `send`.

This is the layer whose transitions are observable on the lazily-spec IPC wire
as a `cellSet` op (see `lazily-spec/.../StateMachineIPC.lean`); that linkage
stays in lazily-spec because it is a wire property.
-/

import LazilyFormal.Primitive

namespace LazilyFormal.StateMachine

open LazilyFormal.Primitive

/-- A state value in a finite state machine (abstract payload). -/
abbrev State := StateId

/-- An event value in a finite state machine (abstract payload). -/
abbrev Event := EventId

/-- A transition function: given the current state and an event, returns the
next state (`some`) or rejects the event (`none`, a guard). -/
abbrev Transition := State → Event → Option State

/-- A finite state machine is a current state plus a pure transition function. -/
structure Machine where
  current : State
  transition : Transition

/-- Send an event to the machine. Returns the machine unchanged if the guard
rejects (`none`), or with the new state if accepted (`some`). -/
def send (m : Machine) (e : Event) : Machine :=
  match m.transition m.current e with
  | some next => { m with current := next }
  | none => m

/-- Whether sending an event would change the state. -/
def sends (m : Machine) (e : Event) : Bool :=
  match m.transition m.current e with
  | some next => decide (next ≠ m.current)
  | none => false

theorem guard_rejection_preserves_state
    (m : Machine) (e : Event)
    (rejected : m.transition m.current e = none) :
    (send m e).current = m.current := by
  simp [send, rejected]

theorem accepted_transition_advances_state
    (m : Machine) (e : Event) (next : State)
    (accepted : m.transition m.current e = some next) :
    (send m e).current = next := by
  simp [send, accepted]

theorem self_transition_preserves_state
    (m : Machine) (e : Event)
    (accepted : m.transition m.current e = some m.current) :
    (send m e).current = m.current := by
  simp [send, accepted]

theorem self_transition_sends_false
    (m : Machine) (e : Event)
    (accepted : m.transition m.current e = some m.current) :
    sends m e = false := by
  simp [sends, accepted]

theorem changed_transition_sends_true
    (m : Machine) (e : Event) (next : State)
    (accepted : m.transition m.current e = some next)
    (changed : next ≠ m.current) :
    sends m e = true := by
  simp [sends, accepted, changed]

/-- The transition function is never changed by `send` — a machine is fully
described by its transition function and current state. -/
theorem send_preserves_transition (m : Machine) (e : Event) :
    (send m e).transition = m.transition := by
  unfold send
  split <;> rfl

end LazilyFormal.StateMachine
