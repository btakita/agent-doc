---------------------------- MODULE LayoutWidthBound ----------------------------
EXTENDS Naturals, TLC

(***************************************************************************)
(* GH #136(a): every desired-layout publication crosses NetChannel, so it   *)
(* may be delayed, reordered, duplicated, dropped, or survive a reconnect. *)
(*                                                                         *)
(* Editor publications are positive split observations and may widen.      *)
(* Derived publications (ensure, escalation, recycle-settle republish) are *)
(* bounded at effect time by the retained generation's local facts:        *)
(*                                                                         *)
(*   max(min(retained, observed + gated), asserted, 1)                     *)
(*                                                                         *)
(* A stale-generation delivery is ignored. No timing or channel ordering   *)
(* participates in the safety argument.                                    *)
(***************************************************************************)

CONSTANTS MaxWidth, MaxMessages, MaxCopies, MaxGen, EnforceDerivedBound

ASSUME /\ MaxWidth \in Nat /\ MaxWidth >= 2
       /\ MaxMessages \in Nat /\ MaxMessages >= 1

VARIABLES net, gen, delivered,
          sent, retained, observed, gated, asserted, publishedWidth,
          lastWasDerived, lastDerivedBound, sawTrim, sawEditorWiden

C == INSTANCE NetChannel

layoutVars == <<sent, retained, observed, gated, asserted, publishedWidth,
                lastWasDerived, lastDerivedBound, sawTrim, sawEditorWiden>>
vars == <<net, gen, delivered, layoutVars>>

Authorities == {"editor", "derived"}
Msg(id, authority, width, generation) ==
    [id |-> id, authority |-> authority, width |-> width, generation |-> generation]
Msgs == {Msg(id, authority, width, generation) :
            id \in 1..MaxMessages,
            authority \in Authorities,
            width \in 1..MaxWidth,
            generation \in 0..MaxGen}

Min(a, b) == IF a < b THEN a ELSE b
Max(a, b) == IF a > b THEN a ELSE b
DerivedBound == Max(Max(Min(retained, observed + gated), asserted), 1)

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ sent \in 0..MaxMessages
    /\ retained \in 1..MaxWidth
    /\ observed \in 0..MaxWidth
    /\ gated \in 0..MaxWidth
    /\ asserted \in 0..MaxWidth
    /\ publishedWidth \in 1..MaxWidth
    /\ lastWasDerived \in BOOLEAN
    /\ lastDerivedBound \in 1..(MaxWidth * 2)
    /\ sawTrim \in BOOLEAN
    /\ sawEditorWiden \in BOOLEAN

Init ==
    /\ C!ChannelInit
    /\ sent = 0
    /\ retained = 1
    /\ observed = 1
    /\ gated = 0
    /\ asserted = 0
    /\ publishedWidth = 1
    /\ lastWasDerived = FALSE
    /\ lastDerivedBound = 1
    /\ sawTrim = FALSE
    /\ sawEditorWiden = FALSE

ObserveExtent ==
    /\ observed' \in 0..MaxWidth
    /\ gated' \in 0..MaxWidth
    /\ asserted' \in 0..MaxWidth
    /\ UNCHANGED <<net, gen, delivered, sent, retained, publishedWidth,
                   lastWasDerived, lastDerivedBound, sawTrim, sawEditorWiden>>

SendPublication(authority, width) ==
    /\ sent < MaxMessages
    /\ C!Send(Msg(sent + 1, authority, width, gen))
    /\ sent' = sent + 1
    /\ UNCHANGED <<retained, observed, gated, asserted, publishedWidth,
                   lastWasDerived, lastDerivedBound, sawTrim, sawEditorWiden>>

Receive(m) ==
    /\ C!Deliver(m)
    /\ IF m.generation # gen
          THEN UNCHANGED layoutVars
          ELSE IF m.authority = "editor"
          THEN /\ publishedWidth' = m.width
               /\ retained' = m.width
               /\ observed' = m.width
               /\ gated' = 0
               /\ asserted' = 0
               /\ lastWasDerived' = FALSE
               /\ lastDerivedBound' = DerivedBound
               /\ sawEditorWiden' = (sawEditorWiden \/ m.width > publishedWidth)
               /\ UNCHANGED <<sent, sawTrim>>
          ELSE LET bound == DerivedBound
                   actual == IF EnforceDerivedBound THEN Min(m.width, bound) ELSE m.width
               IN /\ publishedWidth' = actual
                  /\ retained' = actual
                  /\ lastWasDerived' = TRUE
                  /\ lastDerivedBound' = bound
                  /\ sawTrim' = (sawTrim \/ actual < m.width)
                  /\ UNCHANGED <<sent, observed, gated, asserted, sawEditorWiden>>

AdversaryStep ==
    /\ C!Adversary
    /\ UNCHANGED layoutVars

Next ==
    \/ ObserveExtent
    \/ \E authority \in Authorities, width \in 1..MaxWidth :
          SendPublication(authority, width)
    \/ \E m \in C!InFlight : Receive(m)
    \/ AdversaryStep

Spec == Init /\ [][Next]_vars

DerivedPublicationBounded == lastWasDerived => publishedWidth <= lastDerivedBound

\* Wedge/reach obligations used by scripts/run_tla.sh.
NeverObservesEditorWidenAndDerivedTrim == ~(sawTrim /\ sawEditorWiden)

=============================================================================
