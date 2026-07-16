----------------------------- MODULE CloseoutChurn -----------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Models the dog-food closeout churn repair.  An ACK has already landed, but a
verbose character-op payload temporarily occupies the controller.  Encoding
becomes bounded, the fair controller serves the retained observation, and the
same capture commits exactly once.  A reused heading is deliberately already
in HEAD; only full response-body identity may skip replay.  A recovery snapshot
has fewer duplicate queue matches than the live document, but each overlap is
still marked and the count mismatch cannot block closeout.  An unmarked strict
template response is rejected before mutation.
***************************************************************************)

(* --fair algorithm ChurnFreeCloseout
variables
compactPayload = FALSE,
controllerBusy = TRUE,
ackLanded = TRUE,
ackObserved = FALSE,
pressureMarkerWrites = 0,
headingInHead = TRUE,
fullResponseInHead = FALSE,
cycleState = "committed_unrelated",
captureCopies = 1,
responseCopies = 0,
documentQueueMatches = 3,
snapshotQueueMatches = 2,
documentQueueMarked = 0,
snapshotQueueMarked = 0,
queueSyncDone = FALSE,
unmarkedApplied = FALSE,
malformedRejected = FALSE,
committed = FALSE;

process Encoder = "encoder"
begin
EncodeCompactFrame:
    compactPayload := TRUE;
EncoderDone:
    while TRUE do
        skip;
    end while;
end process;

process Controller = "controller"
begin
AwaitBoundedFrame:
    await compactPayload;
ServeForegroundRead:
    controllerBusy := FALSE;
ControllerDone:
    while TRUE do
        skip;
    end while;
end process;

process Observer = "observer"
begin
RecordPressureOnce:
    pressureMarkerWrites := 1;
AwaitController:
    await ~controllerBusy /\ ackLanded;
ObserveLandedAck:
    ackObserved := TRUE;
ObserverDone:
    while TRUE do
        skip;
    end while;
end process;

process QueueSync = "queue"
begin
MarkLiveMatches:
    documentQueueMarked := documentQueueMatches;
MarkSnapshotOverlap:
    snapshotQueueMarked := snapshotQueueMatches;
    queueSyncDone := TRUE;
QueueDone:
    while TRUE do
        skip;
    end while;
end process;

process StrictMarkerGuard = "marker_guard"
begin
RejectUnmarkedResponse:
    assert ~unmarkedApplied;
    malformedRejected := TRUE;
MarkerGuardDone:
    while TRUE do
        skip;
    end while;
end process;

process Closeout = "closeout"
begin
FullBodyIdentityGate:
    assert headingInHead /\ ~fullResponseInHead;
    if fullResponseInHead then
        cycleState := "skipped_exact_duplicate";
    else
        cycleState := "rotated_open";
    end if;
AwaitRetainedConvergence:
    await ackObserved /\ queueSyncDone;
ApplySameCapture:
    assert cycleState = "rotated_open" /\ captureCopies = 1;
    responseCopies := responseCopies + 1;
    fullResponseInHead := TRUE;
CommitExactlyOnce:
    cycleState := "committed_response";
    committed := TRUE;
CloseoutDone:
    while TRUE do
        skip;
    end while;
end process;
end algorithm; *)

TypeOK ==
    /\ compactPayload \in BOOLEAN
    /\ controllerBusy \in BOOLEAN
    /\ ackLanded \in BOOLEAN
    /\ ackObserved \in BOOLEAN
    /\ pressureMarkerWrites \in Nat
    /\ headingInHead \in BOOLEAN
    /\ fullResponseInHead \in BOOLEAN
    /\ cycleState \in {"committed_unrelated", "rotated_open",
                        "skipped_exact_duplicate", "committed_response"}
    /\ captureCopies \in Nat
    /\ responseCopies \in Nat
    /\ documentQueueMatches \in Nat
    /\ snapshotQueueMatches \in Nat
    /\ documentQueueMarked \in Nat
    /\ snapshotQueueMarked \in Nat
    /\ queueSyncDone \in BOOLEAN
    /\ unmarkedApplied \in BOOLEAN
    /\ malformedRejected \in BOOLEAN
    /\ committed \in BOOLEAN

NoCaptureRecapture == captureCopies = 1
ResponseAppliedAtMostOnce == responseCopies <= 1
HeadingAloneNeverSkips == cycleState # "skipped_exact_duplicate"
QueueMismatchNeverBlocks == queueSyncDone =>
    /\ documentQueueMarked = documentQueueMatches
    /\ snapshotQueueMarked = snapshotQueueMatches
StrictUnmarkedNeverMutates == ~unmarkedApplied
PressureMarkerDoesNotChurn == pressureMarkerWrites <= 1
CommitRequiresObservedAckAndFullBody == committed =>
    /\ ackObserved
    /\ fullResponseInHead
    /\ responseCopies = 1
    /\ queueSyncDone

EventuallyForegroundObservesLandedAck == <>ackObserved
EventuallyPartialQueueSyncCompletes == <>queueSyncDone
EventuallyMalformedResponseRejected == <>malformedRejected
EventuallySameCaptureCommitsExactlyOnce == <>(committed /\ responseCopies = 1)

=============================================================================
