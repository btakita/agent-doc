-------------------------- MODULE ResponseCheckpoint --------------------------
EXTENDS Naturals, Sequences, TLC

CONSTANTS MaxCheckpoints, KeyBurst

(***************************************************************************
Semantic response checkpoints are cumulative document cells. Each checkpoint
replaces the prior uncommitted response tail, while arbitrary operator typing is
preserved. A retained replay may transiently duplicate a protocol boundary, but
normalization must run before integrity/seal. Queue mutation and commit belong
only to the seal transition.
***************************************************************************)

(* --fair algorithm ResponseTransaction
variables
    producedSeq = 0,
    visibleSeq = 0,
    responseCopies = 0,
    operatorChars = 0,
    boundaryCount = 1,
    normalized = TRUE,
    sealed = FALSE,
    queueMutated = FALSE,
    committed = FALSE;

process Operator = "operator"
begin
RepeatKeys:
    while operatorChars < KeyBurst do
        operatorChars := operatorChars + 1;
    end while;
OperatorDone:
    while TRUE do
        skip;
    end while;
end process;

process Writer = "writer"
begin
CheckpointLoop:
    while producedSeq < MaxCheckpoints do
        producedSeq := producedSeq + 1;
        visibleSeq := producedSeq;
        responseCopies := 1;
    end while;
ReplayBoundaryTransient:
    boundaryCount := 2;
    normalized := FALSE;
NormalizeBeforeIntegrity:
    boundaryCount := 1;
    normalized := TRUE;
Seal:
    await visibleSeq = MaxCheckpoints /\ normalized /\ boundaryCount = 1
        /\ operatorChars = KeyBurst;
    sealed := TRUE;
    queueMutated := TRUE;
    committed := TRUE;
WriterDone:
    while TRUE do
        skip;
    end while;
end process;
end algorithm; *)

TypeOK ==
    /\ producedSeq \in 0..MaxCheckpoints
    /\ visibleSeq \in 0..MaxCheckpoints
    /\ responseCopies \in 0..1
    /\ operatorChars \in 0..KeyBurst
    /\ boundaryCount \in 1..2
    /\ normalized \in BOOLEAN
    /\ sealed \in BOOLEAN
    /\ queueMutated \in BOOLEAN
    /\ committed \in BOOLEAN

VisibleCheckpointNeverLeadsProduced == visibleSeq <= producedSeq
CheckpointReplacesInsteadOfAppending == responseCopies <= 1
QueueMutationRequiresSeal == queueMutated => sealed
CommitRequiresLatestNormalizedCell ==
    committed =>
        /\ sealed
        /\ queueMutated
        /\ visibleSeq = MaxCheckpoints
        /\ responseCopies = 1
        /\ normalized
        /\ boundaryCount = 1
        /\ operatorChars = KeyBurst

EventuallyAllOperatorTypingVisible == <> (operatorChars = KeyBurst)
EventuallyCommitted == <> committed

=============================================================================
