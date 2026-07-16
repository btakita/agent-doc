-------------------------- MODULE CrdtLineageFence --------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Finite control-state model for editor-authoritative recovery. CRDT updates are
monotonic only inside one lineage. A whole-document replacement rotates the
lineage; durable frames from the old lineage are terminally quarantined and
ACKed, while operator deletion tombstones and pending agent intent survive the
rebase. Delivery ACK and native editor save are separate transitions: commit
requires disk to contain the exact still-current editor version. TLC explores
every interleaving of the enabled actions below.
***************************************************************************)

Lineages == {"old", "current"}
CanonicalStates == {"base", "operator", "rebased", "agent-applied", "corrupt"}

VARIABLES
    lineage,
    canonical,
    operatorDelete,
    queueVisible,
    durableQueueVisible,
    pendingAgentIntent,
    staleFramePending,
    currentFramePending,
    disk,
    editorSaveRequested,
    ackCursor,
    committed

vars == <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
          pendingAgentIntent, staleFramePending, currentFramePending,
          disk, editorSaveRequested, ackCursor, committed>>

Init ==
    /\ lineage = "old"
    /\ canonical = "base"
    /\ operatorDelete = FALSE
    /\ queueVisible = TRUE
    /\ durableQueueVisible = TRUE
    /\ pendingAgentIntent = FALSE
    /\ staleFramePending = FALSE
    /\ currentFramePending = FALSE
    /\ disk = "base"
    /\ editorSaveRequested = FALSE
    /\ ackCursor = 0
    /\ committed = FALSE

OperatorDeletesQueue ==
    /\ queueVisible
    /\ operatorDelete' = TRUE
    /\ queueVisible' = FALSE
    /\ durableQueueVisible' = FALSE
    /\ canonical' = "operator"
    /\ staleFramePending' = TRUE
    /\ UNCHANGED <<lineage, pendingAgentIntent, currentFramePending,
                    disk, editorSaveRequested, ackCursor, committed>>

CaptureAgentIntent ==
    /\ ~pendingAgentIntent
    /\ ~committed
    /\ pendingAgentIntent' = TRUE
    /\ currentFramePending' = IF lineage = "current" THEN TRUE ELSE currentFramePending
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
                    staleFramePending, disk, editorSaveRequested,
                    ackCursor, committed>>

ReplaceAndRebase ==
    /\ lineage = "old"
    /\ operatorDelete
    /\ lineage' = "current"
    /\ canonical' = IF pendingAgentIntent THEN "agent-applied" ELSE "rebased"
    /\ currentFramePending' = pendingAgentIntent
    /\ UNCHANGED <<operatorDelete, queueVisible, durableQueueVisible, pendingAgentIntent,
                    staleFramePending, disk, editorSaveRequested,
                    ackCursor, committed>>

DeliverStaleFrame ==
    /\ staleFramePending
    /\ lineage = "current"
    /\ staleFramePending' = FALSE
    /\ ackCursor' = IF ackCursor < 2 THEN ackCursor + 1 ELSE ackCursor
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
                    pendingAgentIntent, currentFramePending, disk,
                    editorSaveRequested, committed>>

DeliverCurrentFrame ==
    /\ currentFramePending
    /\ lineage = "current"
    /\ currentFramePending' = FALSE
    /\ canonical' = "agent-applied"
    /\ ackCursor' = IF ackCursor < 2 THEN ackCursor + 1 ELSE ackCursor
    /\ UNCHANGED <<lineage, operatorDelete, queueVisible, durableQueueVisible,
                    pendingAgentIntent, staleFramePending, disk,
                    editorSaveRequested, committed>>

RequestEditorSave ==
    /\ pendingAgentIntent
    /\ canonical = "agent-applied"
    /\ ~currentFramePending
    /\ disk # canonical
    /\ editorSaveRequested' = TRUE
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
                    pendingAgentIntent, staleFramePending,
                    currentFramePending, disk, ackCursor, committed>>

EditorNativeSave ==
    /\ editorSaveRequested
    /\ canonical = "agent-applied"
    /\ ~currentFramePending
    /\ disk' = canonical
    /\ editorSaveRequested' = FALSE
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
                    pendingAgentIntent, staleFramePending,
                    currentFramePending, ackCursor, committed>>

OperatorAdvancesAfterSaveRequest ==
    /\ editorSaveRequested
    /\ pendingAgentIntent
    /\ canonical' = "operator"
    /\ currentFramePending' = TRUE
    /\ editorSaveRequested' = FALSE
    /\ UNCHANGED <<lineage, operatorDelete, queueVisible, durableQueueVisible,
                    pendingAgentIntent, staleFramePending, disk,
                    ackCursor, committed>>

CrashDropsCleanQueue ==
    /\ queueVisible
    /\ ~operatorDelete
    /\ queueVisible' = FALSE
    /\ UNCHANGED <<lineage, canonical, operatorDelete, durableQueueVisible,
                    pendingAgentIntent, staleFramePending, currentFramePending,
                    disk, editorSaveRequested, ackCursor, committed>>

RecoverDurableQueue ==
    /\ durableQueueVisible
    /\ ~operatorDelete
    /\ ~queueVisible
    /\ queueVisible' = TRUE
    /\ UNCHANGED <<lineage, canonical, operatorDelete, durableQueueVisible,
                    pendingAgentIntent, staleFramePending, currentFramePending,
                    disk, editorSaveRequested, ackCursor, committed>>

Commit ==
    /\ pendingAgentIntent
    /\ canonical = "agent-applied"
    /\ ~currentFramePending
    /\ disk = canonical
    /\ committed' = TRUE
    /\ pendingAgentIntent' = FALSE
    /\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
                    staleFramePending, currentFramePending, disk,
                    editorSaveRequested, ackCursor>>

TerminalStutter ==
    /\ committed
    /\ UNCHANGED vars

Next ==
    \/ OperatorDeletesQueue
    \/ CaptureAgentIntent
    \/ ReplaceAndRebase
    \/ DeliverStaleFrame
    \/ DeliverCurrentFrame
    \/ RequestEditorSave
    \/ EditorNativeSave
    \/ OperatorAdvancesAfterSaveRequest
    \/ CrashDropsCleanQueue
    \/ RecoverDurableQueue
    \/ Commit
    \/ TerminalStutter

Spec == Init /\ [][Next]_vars
        /\ WF_vars(DeliverStaleFrame)
        /\ WF_vars(DeliverCurrentFrame)
        /\ WF_vars(RecoverDurableQueue)

TypeOK ==
    /\ lineage \in Lineages
    /\ canonical \in CanonicalStates
    /\ operatorDelete \in BOOLEAN
    /\ queueVisible \in BOOLEAN
    /\ durableQueueVisible \in BOOLEAN
    /\ pendingAgentIntent \in BOOLEAN
    /\ staleFramePending \in BOOLEAN
    /\ currentFramePending \in BOOLEAN
    /\ disk \in CanonicalStates
    /\ editorSaveRequested \in BOOLEAN
    /\ ackCursor \in 0..2
    /\ committed \in BOOLEAN

DeletedQueueNeverResurrects == operatorDelete => ~queueVisible
DeletedQueueIsNotDurable == operatorDelete => ~durableQueueVisible
StaleFrameCannotCorrupt == canonical # "corrupt"
CommitRequiresAppliedIntent == committed => canonical = "agent-applied"
CommitRequiresExactNativeSave == committed => disk = "agent-applied"
ReplacementPreservesOperatorIntent ==
    (lineage = "current" /\ operatorDelete) => ~queueVisible
PendingIntentIsDurable ==
    (pendingAgentIntent /\ lineage = "current") =>
        canonical = "agent-applied" \/ currentFramePending
StaleFrameEventuallyAcked ==
    (lineage = "current" /\ staleFramePending) ~> ~staleFramePending
CleanCrashEventuallyRecovers ==
    (~operatorDelete /\ durableQueueVisible /\ ~queueVisible) ~> queueVisible

=============================================================================
