----------------------- MODULE PaneExecutionAuthority -----------------------
EXTENDS FiniteSets, Naturals, TLC

(***************************************************************************
The model checks the command-admission seam shared by preflight, write, and
controller-owned recovery. A durable owner may be live, stale, or unknown; an
invocation carries an explicit pane/generation and may independently prove
exact process-tree ownership. One bounded owner rebind models the race between
observation and admission. Mutation is a separate step, so TLC checks that a
rejected or superseded admission cannot leak a side effect.
***************************************************************************)

CONSTANTS Panes, Generations

VARIABLES ownerPane,
          ownerGeneration,
          ownerPresent,
          ownerLiveness,
          invocationKind,
          invocationPane,
          invocationGeneration,
          exactInvocationOwner,
          phase,
          mutationOwners,
          mutationCount,
          repairCount,
          unsafeRepairCount,
          rebindUsed

vars == <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
          invocationKind, invocationPane, invocationGeneration, exactInvocationOwner,
          phase, mutationOwners, mutationCount, repairCount,
          unsafeRepairCount, rebindUsed>>

OwnerMatches ==
    /\ ownerPresent
    /\ invocationKind = "explicit"
    /\ invocationPane = ownerPane
    /\ invocationGeneration = ownerGeneration

Init ==
    /\ ownerPane \in Panes
    /\ ownerGeneration \in Generations
    /\ ownerPresent \in BOOLEAN
    /\ ownerLiveness \in {"live", "stale", "unknown"}
    /\ invocationKind \in {"explicit", "headless", "unavailable"}
    /\ invocationPane \in Panes
    /\ invocationGeneration \in Generations
    /\ exactInvocationOwner \in BOOLEAN
    /\ phase = "pending"
    /\ mutationOwners = {}
    /\ mutationCount = 0
    /\ repairCount = 0
    /\ unsafeRepairCount = 0
    /\ rebindUsed = FALSE

PendingRebind ==
    /\ phase = "pending"
    /\ ~rebindUsed
    /\ \E pane \in Panes, generation \in Generations,
          liveness \in {"live", "stale", "unknown"} :
          /\ ownerPane' = pane
          /\ ownerGeneration' = generation
          /\ ownerLiveness' = liveness
    /\ ownerPresent' \in BOOLEAN
    /\ rebindUsed' = TRUE
    /\ UNCHANGED <<invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, phase, mutationOwners,
                    mutationCount, repairCount, unsafeRepairCount>>

AdmitOwner ==
    /\ phase = "pending"
    /\ OwnerMatches
    /\ exactInvocationOwner
    /\ phase' = "admitted"
    /\ UNCHANGED <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
                    invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, mutationOwners, mutationCount,
                    repairCount, unsafeRepairCount, rebindUsed>>

AdmitUnregistered ==
    /\ phase = "pending"
    /\ ~ownerPresent
    /\ invocationKind \in {"explicit", "headless"}
    /\ phase' = "admitted"
    /\ UNCHANGED <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
                    invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, mutationOwners, mutationCount,
                    repairCount, unsafeRepairCount, rebindUsed>>

RejectUnavailableInvocation ==
    /\ phase = "pending"
    /\ (invocationKind = "unavailable"
        \/ (invocationKind = "headless" /\ ownerPresent))
    /\ phase' = "rejected"
    /\ UNCHANGED <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
                    invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, mutationOwners, mutationCount,
                    repairCount, unsafeRepairCount, rebindUsed>>

RejectLiveOrUnknownOwner ==
    /\ phase = "pending"
    /\ ownerPresent
    /\ invocationKind = "explicit"
    /\ (~OwnerMatches \/ ~exactInvocationOwner)
    /\ ownerLiveness \in {"live", "unknown"}
    /\ phase' = "rejected"
    /\ UNCHANGED <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
                    invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, mutationOwners, mutationCount,
                    repairCount, unsafeRepairCount, rebindUsed>>

RejectUnprovedStaleOwner ==
    /\ phase = "pending"
    /\ ownerPresent
    /\ invocationKind = "explicit"
    /\ (~OwnerMatches \/ ~exactInvocationOwner)
    /\ ownerLiveness = "stale"
    /\ ~exactInvocationOwner
    /\ phase' = "rejected"
    /\ UNCHANGED <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
                    invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, mutationOwners, mutationCount,
                    repairCount, unsafeRepairCount, rebindUsed>>

RepairProvedStaleOwner ==
    /\ phase = "pending"
    /\ ownerPresent
    /\ invocationKind = "explicit"
    /\ ~OwnerMatches
    /\ ownerLiveness = "stale"
    /\ exactInvocationOwner
    /\ ownerPane' = invocationPane
    /\ ownerGeneration' = invocationGeneration
    /\ ownerPresent' = TRUE
    /\ ownerLiveness' = "live"
    /\ repairCount' = repairCount + 1
    /\ unsafeRepairCount' = unsafeRepairCount
        + IF ownerLiveness = "live" THEN 1 ELSE 0
    /\ phase' = "admitted"
    /\ UNCHANGED <<invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, mutationOwners, mutationCount,
                    rebindUsed>>

Decide ==
    AdmitOwner
    \/ AdmitUnregistered
    \/ RejectUnavailableInvocation
    \/ RejectLiveOrUnknownOwner
    \/ RejectUnprovedStaleOwner
    \/ RepairProvedStaleOwner

Mutate ==
    /\ phase = "admitted"
    /\ (OwnerMatches \/ ~ownerPresent)
    /\ phase' = "mutated"
    /\ mutationOwners' = mutationOwners \cup {<<ownerPane, ownerGeneration>>}
    /\ mutationCount' = mutationCount + 1
    /\ UNCHANGED <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
                    invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, repairCount, unsafeRepairCount,
                    rebindUsed>>

Settle ==
    /\ phase = "mutated"
    /\ phase' = "terminal"
    /\ UNCHANGED <<ownerPane, ownerGeneration, ownerPresent, ownerLiveness,
                    invocationKind, invocationPane, invocationGeneration,
                    exactInvocationOwner, mutationOwners, mutationCount,
                    repairCount, unsafeRepairCount, rebindUsed>>

Done ==
    /\ phase \in {"rejected", "terminal"}
    /\ UNCHANGED vars

Next == PendingRebind \/ Decide \/ Mutate \/ Settle \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Decide)
    /\ WF_vars(Mutate)
    /\ WF_vars(Settle)

TypeOK ==
    /\ ownerPane \in Panes
    /\ ownerGeneration \in Generations
    /\ ownerPresent \in BOOLEAN
    /\ ownerLiveness \in {"live", "stale", "unknown"}
    /\ invocationKind \in {"explicit", "headless", "unavailable"}
    /\ invocationPane \in Panes
    /\ invocationGeneration \in Generations
    /\ exactInvocationOwner \in BOOLEAN
    /\ phase \in {"pending", "admitted", "rejected", "mutated", "terminal"}
    /\ mutationOwners \subseteq (Panes \X Generations)
    /\ mutationCount \in Nat
    /\ repairCount \in Nat
    /\ unsafeRepairCount \in Nat
    /\ rebindUsed \in BOOLEAN

NonOwnerNeverMutates ==
    mutationCount > 0 => (~ownerPresent \/ (OwnerMatches /\ exactInvocationOwner))

HeadlessNeverMutatesOwnedDocument ==
    (invocationKind = "headless" /\ ownerPresent) => mutationCount = 0

UnavailableInvocationNeverMutates ==
    invocationKind = "unavailable" => mutationCount = 0

RejectedAdmissionHasNoSideEffects == phase = "rejected" => mutationCount = 0

AtMostOneOwnerGenerationMutates == Cardinality(mutationOwners) <= 1

MutationIsExactlyOnce == mutationCount <= 1

StaleRepairNeverSeizesLiveOwner == unsafeRepairCount = 0

AdmittedRecoveryEventuallyTerminates ==
    [](phase = "admitted" => <> (phase = "terminal"))

=============================================================================
