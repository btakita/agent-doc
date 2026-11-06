---------------------------- MODULE PredecessorWriteAfterLoad ----------------------------
EXTENDS Naturals, TLC

(***************************************************************************
`#predecessorwriteafterload`: a predecessor write that is not an authority
handoff. `#predecessorlandandack` reconciles acknowledgement receipts only. A
fact with no handoff row and no receipt is just a ledger row. The successor
loads the ledger (`foldedThrough` is the high-water it folded). The predecessor
may commit the fact at any point, including after that load. `otherSeq` is the
fact's sequence, 0 when it has not been committed.

THE FIX (`ReconcilePredecessorWrites`)
--------------------------------------
At the head of every drain the successor compares the ledger with
`foldedThrough`. A fact above it was committed after the load, so the drain
reloads and the projection holds it (`otherFolded`). A fact the load already
folded needs nothing further.

`PredecessorWriteAfterLoadWedge` (`ReconcilePredecessorWrites = FALSE`, the
drain that only walks handoff receipts) MUST violate `EventuallyOtherFolded`.
`PredecessorWriteAfterLoadReach` asserts the late write never happens and MUST
be violated, so the reload edge is exercised.
***************************************************************************)

CONSTANTS ReconcilePredecessorWrites

SPcs == {"init", "loaded"}

VARIABLES
    seq,            \* ledger: highest committed sequence
    otherSeq,       \* ledger: sequence of the non-handoff fact (0 = absent)
    foldedThrough,  \* successor: high-water of the rows its projection folded
    otherFolded,    \* successor: its projection holds the fact
    sPc,            \* successor: program counter
    pWrote          \* predecessor: the fact's insert has run

vars == <<seq, otherSeq, foldedThrough, otherFolded, sPc, pWrote>>

TypeOK ==
    /\ seq \in 0..1
    /\ otherSeq \in 0..1
    /\ foldedThrough \in 0..1
    /\ otherFolded \in BOOLEAN
    /\ sPc \in SPcs
    /\ pWrote \in BOOLEAN

Init ==
    /\ seq = 0
    /\ otherSeq = 0
    /\ foldedThrough = 0
    /\ otherFolded = FALSE
    /\ sPc = "init"
    /\ pWrote = FALSE

\* The predecessor commits one fact that has no handoff row and no receipt.
PWrite ==
    /\ ~pWrote
    /\ pWrote' = TRUE
    /\ seq' = seq + 1
    /\ otherSeq' = seq + 1
    /\ UNCHANGED <<foldedThrough, otherFolded, sPc>>

SLoad ==
    /\ sPc = "init"
    /\ sPc' = "loaded"
    /\ foldedThrough' = seq
    /\ otherFolded' = (otherSeq # 0)
    /\ UNCHANGED <<seq, otherSeq, pWrote>>

\* `#predecessorwriteafterload`: repeatable, so a write that lands after a drain
\* already ran is still folded. A fact at or below the high-water was folded by
\* the load and is not a reason to reload.
SFold ==
    /\ ReconcilePredecessorWrites
    /\ sPc = "loaded"
    /\ otherSeq > foldedThrough
    /\ otherFolded' = TRUE
    /\ foldedThrough' = seq
    /\ UNCHANGED <<seq, otherSeq, pWrote, sPc>>

Done ==
    /\ pWrote /\ sPc = "loaded"
    /\ otherFolded \/ ~ReconcilePredecessorWrites
    /\ UNCHANGED vars

Next == PWrite \/ SLoad \/ SFold \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(PWrite) /\ WF_vars(SLoad) /\ WF_vars(SFold)

\* The projection holds the fact only after the predecessor committed it.
FoldedImpliesWritten == otherFolded => pWrote

\* The fact is eventually in the successor's projection, whether it committed
\* before the load or after it.
EventuallyOtherFolded == <>otherFolded

\* Reach (asserted negated; MUST be violated): the predecessor commits above
\* the successor's folded high-water, so `SFold` has a row to reload.
NeverLateWrite == ~(sPc = "loaded" /\ otherSeq > foldedThrough)
====================================================================================
