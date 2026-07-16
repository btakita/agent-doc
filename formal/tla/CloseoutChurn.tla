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
After the first delivery proof, an editor advances canonical authority before
disk projection.  The document actor rebases the same retained intent; neither
the response nor its backlog mutation is captured/applied a second time.
If the editor then reconnects by replacement registration, its bootstrap is
already the retained canonical target and its delivery queue is empty.  A fair
session-check settles that historical deferred slot, refreshes the response
snapshot, and commits the same capture without requiring an impossible ACK.
Concurrent repair may observe only a reused response heading while that retained
write is still in flight; it must keep, never retire, the active capture.
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
backlogMutationCopies = 0,
deliveryProof = FALSE,
editorAdvancePending = TRUE,
canonicalAdvancedAfterProof = FALSE,
postProofRebases = 0,
deferredDelivery = TRUE,
replacementReplicaBootstrapped = FALSE,
replacementAckQueueEmpty = FALSE,
snapshotMatchesCapture = FALSE,
sessionCheckRecovered = FALSE,
retainedWriteInFlight = TRUE,
captureRetired = FALSE,
retirementAttempted = FALSE,
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

process PostProofEditor = "post_proof_editor"
begin
AdvanceAfterProof:
    await deliveryProof;
    canonicalAdvancedAfterProof := TRUE;
    editorAdvancePending := FALSE;
EditorDone:
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

process ReplacementReplica = "replacement_replica"
begin
BootstrapRetainedCanonical:
    await responseCopies = 1;
    replacementReplicaBootstrapped := TRUE;
    replacementAckQueueEmpty := TRUE;
ReplicaDone:
    while TRUE do
        skip;
    end while;
end process;

process SessionCheck = "session_check"
begin
AwaitReplacementBootstrap:
    await replacementReplicaBootstrapped /\ replacementAckQueueEmpty;
SettleHistoricalDeferredSlot:
    deferredDelivery := FALSE;
RefreshCapturedResponseSnapshot:
    snapshotMatchesCapture := TRUE;
CommitRecoveredCapture:
    assert cycleState = "rotated_open" /\ captureCopies = 1;
    cycleState := "committed_response";
    sessionCheckRecovered := TRUE;
committed := TRUE;
retainedWriteInFlight := FALSE;
SessionCheckDone:
    while TRUE do
        skip;
    end while;
end process;

process ConcurrentRepair = "concurrent_repair"
begin
ObservePartialResponseDuringRetainedWrite:
await responseCopies = 1;
retirementAttempted := TRUE;
if retainedWriteInFlight \/ committed then
skip;
else
captureRetired := TRUE;
end if;
RepairDone:
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
RecordDeliveryProof:
    deliveryProof := TRUE;
AwaitPostProofEditorCut:
    await ~editorAdvancePending;
RebaseSameIntent:
    if canonicalAdvancedAfterProof then
        postProofRebases := postProofRebases + 1;
        canonicalAdvancedAfterProof := FALSE;
    end if;
ApplySameCapture:
    assert cycleState = "rotated_open" /\ captureCopies = 1;
    responseCopies := responseCopies + 1;
    backlogMutationCopies := backlogMutationCopies + 1;
    fullResponseInHead := TRUE;
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
    /\ backlogMutationCopies \in Nat
    /\ deliveryProof \in BOOLEAN
    /\ editorAdvancePending \in BOOLEAN
    /\ canonicalAdvancedAfterProof \in BOOLEAN
    /\ postProofRebases \in Nat
    /\ deferredDelivery \in BOOLEAN
    /\ replacementReplicaBootstrapped \in BOOLEAN
    /\ replacementAckQueueEmpty \in BOOLEAN
    /\ snapshotMatchesCapture \in BOOLEAN
/\ sessionCheckRecovered \in BOOLEAN
/\ retainedWriteInFlight \in BOOLEAN
/\ captureRetired \in BOOLEAN
/\ retirementAttempted \in BOOLEAN
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
BacklogMutationAppliedAtMostOnce == backlogMutationCopies <= 1
PostProofAdvanceRebasesSameIntent ==
    /\ postProofRebases <= 1
    /\ postProofRebases = 1 =>
        /\ ~editorAdvancePending
        /\ ~canonicalAdvancedAfterProof
HeadingAloneNeverSkips == cycleState # "skipped_exact_duplicate"
QueueMismatchNeverBlocks == queueSyncDone =>
    /\ documentQueueMarked = documentQueueMatches
    /\ snapshotQueueMarked = snapshotQueueMatches
StrictUnmarkedNeverMutates == ~unmarkedApplied
PressureMarkerDoesNotChurn == pressureMarkerWrites <= 1
RetainedWriteCannotLoseCapture == retainedWriteInFlight => ~captureRetired
CommitRequiresObservedAckAndFullBody == committed =>
    /\ ackObserved
    /\ fullResponseInHead
    /\ responseCopies = 1
    /\ backlogMutationCopies = 1
    /\ postProofRebases = 1
    /\ queueSyncDone
    /\ replacementReplicaBootstrapped
    /\ replacementAckQueueEmpty
    /\ ~deferredDelivery
    /\ snapshotMatchesCapture
    /\ sessionCheckRecovered

EventuallyForegroundObservesLandedAck == <>ackObserved
EventuallyPartialQueueSyncCompletes == <>queueSyncDone
EventuallyMalformedResponseRejected == <>malformedRejected
EventuallySameCaptureCommitsExactlyOnce == <>(committed /\ responseCopies = 1)
EventuallyPostProofAdvanceRebases == <>(postProofRebases = 1)
EventuallyReplacementBootstrapSettlesSameCapture ==
    <>(sessionCheckRecovered /\ ~deferredDelivery /\ snapshotMatchesCapture)
EventuallyConcurrentRepairKeepsCapture == <>(retirementAttempted /\ ~captureRetired)

=============================================================================
