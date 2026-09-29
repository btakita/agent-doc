------------------- MODULE RetainedTransitionFixedPoint -------------------
EXTENDS Naturals, TLC

(***************************************************************************
Whether a retained write can put a response into a document twice.

THE MEASURED FAILURE
--------------------
Observed 2026-09-29 on `src/haiven-dev/tasks/fpe.md`. A response was written
through the editor, which applied it before the controller model saw it. The
controller's retained write for the same response (Base without it -> Target
with it) was still pending. A controller disk projection then shaped the
editor cut with ` (HEAD)` and moved the boundary marker, and the operator added
a queue line. The retained transition compared the cut with its Target, found
them unequal, rebased Base -> Target over the cut, and wrote the result:

  crdt_cp_write source=retained_transition_projection_effect applied=true
      content_len=64405        (cut was 56875; one response is ~7.5 KB)

The response appeared twice, and lines of it plus a torn `60c81193 -->` landed
after `<!-- /agent:done -->`, outside every component.

THE RULE
--------
A retained transition whose Base -> Target delta is already in the cut is at
its fixed point: it settles as TargetVisible and writes nothing. Containment is
judged on operator/agent text with binary-owned markers normalized away, the
same rule as `RetainedProjectionHold`. `CheckContainment` gates it:

  TRUE  -- `AtMostOneResponse` and `EventuallySettled` hold
           (RetainedTransitionFixedPoint.cfg).
  FALSE -- the shipped comparison of whole texts. `AtMostOneResponse` MUST be
           violated (RetainedTransitionFixedPointWedge.cfg).

`RetainedTransitionFixedPointReach.cfg` asserts the fixed-point edge is never
taken and must be violated, so the positive run is not passing only through
cuts that lack the response.

MODEL
-----
`responses` counts response copies in the editor cut. `marker` is the
binary-owned marker placement, which the disk projection may change without
anyone typing. `queueLine` is an operator edit that makes the cut differ from
Target in operator text the retained write never touched. The retained write
wants exactly one response.
***************************************************************************)

CONSTANT CheckContainment

VARIABLES responses, marker, queueLine, retained, via

vars == <<responses, marker, queueLine, retained, via>>

TypeOK ==
    /\ responses \in 0..3
    /\ marker \in {"target", "projected"}
    /\ queueLine \in BOOLEAN
    /\ retained \in BOOLEAN
    /\ via \in {"none", "equal", "contained", "rebased"}

Init ==
    /\ responses = 0
    /\ marker = "target"
    /\ queueLine = FALSE
    /\ retained = TRUE
    /\ via = "none"

\* The editor applies the response before the controller model sees it.
EditorApplies ==
    /\ retained
    /\ responses = 0
    /\ responses' = 1
    /\ UNCHANGED <<marker, queueLine, retained, via>>

\* A controller disk projection moves the binary-owned marker in the cut.
DiskProjection ==
    /\ retained
    /\ marker = "target"
    /\ marker' = "projected"
    /\ UNCHANGED <<responses, queueLine, retained, via>>

\* The operator types a queue line the retained write never touched.
OperatorTypes ==
    /\ retained
    /\ ~queueLine
    /\ queueLine' = TRUE
    /\ UNCHANGED <<responses, marker, retained, via>>

\* Cut equals Target byte-for-byte: one response, Target's markers, no
\* operator-only edits.
CutEqualsTarget == responses = 1 /\ marker = "target" /\ ~queueLine

\* The delta (one response) is already in the cut, ignoring markers and
\* unrelated operator edits.
DeltaContained == responses >= 1

Settle ==
    /\ retained
    /\ retained' = FALSE
    /\ IF CutEqualsTarget
          THEN /\ via' = "equal"
               /\ UNCHANGED responses
          ELSE IF CheckContainment /\ DeltaContained
                  THEN /\ via' = "contained"
                       /\ UNCHANGED responses
                  \* Rebase Base -> Target over the cut: inserts the response.
                  ELSE /\ via' = "rebased"
                       /\ responses' = responses + 1
    /\ UNCHANGED <<marker, queueLine>>

Next == EditorApplies \/ DiskProjection \/ OperatorTypes \/ Settle

Spec == Init /\ [][Next]_vars /\ WF_vars(Settle)

AtMostOneResponse == responses <= 1

EventuallySettled == <>(~retained)

NeverSettlesByContainment == via # "contained"

=============================================================================
