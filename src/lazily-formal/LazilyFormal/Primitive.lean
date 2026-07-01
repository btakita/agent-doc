/-
! Primitive types shared by the lazily formal models.

These are the language-agnostic building blocks the flat `StateMachine` kernel
and the full Harel `StateChart` model both rest on. They are intentionally
abstract (`Nat` stand-ins): the formal model fixes *behavior*, not wire encoding
— the lazily-spec conformance fixtures fix the cross-language surface, and this
model fixes the universal properties no fixture suite can prove.
-/

namespace LazilyFormal.Primitive

/-- Abstract identifier for a state node. In the flat `StateMachine` it doubles
    as the abstract payload `State` value. -/
abbrev StateId := Nat

/-- Abstract identifier for an event. -/
abbrev EventId := Nat

/-- An action name fired on entry / exit / transition (host-resolved side effect). -/
abbrev ActionId := Nat

/-- A named guard resolved by the host (`name → bool`); fail-closed when absent. -/
abbrev GuardId := Nat

/-- The active configuration: the set of active state ids (root → leaves),
    modeled as a list. Well-formedness (it actually forms a valid root→leaves
    set) is a per-chart predicate, not baked into the type. -/
abbrev Configuration := List StateId

/-- Host-supplied named-guard resolution for one `send`. -/
abbrev GuardResolver := GuardId → Bool

end LazilyFormal.Primitive
