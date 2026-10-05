------------------------ MODULE EditorTransportHealth ------------------------
EXTENDS Naturals, TLC

(***************************************************************************
The durable editor transport-health episode used by socket writes, CRDT
replica notifications and native editor saves.

States are represented by the health tuple plus `lastOutcome`:

  success              row absent; every episode-local vote is cleared
  timeout              failure grows, refusal run resets
  definitive_rejection failure and refusal run both grow
  degraded             failures reached DegradeThreshold
  recycle_attempted    one-shot latch, monotone inside an episode
  unregistered         refusal run reached UnregisterThreshold
  recovered            the next proven success returns to the initial tuple

SQLite is an idempotent effect sink. `DuplicatePersist` may repeat an upsert but
does not alter the health tuple. No transition waits or retries; each observed
outcome causes at most one bounded transition.

`RecordDefinitiveRefusal` gates the GH #138 fix. The positive config checks the
real transition. The wedge config disables only that edge and MUST violate
`AnsweredRefusalsAreCounted`, proving the refusal accounting is load-bearing.
***************************************************************************)

CONSTANTS MaxFailures, DegradeThreshold, UnregisterThreshold,
          RecordDefinitiveRefusal

VARIABLES rowPresent, failures, refusals, degraded, recycleAttempted,
          endpoint, lastOutcome, answeredRefusals, duplicatePersists

vars == << rowPresent, failures, refusals, degraded, recycleAttempted,
          endpoint, lastOutcome, answeredRefusals, duplicatePersists >>

Init ==
    /\ rowPresent = FALSE
    /\ failures = 0
    /\ refusals = 0
    /\ degraded = FALSE
    /\ recycleAttempted = FALSE
    /\ endpoint = "registered"
    /\ lastOutcome = "success"
    /\ answeredRefusals = 0
    /\ duplicatePersists = 0

TypeOK ==
    /\ rowPresent \in BOOLEAN
    /\ failures \in 0..MaxFailures
    /\ refusals \in 0..MaxFailures
    /\ degraded \in BOOLEAN
    /\ recycleAttempted \in BOOLEAN
    /\ endpoint \in {"registered", "unregistered"}
    /\ lastOutcome \in {"success", "timeout", "definitive_rejection",
                         "recycle_attempted", "duplicate_persist"}
    /\ answeredRefusals \in 0..MaxFailures
    /\ duplicatePersists \in 0..MaxFailures

DefinitiveRejectionRecorded ==
    /\ RecordDefinitiveRefusal
    /\ failures < MaxFailures
    /\ refusals < MaxFailures
    /\ rowPresent' = TRUE
    /\ failures' = failures + 1
    /\ refusals' = refusals + 1
    /\ degraded' = (degraded \/ failures' >= DegradeThreshold
                      \/ refusals' >= UnregisterThreshold)
    /\ UNCHANGED recycleAttempted
    /\ endpoint' = IF refusals' >= UnregisterThreshold
                   THEN "unregistered" ELSE "registered"
    /\ lastOutcome' = "definitive_rejection"
    /\ answeredRefusals' = answeredRefusals + 1
    /\ duplicatePersists' = 0

DefinitiveRejectionDropped ==
    /\ ~RecordDefinitiveRefusal
    /\ answeredRefusals < MaxFailures
    /\ lastOutcome' = "definitive_rejection"
    /\ answeredRefusals' = answeredRefusals + 1
    /\ duplicatePersists' = 0
    /\ UNCHANGED << rowPresent, failures, refusals, degraded,
                     recycleAttempted, endpoint >>

DefinitiveRejection ==
    DefinitiveRejectionRecorded \/ DefinitiveRejectionDropped

Timeout ==
    /\ failures < MaxFailures
    /\ rowPresent' = TRUE
    /\ failures' = failures + 1
    /\ refusals' = 0
    /\ degraded' = (degraded \/ failures' >= DegradeThreshold)
    /\ UNCHANGED recycleAttempted
    /\ endpoint' = "registered"
    /\ lastOutcome' = "timeout"
    /\ answeredRefusals' = 0
    /\ duplicatePersists' = 0

Recycle ==
    /\ rowPresent
    /\ degraded
    /\ ~recycleAttempted
    /\ recycleAttempted' = TRUE
    /\ lastOutcome' = "recycle_attempted"
    /\ duplicatePersists' = 0
    /\ UNCHANGED << rowPresent, failures, refusals, degraded, endpoint,
                     answeredRefusals >>

DuplicatePersist ==
    /\ rowPresent
    /\ duplicatePersists < MaxFailures
    /\ duplicatePersists' = duplicatePersists + 1
    /\ lastOutcome' = "duplicate_persist"
    /\ UNCHANGED << rowPresent, failures, refusals, degraded,
                     recycleAttempted, endpoint, answeredRefusals >>

Success ==
    /\ rowPresent
    /\ rowPresent' = FALSE
    /\ failures' = 0
    /\ refusals' = 0
    /\ degraded' = FALSE
    /\ recycleAttempted' = FALSE
    /\ endpoint' = "registered"
    /\ lastOutcome' = "success"
    /\ answeredRefusals' = 0
    /\ duplicatePersists' = 0

Next ==
    \/ DefinitiveRejection
    \/ Timeout
    \/ Recycle
    \/ DuplicatePersist
    \/ Success

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------

AnsweredRefusalsAreCounted == answeredRefusals <= refusals

DegradationThresholdIsReliable ==
    (rowPresent /\ failures >= DegradeThreshold) => degraded

RecycleRequiresDegradation == recycleAttempted => degraded

UnregistrationMatchesThreshold ==
    endpoint = "unregistered"
        <=> (rowPresent /\ refusals >= UnregisterThreshold)

SuccessRecoversTheEpisode ==
    lastOutcome = "success" =>
        (~rowPresent /\ failures = 0 /\ refusals = 0 /\ ~degraded
         /\ ~recycleAttempted /\ endpoint = "registered")

(* Negation for the reach config: a correct model MUST violate this. *)
NeverReachesUnregistered == endpoint # "unregistered"

=============================================================================
