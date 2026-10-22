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
          lastAction, sawExact, commandOutcome

vars == <<columns, frontendSnapshot, focused, generation, flatCount,
          lastAction, sawExact, commandOutcome>>

TypeOK ==
    /\ columns \in Layouts
    /\ frontendSnapshot \in Layouts
    /\ focused \in Docs
    /\ generation \in 1..3
    /\ flatCount \in 0..2
    /\ lastAction \in {"init", "selection", "flat", "exact", "flat_route", "flat_sync"}
    /\ sawExact \in BOOLEAN
    /\ commandOutcome \in {"none", "ensure", "preserve"}

Init ==
    /\ columns \in Layouts
    /\ frontendSnapshot = columns
    /\ focused \in SeqSet(columns)
    /\ generation = 1
    /\ flatCount = 0
    /\ lastAction = "init"
    /\ sawExact = FALSE
    /\ commandOutcome = "none"

SelectionOnly ==
    /\ \E selected \in Docs : focused' = selected
    /\ lastAction' = "selection"
    /\ UNCHANGED <<columns, frontendSnapshot, generation, flatCount, sawExact,
                    commandOutcome>>

FlatEvidence ==
    /\ flatCount < 2
    /\ columns' = IF EnforceNoGuess
                   THEN columns
                   ELSE IF flatCount = 1 THEN Swap(columns) ELSE columns
    /\ flatCount' = flatCount + 1
    /\ lastAction' = "flat"
    /\ UNCHANGED <<frontendSnapshot, focused, generation, sawExact,
                    commandOutcome>>

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
    /\ UNCHANGED commandOutcome

(***************************************************************************)
(* Explicit editor commands consume the same retained Source. A route may   *)
(* turn flat visibility into a non-structural ensure request. Exact Sync    *)
(* Tmux Panes cannot claim missing geometry, so successful handling is an   *)
(* idempotent preserve terminal rather than a structural effect or error.   *)
(***************************************************************************)
FlatRouteCommand ==
    /\ \E selected \in Docs : focused' = selected
    /\ lastAction' = "flat_route"
    /\ commandOutcome' = "ensure"
    /\ UNCHANGED <<columns, frontendSnapshot, generation, flatCount, sawExact>>

FlatSyncCommand ==
    /\ lastAction' = "flat_sync"
    /\ commandOutcome' = "preserve"
    /\ UNCHANGED <<columns, frontendSnapshot, focused, generation, flatCount,
                    sawExact>>

Next == SelectionOnly \/ FlatEvidence \/ ExactFrontendSnapshot
        \/ FlatRouteCommand \/ FlatSyncCommand
Spec == Init /\ [][Next]_vars

CachedColumnsEqualFrontendAuthority == columns = frontendSnapshot
NonExactInputNeverMutatesStructure ==
    lastAction \in {"selection", "flat", "flat_route", "flat_sync"}
        => columns = frontendSnapshot
FlatSyncIsSuccessfulPreservation ==
    lastAction = "flat_sync"
        => /\ commandOutcome = "preserve"
           /\ columns = frontendSnapshot

NeverAcceptsExact == ~sawExact

=============================================================================
