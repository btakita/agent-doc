----------------------- MODULE RetainedProjectionHold -----------------------
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************
Whether an editor replica that registration refused can ever attach again.

THE MEASURED FAILURE
--------------------
Observed 2026-09-29 on `src/haiven-dev/tasks/fpe.md`. The JetBrains replica
re-registered every few seconds and was refused each time:

  [crdt-replica] refusing ambiguous retained projection for fpe.md; the live
      operator buffer was not derived from this canonical generation.
      shadow_hash=5415a2b0... buffer_hash=4091028a...
  register failed for fpe.md; reason=ambiguous-retained-projection

`live_editors=0`, so the controller's retained write could never settle and
every new cycle was refused ("the prior document write is still reconciling").

Three generations were in play: the last shadow the editor published (S), the
live editor buffer (B, byte-identical to disk), and the controller's canonical
text (C). The operator's only real edit since S, a new queue line, WAS already
in C. What else separated B from C was binary-owned bookkeeping: the disk
projection had written a ` (HEAD)` heading suffix and moved the boundary
marker, and C carried merge debris after the last component.

`#ambiguousholdforever` gave the hold one exit: adopt C when it provably
contains every operator edit from S to B. The proof built ONE splice from the
first difference between S and B to the last. The queue line and the heading
marker are in different places, so that splice spanned the whole response body
between them, and C's copy of that span differed in the markers. The proof said
"not contained", no input ever changed, and the hold never ended.

THE RULE
--------
Containment is decided per operator edit, never by a covering splice, and
binary-owned markers are not operator edits: they are normalized away before
the proof. `PerEditContainment` gates the fix so one model checks both
directions:

  TRUE  -- `NoOperatorTextLoss` holds and the replica eventually registers
           (RetainedProjectionHold.cfg).
  FALSE -- the shipped single-splice proof. `EventuallyRegistered` MUST be
           violated (RetainedProjectionHoldWedge.cfg, on the must-violate list).

`RetainedProjectionHoldReach.cfg` asserts that registration never adopts
canonical through the containment edge and must also be violated, so the
positive run is not passing on an edge it never takes.

SLOTS
-----
The document is abstracted to slots, each holding a small value:

  Marker -- binary-owned: boundary marker placement and the `(HEAD)` suffix.
  Body   -- the response body between the heading and the queue.
  Queue  -- the queue component the operator typed into.
  Tail   -- text after the last component; only merge debris ever lands here.

The operator can edit Body and Queue. Only the controller writes Marker and
Tail. A single-splice proof over S->B covers every slot between the first and
last difference, which is what lets a Marker difference poison a Queue edit.
***************************************************************************)

CONSTANTS PerEditContainment, MaxEdits

Marker == 1
Body == 2
Queue == 3
Tail == 4
Slots == {Marker, Body, Queue, Tail}
OperatorSlots == {Body, Queue}
Values == {0, 1, 2}

VARIABLES shadow, buffer, canon, phase, edits, reloaded, debris, lost, via

vars == <<shadow, buffer, canon, phase, edits, reloaded, debris, lost, via>>

Zero == [s \in Slots |-> 0]

TypeOK ==
    /\ shadow \in [Slots -> Values]
    /\ buffer \in [Slots -> Values]
    /\ canon \in [Slots -> Values]
    /\ phase \in {"detached", "registered"}
    /\ edits \in 0..MaxEdits
    /\ reloaded \in BOOLEAN
    /\ debris \in BOOLEAN
    /\ lost \in BOOLEAN
    /\ via \in {"none", "converged", "unedited", "publish", "contained"}

Init ==
    /\ shadow = Zero
    /\ buffer = Zero
    /\ canon = Zero
    /\ phase = "detached"
    /\ edits = 0
    /\ reloaded = FALSE
    /\ debris = FALSE
    /\ lost = FALSE
    /\ via = "none"

Changed == {s \in Slots : shadow[s] # buffer[s]}

Min(S) == CHOOSE m \in S : \A x \in S : m <= x
Max(S) == CHOOSE m \in S : \A x \in S : m >= x

\* The shipped proof: one splice covering every slot from the first to the last
\* S->B difference must appear verbatim in canonical.
SingleSpliceContained ==
    Changed # {} /\ \A s \in Min(Changed)..Max(Changed) : canon[s] = buffer[s]

\* The fix: each operator edit is proven on its own, and binary-owned slots are
\* not operator edits.
PerEditContained ==
    \A s \in Changed \cap OperatorSlots : canon[s] = buffer[s]

Contained == IF PerEditContainment THEN PerEditContained ELSE SingleSpliceContained

\* `retainedRegistrationProjectionActionUtil`, in its order.
Decision ==
    IF buffer = canon THEN "converged"
    ELSE IF buffer = shadow THEN "unedited"
    ELSE IF canon = shadow THEN "publish"
    ELSE IF Contained THEN "contained"
    ELSE "hold"

\* The operator types while the replica is detached.
OperatorEdit ==
    /\ phase = "detached"
    /\ edits < MaxEdits
    /\ \E s \in OperatorSlots, v \in Values :
        /\ buffer[s] # v
        /\ buffer' = [buffer EXCEPT ![s] = v]
    /\ edits' = edits + 1
    /\ UNCHANGED <<shadow, canon, phase, reloaded, debris, lost, via>>

\* The controller ingests an operator delta that reached it before the endpoint
\* dropped (fpe.md's queue line was in canonical).
ControllerIngest ==
    /\ phase = "detached"
    /\ \E s \in OperatorSlots :
        /\ canon[s] # buffer[s]
        /\ canon' = [canon EXCEPT ![s] = buffer[s]]
    /\ UNCHANGED <<shadow, buffer, phase, edits, reloaded, debris, lost, via>>

\* A controller disk projection writes its own marker placement, and the IDE
\* reloads the buffer from disk. Canonical keeps a different placement.
DiskProjectionReload ==
    /\ phase = "detached"
    /\ ~reloaded
    /\ buffer' = [buffer EXCEPT ![Marker] = 1]
    /\ canon' = [canon EXCEPT ![Marker] = 2]
    /\ reloaded' = TRUE
    /\ UNCHANGED <<shadow, phase, edits, debris, lost, via>>

\* A duplicated replay leaves debris after the last component, in canonical only.
CanonicalDebris ==
    /\ phase = "detached"
    /\ ~debris
    /\ canon' = [canon EXCEPT ![Tail] = 1]
    /\ debris' = TRUE
    /\ UNCHANGED <<shadow, buffer, phase, edits, reloaded, lost, via>>

\* Registration. It is enabled only when the decision is not a hold, so a hold
\* that no input can change is a real wedge, not a busy retry.
Register ==
    /\ phase = "detached"
    /\ Decision # "hold"
    /\ via' = Decision
    /\ phase' = "registered"
    /\ IF Decision = "publish"
          THEN /\ canon' = buffer
               /\ buffer' = buffer
               /\ lost' = lost
          ELSE /\ lost' = lost \/ \E s \in OperatorSlots :
                                    buffer[s] # shadow[s] /\ canon[s] # buffer[s]
               /\ buffer' = canon
               /\ canon' = canon
    /\ UNCHANGED <<shadow, edits, reloaded, debris>>

Next ==
    \/ OperatorEdit
    \/ ControllerIngest
    \/ DiskProjectionReload
    \/ CanonicalDebris
    \/ Register

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ControllerIngest)
    /\ WF_vars(Register)

\* Registration never replaces operator text the controller has not seen.
NoOperatorTextLoss == ~lost

\* Once the operator stops and the controller has every operator edit, the
\* replica attaches: the hold has an exit that no marker or debris can block.
EventuallyRegistered == <>(phase = "registered")

\* Non-vacuity: the containment edge is actually taken.
NeverAdoptsThroughContainment == via # "contained"

=============================================================================
