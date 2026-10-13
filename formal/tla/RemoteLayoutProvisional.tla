----------------------- MODULE RemoteLayoutProvisional -----------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************)
(* GH #250: Remote Dev / Coder can report the selected editor before its    *)
(* visible-editor membership update. The width-preserving replacement is a  *)
(* guess about WHICH split changed, not new geometry authority.             *)
(*                                                                         *)
(* The model explores every ordered two-column layout over four documents,  *)
(* either focused column, every outside selection, an optional second       *)
(* selection while membership still lags, and every corrective visible set *)
(* containing the selected document.                                       *)
(*                                                                         *)
(* `expectedBase` is a specification ghost: the layout before the first     *)
(* selection-only guess. Production carries the same fact as                *)
(* `RemoteLayoutMemory.provisional_base_columns`. The safety invariant says *)
(* the structural correction MUST equal a fold against that pre-guess base. *)
(***************************************************************************)

CONSTANT EnforceProvisional

Docs == {"a", "b", "c", "d"}
Layouts ==
    {<<left, right>> : left \in Docs, right \in Docs}
        \ {<<same, same>> : same \in Docs}

SeqSet(seq) == {seq[index] : index \in 1..Len(seq)}

FocusIndex(seq, focused) == CHOOSE index \in 1..Len(seq) : seq[index] = focused

ReplaceAt(seq, index, document) ==
    [slot \in 1..Len(seq) |-> IF slot = index THEN document ELSE seq[slot]]

(* The production same-width rule: survivors keep their exact slots and the *)
(* one added document fills the one dropped slot. With no survivors, the    *)
(* editor's visible order is the only available order.                      *)
Ordered(base, visible) ==
    LET survivors == SeqSet(base) \intersect SeqSet(visible)
        added == SeqSet(visible) \ SeqSet(base)
    IN IF survivors = {}
       THEN visible
       ELSE [slot \in 1..Len(base) |->
                 IF base[slot] \in SeqSet(visible)
                 THEN base[slot]
                 ELSE CHOOSE document \in added : TRUE]

VARIABLES columns, focused, phase, provisional, expectedBase, lastVisible,
          sawGuess, sawCorrection

vars == <<columns, focused, phase, provisional, expectedBase, lastVisible,
          sawGuess, sawCorrection>>

TypeOK ==
    /\ columns \in Layouts
    /\ focused \in Docs
    /\ phase \in {"stable", "guessed", "corrected"}
    /\ provisional \in Layouts \cup {<<>>}
    /\ expectedBase \in Layouts
    /\ lastVisible \in Layouts
    /\ sawGuess \in BOOLEAN
    /\ sawCorrection \in BOOLEAN

Init ==
    /\ columns \in Layouts
    /\ focused \in SeqSet(columns)
    /\ phase = "stable"
    /\ provisional = <<>>
    /\ expectedBase = columns
    /\ lastVisible = columns
    /\ sawGuess = FALSE
    /\ sawCorrection = FALSE

SelectionFirst ==
    /\ phase = "stable"
    /\ \E selected \in Docs \ SeqSet(columns) :
          /\ columns' = ReplaceAt(columns, FocusIndex(columns, focused), selected)
          /\ focused' = selected
          /\ provisional' = IF EnforceProvisional THEN columns ELSE <<>>
          /\ expectedBase' = columns
    /\ phase' = "guessed"
    /\ sawGuess' = TRUE
    /\ UNCHANGED <<lastVisible, sawCorrection>>

(* Coder may send another selection while the visible set still names the    *)
(* original columns. The first pre-guess base must survive that repetition.  *)
SelectionAgain ==
    /\ phase = "guessed"
    /\ \E selected \in Docs \ (SeqSet(columns) \cup SeqSet(expectedBase)) :
          /\ columns' = ReplaceAt(columns, FocusIndex(columns, focused), selected)
          /\ focused' = selected
    /\ UNCHANGED <<phase, provisional, expectedBase, lastVisible,
                    sawGuess, sawCorrection>>

VisibleCorrection ==
    /\ phase = "guessed"
    /\ \E visible \in Layouts :
          /\ focused \in SeqSet(visible)
          /\ columns' = Ordered(
                 IF Len(provisional) = 2 THEN provisional ELSE columns,
                 visible)
          /\ lastVisible' = visible
    /\ provisional' = <<>>
    /\ phase' = "corrected"
    /\ sawCorrection' = TRUE
    /\ UNCHANGED <<focused, expectedBase, sawGuess>>

Next == SelectionFirst \/ SelectionAgain \/ VisibleCorrection

Spec == Init /\ [][Next]_vars

(* The central GH #250 safety property. *)
CorrectionUsesPreGuessBase ==
    phase = "corrected" => columns = Ordered(expectedBase, lastVisible)

ProvisionalPreservesAncestry ==
    (EnforceProvisional /\ phase = "guessed") => provisional = expectedBase

ProvisionalClearsAfterCorrection ==
    phase = "corrected" => provisional = <<>>

(* Reachability obligation: a passing safety check must actually exercise a  *)
(* selection-first guess followed by structural correction.                  *)
NeverCorrects == ~(sawGuess /\ sawCorrection)

=============================================================================
