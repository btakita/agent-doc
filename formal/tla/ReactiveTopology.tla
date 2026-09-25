------------------------- MODULE ReactiveTopology -------------------------
EXTENDS FiniteSets, Naturals, TLC

(***************************************************************************
This model is the compositional contract for one lifetime-scoped Lazily graph:

  observation Source -> policy Computed -> Effect -> receipt Source

An observation may supersede a derived decision or a pending effect. Only an
effect carrying the exact current observation generation may mutate, and its
receipt carries that same generation. Closing the scope makes every pending
effect stale. Weak fairness plus a finite generation bound proves convergence
once the environment stops publishing newer observations.
***************************************************************************)

CONSTANT MaxGeneration

VARIABLES scopeOpen,
          observationGeneration,
          computedGeneration,
          pendingEffect,
          effectGeneration,
          receiptGeneration,
          mutatedGenerations,
          mutationCount,
          staleMutations,
          closedScopeMutations

vars == <<scopeOpen, observationGeneration, computedGeneration,
          pendingEffect, effectGeneration, receiptGeneration,
          mutatedGenerations, mutationCount, staleMutations,
          closedScopeMutations>>

Init ==
    /\ scopeOpen = TRUE
    /\ observationGeneration = 0
    /\ computedGeneration = 0
    /\ pendingEffect = FALSE
    /\ effectGeneration = 0
    /\ receiptGeneration = 0
    /\ mutatedGenerations = {}
    /\ mutationCount = 0
    /\ staleMutations = 0
    /\ closedScopeMutations = 0

PublishObservation ==
    /\ scopeOpen
    /\ observationGeneration < MaxGeneration
    /\ observationGeneration' = observationGeneration + 1
    /\ UNCHANGED <<computedGeneration, pendingEffect, effectGeneration,
                    receiptGeneration, mutatedGenerations, staleMutations,
                    mutationCount, closedScopeMutations, scopeOpen>>

Recompute ==
    /\ scopeOpen
    /\ computedGeneration < observationGeneration
    /\ computedGeneration' = observationGeneration
    /\ UNCHANGED <<observationGeneration, pendingEffect, effectGeneration,
                    receiptGeneration, mutatedGenerations, staleMutations,
                    mutationCount, closedScopeMutations, scopeOpen>>

ScheduleEffect ==
    /\ scopeOpen
    /\ ~pendingEffect
    /\ computedGeneration = observationGeneration
    /\ receiptGeneration < computedGeneration
    /\ pendingEffect' = TRUE
    /\ effectGeneration' = computedGeneration
    /\ UNCHANGED <<observationGeneration, computedGeneration,
                    receiptGeneration, mutatedGenerations, staleMutations,
                    mutationCount, closedScopeMutations, scopeOpen>>

RunEffect ==
    /\ scopeOpen
    /\ pendingEffect
    /\ effectGeneration = computedGeneration
    /\ effectGeneration = observationGeneration
    /\ effectGeneration \notin mutatedGenerations
    /\ pendingEffect' = FALSE
    /\ receiptGeneration' = effectGeneration
    /\ mutatedGenerations' = mutatedGenerations \cup {effectGeneration}
    /\ mutationCount' = mutationCount + 1
    /\ staleMutations' = staleMutations
        + IF effectGeneration = observationGeneration THEN 0 ELSE 1
    /\ closedScopeMutations' = closedScopeMutations
        + IF scopeOpen THEN 0 ELSE 1
    /\ UNCHANGED <<observationGeneration, computedGeneration,
                    effectGeneration, scopeOpen>>

DiscardStaleEffect ==
    /\ pendingEffect
    /\ (~scopeOpen \/ effectGeneration # observationGeneration)
    /\ pendingEffect' = FALSE
    /\ UNCHANGED <<scopeOpen, observationGeneration, computedGeneration,
                    effectGeneration, receiptGeneration, mutatedGenerations,
                    mutationCount, staleMutations, closedScopeMutations>>

CloseScope ==
    /\ scopeOpen
    /\ scopeOpen' = FALSE
    /\ UNCHANGED <<observationGeneration, computedGeneration,
                    pendingEffect, effectGeneration, receiptGeneration,
                    mutatedGenerations, mutationCount, staleMutations,
                    closedScopeMutations>>

Closed ==
    /\ ~scopeOpen
    /\ UNCHANGED vars

Next == PublishObservation \/ Recompute \/ ScheduleEffect \/ RunEffect
        \/ DiscardStaleEffect \/ CloseScope \/ Closed

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Recompute)
    /\ WF_vars(ScheduleEffect)
    /\ WF_vars(RunEffect)
    /\ WF_vars(DiscardStaleEffect)

TypeOK ==
    /\ scopeOpen \in BOOLEAN
    /\ observationGeneration \in 0..MaxGeneration
    /\ computedGeneration \in 0..MaxGeneration
    /\ pendingEffect \in BOOLEAN
    /\ effectGeneration \in 0..MaxGeneration
    /\ receiptGeneration \in 0..MaxGeneration
    /\ mutatedGenerations \subseteq 0..MaxGeneration
    /\ mutationCount \in Nat
    /\ staleMutations \in Nat
    /\ closedScopeMutations \in Nat

DerivedNeverLeadsObservation == computedGeneration <= observationGeneration

ReceiptNeverLeadsDerived == receiptGeneration <= computedGeneration

EveryMutationHasObservationLineage ==
    \A generation \in mutatedGenerations : generation <= observationGeneration

ReceiptHasMutationLineage ==
    receiptGeneration = 0 \/ receiptGeneration \in mutatedGenerations

StaleEffectNeverMutates == staleMutations = 0

ClosedScopeEffectNeverMutates == closedScopeMutations = 0

EachGenerationMutatesAtMostOnce ==
    mutationCount = Cardinality(mutatedGenerations)

QuiescentOpenGraphEventuallyReceipts ==
    [](scopeOpen /\ observationGeneration = MaxGeneration
       => <> (~scopeOpen \/ receiptGeneration = MaxGeneration))

=============================================================================
