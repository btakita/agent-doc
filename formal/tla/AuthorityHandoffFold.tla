---------------------------- MODULE AuthorityHandoffFold ----------------------------
EXTENDS Naturals, TLC

(***************************************************************************
`#retiredhandoffapplyrace`: a durable document-authority handoff row across a
Project Controller handoff.

A retired predecessor (generation 1) claimed the row and parked its main-ledger
append in SQLite's busy handler. The successor (generation 2) loads the ledger,
reads the handoff rows into its pending-authority overlay, and drains: claim the
row (generation 2), append the event (`INSERT OR IGNORE`), ACK the row and clear
the overlay. The parked predecessor insert may commit at ANY point, including
after the successor's ledger load and before the successor's own append, which
then reports `inserted == false` for a row its projection never folded.

The successor projection holds the event (`folded`) when its ledger load saw the
row, when its own append inserted it, or when a durable reload ran after the row
committed. `foldedThrough` is the high-water sequence the projection folded.

THE FIX (`FoldOnLateDuplicate`)
-------------------------------
A duplicate whose ledger sequence is at or below `foldedThrough` was folded by
the load and is ACKed at once. One above it was committed by another writer
after the load, so the successor reloads the durable ledger before the ACK.

`AuthorityHandoffFoldWedge` (`FoldOnLateDuplicate = FALSE`, the shipped rule:
every duplicate is ACKed as-is) MUST violate `SuccessorAckImpliesFolded`.
`AuthorityHandoffFoldReach` asserts the late duplicate never happens and MUST be
violated: the race interleaving is reachable, so the positive run is not
vacuous.

Out of scope: a predecessor that both lands and ACKs its row after the
successor's load but before the successor claims it. Nothing then remains for
the successor's drain, and its projection learns the event only from the next
general durable reload -- the class of any predecessor write committed after a
successor's load, which this model does not claim to close.
***************************************************************************)

CONSTANTS FoldOnLateDuplicate

Rows == {"gen1", "gen2", "acked"}
SPcs == {"init", "loaded", "read", "claimed", "appended", "done"}
Outcomes == {"none", "inserted", "folded", "late"}

VARIABLES
    seq,            \* ledger: highest committed sequence
    eventSeq,       \* ledger: sequence of the handoff event (0 = absent)
    row,            \* handoff db: claim generation, or acked
    pLanded,        \* predecessor: its parked insert has run
    pDone,          \* predecessor: its ACK attempt has run
    sPc,            \* successor: program counter
    foldedThrough,  \* successor: high-water of the rows its projection folded
    folded,         \* successor: its projection holds the event
    overlay,        \* successor: pending-authority overlay holds the event
    outcome,        \* successor: what its append observed
    sAcked          \* successor: it ACKed the row and cleared the overlay

vars == <<seq, eventSeq, row, pLanded, pDone, sPc, foldedThrough, folded,
          overlay, outcome, sAcked>>

TypeOK ==
    /\ seq \in 0..2
    /\ eventSeq \in 0..2
    /\ row \in Rows
    /\ pLanded \in BOOLEAN
    /\ pDone \in BOOLEAN
    /\ sPc \in SPcs
    /\ foldedThrough \in 0..2
    /\ folded \in BOOLEAN
    /\ overlay \in BOOLEAN
    /\ outcome \in Outcomes
    /\ sAcked \in BOOLEAN

Init ==
    /\ seq = 0
    /\ eventSeq = 0
    /\ row = "gen1"
    /\ pLanded = FALSE
    /\ pDone = FALSE
    /\ sPc = "init"
    /\ foldedThrough = 0
    /\ folded = FALSE
    /\ overlay = FALSE
    /\ outcome = "none"
    /\ sAcked = FALSE

\* The parked predecessor apply commits (an insert that may find the row).
PLand ==
    /\ ~pLanded
    /\ pLanded' = TRUE
    /\ IF eventSeq = 0
       THEN /\ seq' = seq + 1
            /\ eventSeq' = seq + 1
       ELSE UNCHANGED <<seq, eventSeq>>
    /\ UNCHANGED <<row, pDone, sPc, foldedThrough, folded, overlay, outcome, sAcked>>

\* Its ACK is generation-fenced: it removes only a row still claimed by 1.
PAck ==
    /\ pLanded /\ ~pDone
    /\ pDone' = TRUE
    /\ row' = IF row = "gen1" THEN "acked" ELSE row
    /\ UNCHANGED <<seq, eventSeq, pLanded, sPc, foldedThrough, folded, overlay, outcome, sAcked>>

SLoad ==
    /\ sPc = "init"
    /\ sPc' = "loaded"
    /\ foldedThrough' = seq
    /\ folded' = (eventSeq # 0)
    /\ UNCHANGED <<seq, eventSeq, row, pLanded, pDone, overlay, outcome, sAcked>>

SReadRows ==
    /\ sPc = "loaded"
    /\ sPc' = "read"
    /\ overlay' = (row # "acked")
    /\ UNCHANGED <<seq, eventSeq, row, pLanded, pDone, foldedThrough, folded, outcome, sAcked>>

SClaim ==
    /\ sPc = "read"
    /\ IF row = "acked"
       THEN /\ sPc' = "done"
            /\ UNCHANGED row
       ELSE /\ sPc' = "claimed"
            /\ row' = "gen2"
    /\ UNCHANGED <<seq, eventSeq, pLanded, pDone, foldedThrough, folded, overlay, outcome, sAcked>>

SAppend ==
    /\ sPc = "claimed"
    /\ sPc' = "appended"
    /\ IF eventSeq = 0
       THEN /\ seq' = seq + 1
            /\ eventSeq' = seq + 1
            /\ folded' = TRUE
            /\ outcome' = "inserted"
       ELSE /\ UNCHANGED <<seq, eventSeq, folded>>
            /\ outcome' = IF eventSeq <= foldedThrough THEN "folded" ELSE "late"
    /\ UNCHANGED <<row, pLanded, pDone, foldedThrough, overlay, sAcked>>

\* The durable reload: the projection becomes the fold of the whole ledger.
SAck ==
    /\ sPc = "appended"
    /\ sPc' = "done"
    /\ IF outcome = "late" /\ FoldOnLateDuplicate
       THEN /\ folded' = TRUE
            /\ foldedThrough' = seq
       ELSE UNCHANGED <<folded, foldedThrough>>
    /\ row' = IF row = "gen2" THEN "acked" ELSE row
    /\ overlay' = FALSE
    /\ sAcked' = TRUE
    /\ UNCHANGED <<seq, eventSeq, pLanded, pDone, outcome>>

Done ==
    /\ pDone /\ sPc = "done"
    /\ UNCHANGED vars

Next == PLand \/ PAck \/ SLoad \/ SReadRows \/ SClaim \/ SAppend \/ SAck \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(PLand) /\ WF_vars(PAck)
    /\ WF_vars(SLoad) /\ WF_vars(SReadRows) /\ WF_vars(SClaim)
    /\ WF_vars(SAppend) /\ WF_vars(SAck)

\* The successor ACKs a row and clears its overlay only for an event its own
\* projection holds.
SuccessorAckImpliesFolded == sAcked => folded

\* The ledger never holds the event twice.
SingleLedgerRow == seq <= 1

\* Every claimed row is eventually acknowledged by someone.
EventuallyAcked == <>(row = "acked")

\* Reach (asserted negated; MUST be violated): the late duplicate happens.
NeverLateDuplicate == outcome # "late"
====================================================================================
