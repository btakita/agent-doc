------------------------- MODULE RemoteLayoutNoGuess -------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

(***************************************************************************)
(* GH #343: flat visible, selection, repetition, and timing evidence may    *)
(* update focus but cannot mutate pane membership or order. Only an exact   *)
(* frontend-authored column vector may replace the cached snapshot.          *)
(***************************************************************************)

CONSTANT EnforceNoGuess

Docs == {"a", "b", "c", "d"}
Layouts ==
    {<<left, right>> : left \in Docs, right \in Docs}
        \ {<<same, same>> : same \in Docs}
SeqSet(seq) == {seq[index] : index \in 1..Len(seq)}
Swap(seq) == <<seq[2], seq[1]>>

VARIABLES columns, frontendSnapshot, focused, generation, flatCount,
          lastAction, sawExact

vars == <<columns, frontendSnapshot, focused, generation, flatCount,
          lastAction, sawExact>>

TypeOK ==
    /\ columns \in Layouts
    /\ frontendSnapshot \in Layouts
    /\ focused \in Docs
    /\ generation \in 1..3
    /\ flatCount \in 0..2
    /\ lastAction \in {"init", "selection", "flat", "exact"}
    /\ sawExact \in BOOLEAN

Init ==
    /\ columns \in Layouts
    /\ frontendSnapshot = columns
    /\ focused \in SeqSet(columns)
    /\ generation = 1
    /\ flatCount = 0
    /\ lastAction = "init"
    /\ sawExact = FALSE

SelectionOnly ==
    /\ \E selected \in Docs : focused' = selected
    /\ lastAction' = "selection"
    /\ UNCHANGED <<columns, frontendSnapshot, generation, flatCount, sawExact>>

FlatEvidence ==
    /\ flatCount < 2
    /\ columns' = IF EnforceNoGuess
                   THEN columns
                   ELSE IF flatCount = 1 THEN Swap(columns) ELSE columns
    /\ flatCount' = flatCount + 1
    /\ lastAction' = "flat"
    /\ UNCHANGED <<frontendSnapshot, focused, generation, sawExact>>

ExactFrontendSnapshot ==
    /\ generation < 3
    /\ \E snapshot \in Layouts :
          /\ columns' = snapshot
          /\ frontendSnapshot' = snapshot
          /\ focused' \in SeqSet(snapshot)
    /\ generation' = generation + 1
    /\ flatCount' = 0
    /\ lastAction' = "exact"
    /\ sawExact' = TRUE

Next == SelectionOnly \/ FlatEvidence \/ ExactFrontendSnapshot
Spec == Init /\ [][Next]_vars

CachedColumnsEqualFrontendAuthority == columns = frontendSnapshot
NonExactInputNeverMutatesStructure ==
    lastAction \in {"selection", "flat"} => columns = frontendSnapshot

NeverAcceptsExact == ~sawExact

=============================================================================
