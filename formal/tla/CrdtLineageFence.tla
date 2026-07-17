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
committed,
responseCellLive,
boundaryCount

vars == <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
          pendingAgentIntent, staleFramePending, currentFramePending,
disk, editorSaveRequested, ackCursor, committed, responseCellLive,
boundaryCount>>

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
/\ responseCellLive = FALSE
/\ boundaryCount = 1

OperatorDeletesQueue ==
    /\ queueVisible
    /\ operatorDelete' = TRUE
    /\ queueVisible' = FALSE
    /\ durableQueueVisible' = FALSE
    /\ canonical' = "operator"
    /\ staleFramePending' = TRUE
/\ UNCHANGED <<lineage, pendingAgentIntent, currentFramePending,
disk, editorSaveRequested, ackCursor, committed, responseCellLive,
boundaryCount>>

CaptureAgentIntent ==
    /\ ~pendingAgentIntent
    /\ ~committed
    /\ pendingAgentIntent' = TRUE
    /\ currentFramePending' = (IF lineage = "current" THEN TRUE ELSE currentFramePending)
/\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
staleFramePending, disk, editorSaveRequested,
ackCursor, committed, responseCellLive, boundaryCount>>

ReplaceAndRebase ==
    /\ lineage = "old"
    /\ operatorDelete
    /\ lineage' = "current"
/\ canonical' = (IF pendingAgentIntent THEN "agent-applied" ELSE "rebased")
/\ currentFramePending' = pendingAgentIntent
/\ UNCHANGED <<operatorDelete, queueVisible, durableQueueVisible, pendingAgentIntent,
staleFramePending, disk, editorSaveRequested,
ackCursor, committed, responseCellLive, boundaryCount>>

DeliverStaleFrame ==
    /\ staleFramePending
    /\ lineage = "current"
    /\ staleFramePending' = FALSE
    /\ ackCursor' = (IF ackCursor < 2 THEN ackCursor + 1 ELSE ackCursor)
/\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
pendingAgentIntent, currentFramePending, disk,
editorSaveRequested, committed, responseCellLive, boundaryCount>>

DeliverCurrentFrame ==
    /\ currentFramePending
    /\ lineage = "current"
/\ currentFramePending' = FALSE
/\ canonical' = "agent-applied"
/\ responseCellLive' = TRUE
/\ ackCursor' = (IF ackCursor < 2 THEN ackCursor + 1 ELSE ackCursor)
/\ UNCHANGED <<lineage, operatorDelete, queueVisible, durableQueueVisible,
pendingAgentIntent, staleFramePending, disk,
editorSaveRequested, committed, boundaryCount>>

ProjectLiveResponse ==
/\ responseCellLive
/\ pendingAgentIntent
/\ ackCursor < 2
/\ ackCursor' = ackCursor + 1
/\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
                pendingAgentIntent, staleFramePending, currentFramePending, disk,
                editorSaveRequested, committed, responseCellLive, boundaryCount>>

RequestEditorSave ==
    /\ pendingAgentIntent
    /\ canonical = "agent-applied"
    /\ ~currentFramePending
    /\ disk # canonical
    /\ editorSaveRequested' = TRUE
/\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
pendingAgentIntent, staleFramePending,
currentFramePending, disk, ackCursor, committed, responseCellLive,
boundaryCount>>

EditorNativeSave ==
    /\ editorSaveRequested
    /\ canonical = "agent-applied"
    /\ ~currentFramePending
    /\ disk' = canonical
    /\ editorSaveRequested' = FALSE
/\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
pendingAgentIntent, staleFramePending,
currentFramePending, ackCursor, committed, responseCellLive, boundaryCount>>

OperatorAdvancesAfterSaveRequest ==
    /\ editorSaveRequested
    /\ pendingAgentIntent
    /\ canonical' = "operator"
    /\ currentFramePending' = TRUE
    /\ editorSaveRequested' = FALSE
/\ UNCHANGED <<lineage, operatorDelete, queueVisible, durableQueueVisible,
pendingAgentIntent, staleFramePending, disk,
ackCursor, committed, responseCellLive, boundaryCount>>

CrashDropsCleanQueue ==
    /\ queueVisible
    /\ ~operatorDelete
    /\ queueVisible' = FALSE
/\ UNCHANGED <<lineage, canonical, operatorDelete, durableQueueVisible,
pendingAgentIntent, staleFramePending, currentFramePending,
disk, editorSaveRequested, ackCursor, committed, responseCellLive,
boundaryCount>>

RecoverDurableQueue ==
    /\ durableQueueVisible
    /\ ~operatorDelete
    /\ ~queueVisible
    /\ queueVisible' = TRUE
/\ UNCHANGED <<lineage, canonical, operatorDelete, durableQueueVisible,
pendingAgentIntent, staleFramePending, currentFramePending,
disk, editorSaveRequested, ackCursor, committed, responseCellLive,
boundaryCount>>

Commit ==
    /\ pendingAgentIntent
    /\ canonical = "agent-applied"
    /\ ~currentFramePending
    /\ disk = canonical
    /\ committed' = TRUE
    /\ pendingAgentIntent' = FALSE
/\ UNCHANGED <<lineage, canonical, operatorDelete, queueVisible, durableQueueVisible,
staleFramePending, currentFramePending, disk,
editorSaveRequested, ackCursor, responseCellLive, boundaryCount>>

TerminalStutter ==
    /\ committed
    /\ UNCHANGED vars

Next ==
    \/ OperatorDeletesQueue
    \/ CaptureAgentIntent
    \/ ReplaceAndRebase
    \/ DeliverStaleFrame
\/ DeliverCurrentFrame
\/ ProjectLiveResponse
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
/\ responseCellLive \in BOOLEAN
/\ boundaryCount \in 0..1

DeletedQueueNeverResurrects == operatorDelete => ~queueVisible
DeletedQueueIsNotDurable == operatorDelete => ~durableQueueVisible
StaleFrameCannotCorrupt == canonical # "corrupt"
CommitRequiresAppliedIntent == committed => canonical = "agent-applied" /\ responseCellLive
CommitRequiresExactNativeSave == committed => disk = "agent-applied"
ReplacementPreservesOperatorIntent ==
    (lineage = "current" /\ operatorDelete) => ~queueVisible
PendingIntentIsDurable ==
(pendingAgentIntent /\ lineage = "current") =>
(canonical = "agent-applied" /\ responseCellLive) \/ currentFramePending
SingleBoundary == boundaryCount <= 1
ResponseProjectionPreservesOperatorCut ==
(operatorDelete /\ responseCellLive) => ~queueVisible /\ ~durableQueueVisible
StaleFrameEventuallyAcked ==
    (lineage = "current" /\ staleFramePending) ~> ~staleFramePending
CleanCrashEventuallyRecovers ==
    (~operatorDelete /\ durableQueueVisible /\ ~queueVisible) ~> queueVisible

=============================================================================
