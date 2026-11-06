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

THE ORPHAN (`#orphanhandoffoverlay`, `OrphanRule`)
--------------------------------------------------
A predecessor that ACKs its row after the successor read it into the overlay
leaves an entry with no row to claim. `OrphanRule` is what the drain then does:

- "refold": the shipped rule. A row the projection folded clears the entry at
  once; otherwise the successor reloads the durable ledger first.
- "ignore": the previous rule. The entry outlives the drain, so the projection,
  and the reactive graphs that never see the overlay, lack the authority.
  `AuthorityHandoffFoldOrphanWedge` MUST violate `NoOrphanAtRest`.
- "clear": the tempting fix, clearing the entry because its row is gone. It
  loses the authority from reads. `AuthorityHandoffFoldClearWedge` MUST violate
  `OverlayClearImpliesFolded`.

`AuthorityHandoffFoldOrphanReach` asserts an unfolded orphan never happens and
MUST be violated, so the reload branch of "refold" is reachable.

THE LATE ACK (`#predecessorlandandack`, `ReconcileAckReceipts`)
---------------------------------------------------------------
A predecessor can also land and ACK its row after the successor's load but
before the successor reads the rows. Neither row nor overlay entry then exists,
so nothing the drain walks can carry the event. An ACK therefore commits, in the
transaction that deletes the row, a receipt naming the event (`receipt`). A
successor reconciles the receipts of older generations at the head of every
drain: a sequence at or below `foldedThrough` was folded by the load, one above
it was committed since, so it reloads the durable ledger, then discards the
receipt.

`AuthorityHandoffFoldLateAckWedge` (`ReconcileAckReceipts = FALSE`: the row
simply vanishes) MUST violate `EventuallyFolded`.
`AuthorityHandoffFoldLateAckReach` asserts the late ACK never happens and MUST
be violated, so the reconcile edge is exercised.

Out of scope: the rest of the class of any predecessor write committed after a
successor's load. A fact with no handoff row and no receipt reaches the
successor only through its next general durable reload.
***************************************************************************)

CONSTANTS FoldOnLateDuplicate, OrphanRule, ReconcileAckReceipts

ASSUME OrphanRule \in {"ignore", "clear", "refold"}

Rows == {"gen1", "gen2", "acked"}
SPcs == {"init", "loaded", "read", "orphan", "claimed", "appended", "done"}
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
    noted,          \* successor: the overlay held the event at some point
    outcome,        \* successor: what its append observed
    sAcked,         \* successor: it ACKed the row and cleared the overlay
    receipt,        \* handoff db: the predecessor's ACK receipt is unconsumed
    sReconciled     \* successor: it consumed a predecessor's ACK receipt

vars == <<seq, eventSeq, row, pLanded, pDone, sPc, foldedThrough, folded,
          overlay, noted, outcome, sAcked, receipt, sReconciled>>

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
    /\ noted \in BOOLEAN
    /\ outcome \in Outcomes
    /\ sAcked \in BOOLEAN
    /\ receipt \in BOOLEAN
    /\ sReconciled \in BOOLEAN

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
    /\ noted = FALSE
    /\ outcome = "none"
    /\ sAcked = FALSE
    /\ receipt = FALSE
    /\ sReconciled = FALSE

\* The parked predecessor apply commits (an insert that may find the row).
PLand ==
    /\ ~pLanded
    /\ pLanded' = TRUE
    /\ IF eventSeq = 0
       THEN /\ seq' = seq + 1
            /\ eventSeq' = seq + 1
       ELSE UNCHANGED <<seq, eventSeq>>
    /\ UNCHANGED <<row, pDone, sPc, foldedThrough, folded, overlay, noted, outcome, sAcked,
                   receipt, sReconciled>>

\* Its ACK is generation-fenced: it removes only a row still claimed by 1, and
\* commits the receipt in the transaction that deletes the row.
PAck ==
    /\ pLanded /\ ~pDone
    /\ pDone' = TRUE
    /\ row' = IF row = "gen1" THEN "acked" ELSE row
    /\ receipt' = (receipt \/ row = "gen1")
    /\ UNCHANGED <<seq, eventSeq, pLanded, sPc, foldedThrough, folded, overlay, noted, outcome,
                   sAcked, sReconciled>>

SLoad ==
    /\ sPc = "init"
    /\ sPc' = "loaded"
    /\ foldedThrough' = seq
    /\ folded' = (eventSeq # 0)
    /\ UNCHANGED <<seq, eventSeq, row, pLanded, pDone, overlay, noted, outcome, sAcked,
                   receipt, sReconciled>>

SReadRows ==
    /\ sPc = "loaded"
    /\ sPc' = "read"
    /\ overlay' = (row # "acked")
    /\ noted' = overlay'
    /\ UNCHANGED <<seq, eventSeq, row, pLanded, pDone, foldedThrough, folded, outcome, sAcked,
                   receipt, sReconciled>>

SClaim ==
    /\ sPc = "read"
    /\ IF row = "acked"
       THEN /\ sPc' = IF overlay /\ OrphanRule # "ignore" THEN "orphan" ELSE "done"
            /\ UNCHANGED row
       ELSE /\ sPc' = "claimed"
            /\ row' = "gen2"
    /\ UNCHANGED <<seq, eventSeq, pLanded, pDone, foldedThrough, folded, overlay, noted, outcome, sAcked,
                   receipt, sReconciled>>

\* No row is left for an overlay entry: settle it. The reload makes the
\* projection the fold of the whole ledger before the entry stops answering.
SSettle ==
    /\ sPc = "orphan"
    /\ sPc' = "done"
    /\ IF OrphanRule = "refold" /\ ~folded
       THEN /\ folded' = TRUE
            /\ foldedThrough' = seq
       ELSE UNCHANGED <<folded, foldedThrough>>
    /\ overlay' = FALSE
    /\ UNCHANGED <<seq, eventSeq, row, pLanded, pDone, noted, outcome, sAcked,
                   receipt, sReconciled>>

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
    /\ UNCHANGED <<row, pLanded, pDone, foldedThrough, overlay, noted, sAcked, receipt, sReconciled>>

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
    /\ UNCHANGED <<seq, eventSeq, pLanded, pDone, noted, outcome, receipt, sReconciled>>

\* `#predecessorlandandack`: runs at the head of every drain, so at any point
\* after the load. The receipt's sequence decides, exactly as a duplicate's does;
\* a receipt is consumed only once its event is folded.
SReconcile ==
    /\ ReconcileAckReceipts
    /\ sPc # "init"
    /\ receipt
    /\ IF eventSeq > foldedThrough
       THEN /\ folded' = TRUE
            /\ foldedThrough' = seq
       ELSE UNCHANGED <<folded, foldedThrough>>
    /\ receipt' = FALSE
    /\ sReconciled' = TRUE
    /\ UNCHANGED <<seq, eventSeq, row, pLanded, pDone, sPc, overlay, noted, outcome, sAcked>>

Done ==
    /\ pDone /\ sPc = "done"
    /\ UNCHANGED vars

Next == PLand \/ PAck \/ SLoad \/ SReadRows \/ SClaim \/ SSettle \/ SAppend \/ SAck
    \/ SReconcile \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(PLand) /\ WF_vars(PAck)
    /\ WF_vars(SLoad) /\ WF_vars(SReadRows) /\ WF_vars(SClaim)
    /\ WF_vars(SSettle) /\ WF_vars(SAppend) /\ WF_vars(SAck) /\ WF_vars(SReconcile)

\* The successor ACKs a row and clears its overlay only for an event its own
\* projection holds.
SuccessorAckImpliesFolded == sAcked => folded

\* `#orphanhandoffoverlay`: no overlay entry outlives the drain.
NoOrphanAtRest == sPc = "done" => ~overlay

\* `#orphanhandoffoverlay`: an entry stops answering reads only for an event the
\* projection holds, however it was cleared.
OverlayClearImpliesFolded == (noted /\ ~overlay) => folded

\* A successor consumes a receipt only for an event its own projection holds.
ReconciledImpliesFolded == sReconciled => folded

\* The ledger never holds the event twice.
SingleLedgerRow == seq <= 1

\* Every claimed row is eventually acknowledged by someone.
EventuallyAcked == <>(row = "acked")

\* However the row left the handoff table, the successor's projection ends up
\* holding the event.
EventuallyFolded == <>folded

\* Reach (asserted negated; MUST be violated): the late duplicate happens.
NeverLateDuplicate == outcome # "late"

\* Reach (asserted negated; MUST be violated): an entry with no row holds an
\* event the projection has not folded, so "refold" must reload.
NeverUnfoldedOrphan == ~(sPc = "orphan" /\ ~folded)

\* Reach (asserted negated; MUST be violated): the predecessor's ACK of a row
\* committed above the successor's load is observable, so `SReconcile` reloads.
NeverLateAck == ~(receipt /\ sPc # "init" /\ eventSeq > foldedThrough)
====================================================================================
