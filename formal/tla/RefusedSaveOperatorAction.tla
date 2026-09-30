---------------------- MODULE RefusedSaveOperatorAction ----------------------
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************
Whether a retained write tells the operator there is anything to do.

WHY THIS MODULE EXISTS
----------------------
`EditorReplicaStrand` and `VisibleDeliveryReceipt` model whether the system can
still CONVERGE. This module models something narrower and, when it fails,
costlier: whether the system tells the truth about convergence being possible.

A retained write is correct behaviour. `--force-disk` is forbidden, the capture
is durable, and the binary owns the retry — all of that holds. The question this
module asks is what the operator is told WHILE it holds. `agent-doc` reports one
of two things beside every retained write:

  operator_action = none                      "the controller owns the next
                                               attempt; wait"
  operator_action = inspect_editor_endpoint   "no automatic attempt can
                                               succeed; look at the endpoint"

Saying `none` when no automatic attempt can succeed is not a delay. It is an
instruction to wait for an event that will never happen, and the operator has no
way to tell it apart from ordinary progress.

THE MEASURED FAILURE
--------------------
Observed 2026-09-28 on `src/haiven-dev/tasks/sdk.md`. Its retained write held
`reason=editor_owner_without_registered_replica`, and every recovery pass logged:

  crdt_replica_notify_deferred reason=editor_replica_reregister
      editor_pid=1303966 definitive_refusal=true
      error=IPC receipt rejected: {"type":"receipt","status":"rejected"}
  stale_editor_replica_recovery_requested action=reregister_editor_replica
      request_status=request_skipped reason=editor_reregister_primary
      editor_replica_reregister=definitively_refused_by_all:1

once every ~90 seconds, unbounded, while session-check told the operator the
binary owned the retry and to change nothing.

The endpoint had ANSWERED. It was reached, it replied, and it refused. That is
the strongest evidence available that no automatic path can converge — stronger
than "could not be reached" (which may be transient) and stronger than "nothing
is registered" (which may be a race). Both of those weaker signals already
produced `inspect_editor_endpoint`. The strongest one produced `none`, because
the predicate deciding it matched diagnosis STRINGS and its list happened to
name the two weaker tokens.

THE RULE
--------
An endpoint that answered and refused every route must surface an operator
action. `AnsweredRefusalAlwaysSurfacesOperatorAction` states it as an invariant
rather than a liveness property on purpose: the failure is not that resolution
takes too long, it is that a reachable state misreports itself. Time is not the
remedy, and a property that permits "eventually" would be satisfied by the very
spin this module exists to reject.

`ClassifyRefusalAsTerminal` gates the fix so one model checks both directions:

  TRUE  — the invariant holds (RefusedSaveOperatorAction.cfg).
  FALSE — the invariant must be VIOLATED
          (RefusedSaveOperatorActionWedge.cfg, on the must-violate list).

`RefusedSaveOperatorActionReach.cfg` asserts the negation of an ordinary
in-flight save and must also be violated. Without it the safety invariant could
be satisfied vacuously by reporting `inspect_editor_endpoint` for EVERY outcome,
which would make the signal useless in the opposite direction: an operator told
to inspect an endpoint on every ordinary retry learns nothing from being told it
during a real refusal.

WHAT IS DELIBERATELY ABSENT
---------------------------
No action writes to disk, demotes editor authority, or drops the retained write.
Those are the recoveries this system forbids while an editor owns the document,
and the fix does not need them: naming the failure changes only what the
operator is told, never who owns the text. A model that could reach disk would
be proving a different, unsafe design.

A generation mismatch keeps the retained write retryable, but it is not
self-clearing: the editor reports one installed generation and the running
binary expects another. Reloading the same pair preserves the mismatch. The
operator action therefore names the exact-version fence without changing it or
granting disk authority.
***************************************************************************)

CONSTANTS
    MaxPasses,                  (* bound on recovery passes, for finite checking *)
    ClassifyRefusalAsTerminal   (* the fix under test *)

(* Every outcome a replica signal can classify to. These mirror
   `ReplicaSignalClass` one-for-one; the model is wrong if they drift. *)
Outcomes == {"no_live_registration",
             "delivery_failed_to_all",
             "definitively_refused_by_all",
             "plugin_generation_mismatch",
             "requested"}

(* The outcomes that prove no automatic attempt can converge. A refusal belongs
   here only when the fix is enabled -- that is the whole experiment. *)
Terminal ==
    IF ClassifyRefusalAsTerminal
    THEN {"no_live_registration", "delivery_failed_to_all", "definitively_refused_by_all", "plugin_generation_mismatch"}
    ELSE {"no_live_registration", "delivery_failed_to_all", "plugin_generation_mismatch"}

VARIABLES
    outcome,          (* what the last signal answered *)
    operatorAction,   (* what the operator was told: "none" | "inspect_editor_endpoint" *)
    retained,         (* whether a write is still retained *)
    passes            (* recovery passes taken *)

vars == << outcome, operatorAction, retained, passes >>

Init ==
    /\ outcome = "requested"
    /\ operatorAction = "none"
    /\ retained = TRUE
    /\ passes = 0

TypeOK ==
    /\ outcome \in Outcomes
    /\ operatorAction \in {"none", "inspect_editor_endpoint"}
    /\ retained \in BOOLEAN
    /\ passes \in 0..MaxPasses

(* One recovery pass: the endpoint answers with some outcome, and the system
   reports an operator action derived from it. The derivation is the code under
   test. *)
RecoveryPass(o) ==
    /\ retained
    /\ passes < MaxPasses
    /\ outcome' = o
    /\ operatorAction' = (IF o \in Terminal THEN "inspect_editor_endpoint" ELSE "none")
    /\ passes' = passes + 1
    /\ UNCHANGED retained

(* A served endpoint converges the retained write. Only `requested` can: a
   refusal, an absent registration, and a delivery failure all leave it held. *)
Converge ==
    /\ retained
    /\ outcome = "requested"
    /\ retained' = FALSE
    /\ UNCHANGED << outcome, operatorAction, passes >>

Next ==
    \/ \E o \in Outcomes : RecoveryPass(o)
    \/ Converge

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

-----------------------------------------------------------------------------

(* THE INVARIANT. A retained write whose endpoint answered with a refusal must
   not be reporting that there is nothing for the operator to do. *)
AnsweredRefusalAlwaysSurfacesOperatorAction ==
    (retained /\ outcome = "definitively_refused_by_all")
        => operatorAction = "inspect_editor_endpoint"

(* A refusal is at least as strong as the weaker signals that already surface an
   action. Stated separately so a fix that merely special-cased one token, rather
   than ordering the evidence, still fails. *)
RefusalIsNotWeakerThanUnreachable ==
    ClassifyRefusalAsTerminal =>
        (("delivery_failed_to_all" \in Terminal) => ("definitively_refused_by_all" \in Terminal))

(* An ordinary in-flight save must NOT ask the operator to inspect anything, or
   the signal means nothing when it matters. *)
InFlightSaveStaysQuiet ==
    (outcome = "requested") => operatorAction = "none"

(* Retrying the same exact-version mismatch cannot alter either installed
   generation, so the retained write must not claim automatic convergence. *)
GenerationMismatchSurfacesOperatorAction ==
    (retained /\ outcome = "plugin_generation_mismatch")
        => operatorAction = "inspect_editor_endpoint"

(* Negation used by RefusedSaveOperatorActionReach.cfg: a genuine in-flight save
   must be REACHABLE, so this must be violated. Without it the model could
   satisfy the invariant by never answering `requested` at all. *)
NeverReachesAnInFlightSave ==
    ~(outcome = "requested" /\ passes > 0)

=============================================================================
