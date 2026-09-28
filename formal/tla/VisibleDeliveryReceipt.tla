------------------------- MODULE VisibleDeliveryReceipt -------------------------
EXTENDS Naturals, TLC

(***************************************************************************
The visible-delivery receipt ladder: how an unsaved editor cut reaches disk
when the one live replica stops converging.

WHY THIS MODULE EXISTS
----------------------
`EditorReplicaStrand` models the READ side of a definitively-refusing endpoint:
an attachment latch that outlives the endpoint's own refusal, and the demotion
edge that lets a resolve descend to disk. That fix landed. Its dual on the
WRITE side did not, and the same class of endpoint refusal wedges closeout
through a completely different predicate.

The failure this module admits is the one the logs actually show. Measured
2026-09-28T01:37:04Z through 02:15Z on `tasks/agent-doc/agent-doc-bugs.md`:

  crdt_replica_barrier_released_without_progress clients=5493558854380741 waits=12
  crdt_nonconverging_replica_recovery client_id=5493558854380741
      reregister=definitively_refused_by_all:1 found=1 notified=0
  crdt_commit_barrier ready=true delivery_required=false
      visible_delivery_projected=false live_editors=1
  controller_commit_projection_pending decision=NativeSaveRequired request_sent=false
  editor_projection_persistence_pending driver=editor_native_save
      save_diagnosis=native_save_gate_not_ready operator_action=none disk_write=false
  retained_write_blocks_new_cycle cause=authority_disk_diverged

...then `controller_crdt_replica_handled method=replica_pull` at ~4/s for the
next 38 minutes with the authority hash never moving. Every subsequent turn was
refused admission.

THE TWO PREDICATES
------------------
`RelayHub` derives two facts over the live membership cut:

  delivery_converged        == \A live m : ~HoldsBarrier(m)
  visible_delivery_projected == \A live m : m.pending = {}

  HoldsBarrier(m) == m.pending # {} /\ m.redeliveries <= R /\ m.waits <= W

They are deliberately firewalled. The first is an AVAILABILITY policy: after a
bounded non-convergence budget it releases one broken replica so it cannot block
unrelated work. The second is the RECEIPT, and only a receipt may authorize a
native save or a retained closeout. That firewall is correct and this module
keeps it (see `AvailabilityIsNotAReceipt`).

THE DEAD END
------------
The release is applied to ONE of them. Once a live member blows its budget with
a non-empty queue:

  * `HoldsBarrier` goes FALSE, so `delivery_converged` goes TRUE  - released;
  * `pending` is untouched, so `visible_delivery_projected` stays FALSE.

Nothing decrements the streaks (only an ACK or a fresh obligation on an empty
queue clears them), nothing drains `pending` (only an ACK does), and nothing
removes the member from the live cut. So `visible_delivery_projected` is
STABLY false. The native-save gate needs it, never fires, disk never receives
the canonical cut, the retained write never settles, and preflight refuses every
later cycle. A reachable state with no outgoing transition.

The one-shot `claim_nonconverging_recovery` is not an edge out. It asks the
endpoint to rebuild, and the endpoint ANSWERS AND REFUSES - `definitively_-
refused_by_all:1`. `ReplicaSignalOutcome::diagnosis` already classifies that
token apart from a delivery fault, and says in its own comment that "the
caller's correct response is to stop treating the endpoint as serving this
document, not to try again". No caller implemented it. The claim is one-shot, so
after the refusal nothing is retried, nothing escalates, and the replica keeps
vetoing the receipt forever.

THE MISSING EDGE
----------------
A definitive refusal from a live endpoint is PROOF that the endpoint no longer
serves this document - the same proof `EditorReplicaStrand` uses to demote a
stale attachment latch. Here it justifies dropping the replica from the DELIVERY
CUT: `RelayHub::disconnect`, which clears the delivery queue and the streaks,
marks the member not-live, and leaves its CRDT replica state intact so a genuine
`reconnect` still performs the bidirectional state-vector catch-up. Nothing
authoritative is lost: the pending queue holds updates flowing canonical ->
member, which is content the member is MISSING, never content only it holds.

`DropOnDefinitiveRefusal` gates that edge so the same model checks both
directions. With it FALSE, TLC must report the wedge - that is the non-vacuity
obligation, enforced by `VisibleDeliveryReceiptWedge.cfg` and
`scripts/run_tla.sh`'s must-violate list.

Note what is deliberately ABSENT: any action that restores `endpointServes`.
Reopening the editor tab is the operator action the design must not require, so
modelling it would assume away the very wedge. Progress must come from agent-doc
alone. Also absent: any action that makes `visible_delivery_projected` follow
`delivery_converged`. That is the tempting wrong fix - it would restore liveness
by deleting the receipt - and `AvailabilityIsNotAReceipt` rejects it.
***************************************************************************)

CONSTANTS
    MaxNonconvergence,      (* the streak budget; 50 redeliveries / 12 waits in production *)
    DropOnDefinitiveRefusal (* the fix under test *)

VARIABLES
    pending,            (* TRUE iff the live member has a queued undelivered update *)
    streak,             (* non-convergence evidence: redeliveries + expired barrier waits *)
    member,             (* "live" | "offline" - membership in the delivery cut *)
    endpointServes,     (* TRUE iff the editor endpoint will serve this document *)
    recoverySignalled,  (* the one-shot `claim_nonconverging_recovery` was taken *)
    refused,            (* the recovery answered `definitively_refused_by_all` *)
    disk,               (* "stale" | "current" - disk against the canonical authority *)
    savedWithoutReceipt (* history: a native save was authorized with no receipt *)

vars ==
    << pending, streak, member, endpointServes, recoverySignalled, refused,
       disk, savedWithoutReceipt >>

(*************************************************************************)
(* The two RelayHub predicates, transcribed from                          *)
(* `agent-doc-document-realtime/src/crdt_relay.rs`.                       *)
(*************************************************************************)
HoldsBarrier ==
    /\ member = "live"
    /\ pending
    /\ streak < MaxNonconvergence

DeliveryConverged == ~HoldsBarrier

VisibleDeliveryProjected == (member = "live") => ~pending

Init ==
    /\ pending = TRUE            (* a canonical cut was published to the editor *)
    /\ streak = 0
    /\ member = "live"
    /\ endpointServes = TRUE
    /\ recoverySignalled = FALSE
    /\ refused = FALSE
    /\ disk = "stale"
    /\ savedWithoutReceipt = FALSE

TypeOK ==
    /\ pending \in BOOLEAN
    /\ streak \in 0..MaxNonconvergence
    /\ member \in {"live", "offline"}
    /\ endpointServes \in BOOLEAN
    /\ recoverySignalled \in BOOLEAN
    /\ refused \in BOOLEAN
    /\ disk \in {"stale", "current"}
    /\ savedWithoutReceipt \in BOOLEAN

(*************************************************************************)
(* ENVIRONMENT. Not fair: TLC explores both the run where the endpoint    *)
(* keeps serving and the run where it stops. It stops while a delivery is *)
(* outstanding, which is the scope of this module; an endpoint that stops *)
(* with nothing queued is `EditorReplicaStrand`'s read-side ladder.       *)
(*************************************************************************)
EndpointStopsServing ==
    /\ endpointServes
    /\ pending
    /\ member = "live"
    /\ endpointServes' = FALSE
    /\ UNCHANGED << pending, streak, member, recoverySignalled, refused, disk,
                    savedWithoutReceipt >>

(*************************************************************************)
(* The observed ~4/s pull loop. Handing out the same unacked head again is *)
(* process liveness, not delivery progress, so it charges the streak. A    *)
(* refusing endpoint still pulls - that IS the production trace.           *)
(*************************************************************************)
PullWithoutAck ==
    /\ member = "live"
    /\ pending
    /\ streak < MaxNonconvergence
    /\ streak' = streak + 1
    /\ UNCHANGED << pending, member, endpointServes, recoverySignalled, refused,
                    disk, savedWithoutReceipt >>

(* Only a serving endpoint can ACK. An ACK drains the queue and clears the  *)
(* streaks - the sole way `pending` becomes empty without leaving the cut.  *)
AckDelivery ==
    /\ member = "live"
    /\ pending
    /\ endpointServes
    /\ pending' = FALSE
    /\ streak' = 0
    /\ UNCHANGED << member, endpointServes, recoverySignalled, refused, disk,
                    savedWithoutReceipt >>

(* `RelayHub::claim_nonconverging_recovery` - one-shot, and only once the   *)
(* member has been released from the barrier.                              *)
ClaimRecovery ==
    /\ member = "live"
    /\ pending
    /\ ~HoldsBarrier
    /\ ~recoverySignalled
    /\ recoverySignalled' = TRUE
    /\ UNCHANGED << pending, streak, member, endpointServes, refused, disk,
                    savedWithoutReceipt >>

(* The endpoint answers and rebuilds. *)
RecoveryAccepted ==
    /\ recoverySignalled
    /\ endpointServes
    /\ member = "live"
    /\ pending
    /\ pending' = FALSE
    /\ streak' = 0
    /\ UNCHANGED << member, endpointServes, recoverySignalled, refused, disk,
                    savedWithoutReceipt >>

(* The endpoint answers and REFUSES - a receipt, not a timeout. This is the *)
(* `definitively_refused_by_all:1` the logs record.                         *)
RecoveryDefinitivelyRefused ==
    /\ recoverySignalled
    /\ ~endpointServes
    /\ ~refused
    /\ refused' = TRUE
    /\ UNCHANGED << pending, streak, member, endpointServes, recoverySignalled,
                    disk, savedWithoutReceipt >>

(*************************************************************************)
(* THE MISSING EDGE. A definitive refusal drops the replica from the      *)
(* delivery cut. `~HoldsBarrier` is required so a replica still inside its *)
(* budget - still converging - is never preempted, the same way the read   *)
(* side consults a refusal only after the retry budget is spent.           *)
(*************************************************************************)
DropRefusedFromDeliveryCut ==
    /\ DropOnDefinitiveRefusal
    /\ refused
    /\ ~endpointServes
    /\ member = "live"
    /\ pending
    /\ ~HoldsBarrier
    /\ member' = "offline"
    /\ pending' = FALSE      (* `disconnect` clears the delivery queue ... *)
    /\ streak' = 0           (* ... and the non-convergence streaks.       *)
    /\ UNCHANGED << endpointServes, recoverySignalled, refused, disk,
                    savedWithoutReceipt >>

(*************************************************************************)
(* The editor projects its buffer to disk. Gated on the RECEIPT, never on  *)
(* availability. Re-assertable, so a settled document self-loops rather    *)
(* than reporting a spurious deadlock.                                     *)
(*************************************************************************)
NativeSave ==
    /\ member = "live"
    /\ endpointServes
    /\ VisibleDeliveryProjected
    /\ disk' = "current"
    /\ savedWithoutReceipt' = (savedWithoutReceipt \/ ~VisibleDeliveryProjected)
    /\ UNCHANGED << pending, streak, member, endpointServes, recoverySignalled,
                    refused >>

(* With no live editor in the cut, agent-doc's own authority write owns disk. *)
DetachedWrite ==
    /\ member = "offline"
    /\ disk' = "current"
    /\ UNCHANGED << pending, streak, member, endpointServes, recoverySignalled,
                    refused, savedWithoutReceipt >>

Next ==
    \/ EndpointStopsServing
    \/ PullWithoutAck
    \/ AckDelivery
    \/ ClaimRecovery
    \/ RecoveryAccepted
    \/ RecoveryDefinitivelyRefused
    \/ DropRefusedFromDeliveryCut
    \/ NativeSave
    \/ DetachedWrite

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(PullWithoutAck)
    /\ WF_vars(AckDelivery)
    /\ WF_vars(ClaimRecovery)
    /\ WF_vars(RecoveryAccepted)
    /\ WF_vars(RecoveryDefinitivelyRefused)
    /\ WF_vars(DropRefusedFromDeliveryCut)
    /\ WF_vars(NativeSave)
    /\ WF_vars(DetachedWrite)

(*************************************************************************)
(* SAFETY                                                                 *)
(*************************************************************************)

(* The firewall from `#silentreplicabarrier`: an availability release is not *)
(* a receipt. A live member holding a queued update must keep the receipt    *)
(* FALSE even once the barrier has let it go. This rejects the tempting      *)
(* wrong fix of defining the receipt as delivery convergence - that would    *)
(* restore liveness by deleting the proof.                                   *)
AvailabilityIsNotAReceipt ==
    (DeliveryConverged /\ member = "live" /\ pending) => ~VisibleDeliveryProjected

(* A native save is never authorized by anything weaker than the receipt.    *)
(* Always FALSE while `NativeSave` keeps its guard; it fires the moment a    *)
(* refactor weakens that guard, which is the regression this encodes.        *)
NeverSavesWithoutReceipt == ~savedWithoutReceipt

(* Dropping is only ever justified by a definitive refusal. This is what     *)
(* stops the fix from becoming a disguised `--force-disk`: the detached      *)
(* write path is reachable only behind proof the endpoint stopped serving.   *)
DropRequiresDefinitiveRefusal ==
    (member = "offline") => (refused /\ ~endpointServes)

(*************************************************************************)
(* LIVENESS - the property the production wedge violates.                 *)
(*                                                                        *)
(* The canonical cut must reach disk WITHOUT any operator action. With     *)
(* `DropOnDefinitiveRefusal = FALSE` the refused-and-released state has no *)
(* outgoing transition, which is the point.                                *)
(*************************************************************************)
CanonicalAlwaysEventuallyReachesDisk ==
    []<>(disk = "current")

(*************************************************************************)
(* REACH obligation (asserted negated in `VisibleDeliveryReceiptReach.cfg`,*)
(* which must be VIOLATED): a save through a still-serving live editor is  *)
(* still reachable. Without this, a fix that routed every document through *)
(* the drop/detached path would satisfy the liveness property while having *)
(* silently abandoned the editor-native save.                              *)
(*************************************************************************)
NeverSavesThroughLiveEditor == ~(disk = "current" /\ member = "live")

=============================================================================
