-------------------------- MODULE CrdtLineageFence --------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Finite control-state model for editor-authoritative recovery.  CRDT updates
are monotonic only inside one lineage.  A whole-document replacement rotates
the lineage; durable frames from the old lineage are terminally quarantined
and ACKed, while operator deletion tombstones and pending agent intent survive
the rebase. TLC explores every interleaving of the enabled actions below.
***************************************************************************)

Lineages == {"old", "current"}
CanonicalStates == {"base", "operator", "rebased", "agent-applied", "corrupt"}

VARIABLES
    lineage,
    canonical,
    operatorDelete,
    queueVisible,
    pendingAgentIntent,
    staleFramePending,
    currentFramePending,
    ackCursor,
    committed

vars == <<lineage, canonical, operatorDelete, queueVisible,
          pendingAgentIntent, staleFramePending, currentFramePending,
          ackCursor, committed>>

Init ==
    /\ lineage = "old"
    /\ canonical = "base"
    /\ operatorDelete = FALSE
    /\ queueVisible = TRUE
    /\ pendingAgentIntent = FALSE
    /\ staleFramePending = FALSE
    /\ currentFramePending = FALSE
    /\ ackCursor = 0
    /\ committed = FALSE

OperatorDeletesQueue ==
    /\ queueVisible
    /\ operatorDelete' = TRUE
    /\ queueVisible' = FALSE
    /\ canonical' = "operator"
    /\ staleFramePending' = TRUE
    /\ UNCHANGED <<lineage, pendingAgentIntent, currentFramePending,
                    ackCursor, committed>>

CaptureAgentIntent ==
    /\ ~pendingAgentIntent
    /\ pendingAgentIntent' = TRUE
    /\ currentFramePending' = IF lineage = "current" THEN TRUE ELSE currentFramePending
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible,
                    staleFramePending, ackCursor, committed>>

ReplaceAndRebase ==
    /\ lineage = "old"
    /\ operatorDelete
    /\ lineage' = "current"
    /\ canonical' = IF pendingAgentIntent THEN "agent-applied" ELSE "rebased"
    /\ currentFramePending' = pendingAgentIntent
    /\ UNCHANGED <<operatorDelete, queueVisible, pendingAgentIntent,
                    staleFramePending, ackCursor, committed>>

DeliverStaleFrame ==
    /\ staleFramePending
    /\ lineage = "current"
    /\ staleFramePending' = FALSE
    /\ ackCursor' = IF ackCursor < 2 THEN ackCursor + 1 ELSE ackCursor
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible,
                    pendingAgentIntent, currentFramePending, committed>>

DeliverCurrentFrame ==
    /\ currentFramePending
    /\ lineage = "current"
    /\ currentFramePending' = FALSE
    /\ canonical' = "agent-applied"
    /\ ackCursor' = IF ackCursor < 2 THEN ackCursor + 1 ELSE ackCursor
    /\ UNCHANGED <<lineage, operatorDelete, queueVisible,
                    pendingAgentIntent, staleFramePending, committed>>

Commit ==
    /\ pendingAgentIntent
    /\ canonical = "agent-applied"
    /\ ~currentFramePending
    /\ committed' = TRUE
    /\ pendingAgentIntent' = FALSE
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible,
                    staleFramePending, currentFramePending, ackCursor>>

Next ==
    \/ OperatorDeletesQueue
    \/ CaptureAgentIntent
    \/ ReplaceAndRebase
    \/ DeliverStaleFrame
    \/ DeliverCurrentFrame
    \/ Commit

Spec == Init /\ [][Next]_vars /\ WF_vars(DeliverStaleFrame) /\ WF_vars(DeliverCurrentFrame)

TypeOK ==
    /\ lineage \in Lineages
    /\ canonical \in CanonicalStates
    /\ operatorDelete \in BOOLEAN
    /\ queueVisible \in BOOLEAN
    /\ pendingAgentIntent \in BOOLEAN
    /\ staleFramePending \in BOOLEAN
    /\ currentFramePending \in BOOLEAN
    /\ ackCursor \in 0..2
    /\ committed \in BOOLEAN

DeletedQueueNeverResurrects == operatorDelete => ~queueVisible
StaleFrameCannotCorrupt == canonical # "corrupt"
CommitRequiresAppliedIntent == committed => canonical = "agent-applied"
ReplacementPreservesOperatorIntent ==
    (lineage = "current" /\ operatorDelete) => ~queueVisible
PendingIntentIsDurable ==
    (pendingAgentIntent /\ lineage = "current") =>
        canonical = "agent-applied" \/ currentFramePending
StaleFrameEventuallyAcked ==
    (lineage = "current" /\ staleFramePending) ~> ~staleFramePending

=============================================================================
