-------------------- MODULE RegistrationAcknowledgedBase --------------------
EXTENDS Naturals, TLC

(***************************************************************************
Which common ancestor a JetBrains replica registration proves its
containment and clean merge from (`#ackedreplicabase`).

THE MEASURED FAILURE
--------------------
agent-doc-bugs.md, 2026-10-28 12:57:15-12:57:53. The operator typed a queue
row without pause. Every burst reached the controller through the cached
replica (`forwardLocalEdits ... durable=true`), but the settled shadow (S)
advances only when the controller accepts a visible projection, and every
keystroke deferred that (`visible_length_mismatch`), so S stayed 38 s old.
The resume-id capture wrote the frontmatter, undelivered because the editor
was mid-typing. A replace delivery then made the editor re-register:

  [crdt-replica] refusing ambiguous retained projection for agent-doc-bugs.md
      shadow_hash=3d5644c1... buffer_hash=fd456c7c... canonical_hash=a44cc912...

Canonical (C) held all the typing but the last keystroke (a one-character
retype) plus the resume id; the buffer (B) held all the typing. From S,
canonical's copy of the row and the buffer's copy are two different changes of
one slot, so neither the containment proof nor the line merge answered.

THE RULE
--------
The cached replica's text (A) is also a common ancestor whenever the
controller answers that canonical covers the frontier captured from that same
replica: every operation in A is in C, and B is A plus local typing. When S
proves nothing, registration proves containment and the clean merge from A.
`UseAcknowledgedBase` gates the rule so one model checks both directions:

  TRUE  -- `NoOperatorTextLoss`, `NoControllerTextLoss`, and `NoSpuriousHold`
           hold (RegistrationAcknowledgedBase.cfg).
  FALSE -- the shipped shadow-only proof. `NoSpuriousHold` MUST be violated
           (RegistrationAcknowledgedBaseWedge.cfg, on the must-violate list).

`RegistrationAcknowledgedBaseReach.cfg` asserts that registration never merges
from A and must also be violated, so the positive run is not passing on an edge
it never takes.

SLOTS
-----
  Front -- the frontmatter line the controller's managed write changes.
  Row   -- the row the operator is typing. Typing is monotone (each keystroke is
           a new value), which is what continuous typing is; an operator
           returning a slot to an older value is outside this model.

The controller may also rewrite Row itself (`AllowRowConflict`): that is a real
conflict from every ancestor, and registration must still hold rather than
choose a side.
***************************************************************************)

CONSTANTS UseAcknowledgedBase, AllowRowConflict, MaxEdits

Front == 1
Row == 2
Slots == {Front, Row}
ControllerRow == MaxEdits + 1
Values == 0..ControllerRow

VARIABLES shadow, buffer, replica, canon, inflight, phase, edits,
          wrote, ingested, lostOp, lostCtl, via

vars == <<shadow, buffer, replica, canon, inflight, phase, edits,
          wrote, ingested, lostOp, lostCtl, via>>

Zero == [s \in Slots |-> 0]

TypeOK ==
    /\ shadow \in [Slots -> Values]
    /\ buffer \in [Slots -> Values]
    /\ replica \in [Slots -> Values]
    /\ canon \in [Slots -> Values]
    /\ inflight \in BOOLEAN
    /\ phase \in {"pending", "held", "registered"}
    /\ edits \in 0..MaxEdits
    /\ wrote \in BOOLEAN
    /\ ingested \in 0..MaxEdits
    /\ lostOp \in BOOLEAN
    /\ lostCtl \in BOOLEAN
    /\ via \in {"none", "converged", "unedited", "publish", "contained",
                "merge", "contained-ack", "merge-ack", "hold"}

Init ==
    /\ shadow = Zero
    /\ buffer = Zero
    /\ replica = Zero
    /\ canon = Zero
    /\ inflight = FALSE
    /\ phase = "pending"
    /\ edits = 0
    /\ wrote = FALSE
    /\ ingested = 0
    /\ lostOp = FALSE
    /\ lostCtl = FALSE
    /\ via = "none"

\* The controller holds every operation of the cached replica.
Covers == ~inflight

\* Every slot B changed since `base` already reads the same in C.
ContainedFrom(base) == \A s \in Slots : buffer[s] # base[s] => canon[s] = buffer[s]

\* No slot changed differently on both sides since `base` (the line merge).
MergesFrom(base) ==
    \A s \in Slots : ~(buffer[s] # base[s] /\ canon[s] # base[s] /\ canon[s] # buffer[s])

Merged(base) == [s \in Slots |-> IF buffer[s] # base[s] THEN buffer[s] ELSE canon[s]]

AckUsable == UseAcknowledgedBase /\ Covers /\ replica # shadow

\* `RegistrationProjection` over `registrationCausalProofUtil`, in their order.
Decision ==
    IF buffer = canon THEN "converged"
    ELSE IF buffer = shadow /\ Covers THEN "unedited"
    ELSE IF canon = shadow THEN "publish"
    ELSE IF ContainedFrom(shadow) THEN "contained"
    ELSE IF MergesFrom(shadow) THEN "merge"
    ELSE IF AckUsable /\ (buffer = replica \/ (canon # replica /\ ContainedFrom(replica)))
        THEN "contained-ack"
    ELSE IF AckUsable /\ MergesFrom(replica) THEN "merge-ack"
    ELSE "hold"

\* The operator types into the buffer; the burst has not reached the replica.
Type ==
    /\ phase = "pending"
    /\ edits < MaxEdits
    /\ buffer' = [buffer EXCEPT ![Row] = edits + 1]
    /\ edits' = edits + 1
    /\ UNCHANGED <<shadow, replica, canon, inflight, phase, wrote, ingested, lostOp, lostCtl, via>>

\* The local-delta worker applies the captured burst to the cached replica.
LocalApply ==
    /\ phase = "pending"
    /\ replica[Row] # buffer[Row]
    /\ replica' = [replica EXCEPT ![Row] = buffer[Row]]
    /\ inflight' = TRUE
    /\ UNCHANGED <<shadow, buffer, canon, phase, edits, wrote, ingested, lostOp, lostCtl, via>>

\* The durable push lands: canonical now covers the replica's frontier.
Ingest ==
    /\ phase = "pending"
    /\ inflight
    /\ canon' = [canon EXCEPT ![Row] = replica[Row]]
    /\ inflight' = FALSE
    /\ ingested' = replica[Row]
    /\ UNCHANGED <<shadow, buffer, replica, phase, edits, wrote, lostOp, lostCtl, via>>

\* The controller's managed write (the resume id), retained and undelivered.
ControllerWrite ==
    /\ phase = "pending"
    /\ ~wrote
    /\ canon' = [canon EXCEPT ![Front] = 1]
    /\ wrote' = TRUE
    /\ UNCHANGED <<shadow, buffer, replica, inflight, phase, edits, ingested, lostOp, lostCtl, via>>

\* The controller rewrites the row the operator is typing: a true conflict.
ControllerRowRewrite ==
    /\ AllowRowConflict
    /\ phase = "pending"
    /\ canon[Row] # ControllerRow
    /\ canon' = [canon EXCEPT ![Row] = ControllerRow]
    /\ UNCHANGED <<shadow, buffer, replica, inflight, phase, edits, wrote, ingested, lostOp, lostCtl, via>>

\* The text registration leaves in the buffer and canonical.
Result ==
    CASE Decision = "publish" -> buffer
      [] Decision = "merge" -> Merged(shadow)
      [] Decision = "merge-ack" -> Merged(replica)
      [] OTHER -> canon

\* A replace delivery makes the editor re-register, at any moment.
Register ==
    /\ phase = "pending"
    /\ via' = Decision
    /\ IF Decision = "hold"
          THEN /\ phase' = "held"
               /\ UNCHANGED <<buffer, canon, lostOp, lostCtl>>
          ELSE /\ phase' = "registered"
               /\ buffer' = Result
               /\ canon' = Result
               \* Operator text the controller never ingested, destroyed.
               /\ lostOp' = lostOp \/ (Result[Row] # buffer[Row] /\ buffer[Row] > ingested)
               \* A controller write dropped: the managed Front write, or its Row rewrite.
               /\ lostCtl' = lostCtl \/ Result[Front] # canon[Front]
                                    \/ (canon[Row] = ControllerRow /\ Result[Row] # ControllerRow)
    /\ UNCHANGED <<shadow, replica, inflight, edits, wrote, ingested>>

Next ==
    \/ Type
    \/ LocalApply
    \/ Ingest
    \/ ControllerWrite
    \/ ControllerRowRewrite
    \/ Register

Spec == Init /\ [][Next]_vars

\* The Row slot is the operator's: registration never changes what they typed.
NoOperatorTextLoss == ~lostOp

\* The Front slot is the controller's: registration never drops its write.
NoControllerTextLoss == ~lostCtl

\* A proven ancestor from which the two sides changed different slots.
DisjointSinceAcknowledged ==
    /\ Covers
    /\ \A s \in Slots :
        ~(buffer[s] # replica[s] /\ canon[s] # replica[s] /\ canon[s] # buffer[s])

\* Registration never holds when the acknowledged replica text proves a merge.
NoSpuriousHold == phase = "held" => ~DisjointSinceAcknowledged

\* Non-vacuity: the acknowledged-base merge edge is actually taken.
NeverMergesFromAcknowledged == via # "merge-ack"

=============================================================================
