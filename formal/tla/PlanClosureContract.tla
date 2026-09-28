-------------------------- MODULE PlanClosureContract --------------------------
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************
What a dispatch plan may assert was completed.

WHY THIS MODULE EXISTS
----------------------
`VisibleDeliveryReceipt` and `EditorReplicaStrand` model LIVENESS wedges: a
reachable state with no outgoing transition. This module models the opposite
failure in the same system — a transition that is enabled when it should not be,
and whose effect is unrecoverable. A wedge costs time; a false closeout marks
unexecuted work complete and the evidence that it was never done is gone.

`agent-doc plan` emits `repo_actions`, `pending_mutations` and
`required_commands`. SKILL.md step 0d tells the agent those ARE the execution
contract, so a pre-filled `--done <id>` is not advice: an agent that follows the
contract literally closes that item.

The failure this module admits is the one the logs actually show. Measured on
`tasks/software/lazily.md` at cycle-1790447018749, after the operator set the
queue to stop:

  preflight: queue_active: false, queue_drainable_head_count: 0,
             queue_continuation_required: false, warn inactive_queue_residue
  plan:      repo_actions for all three inactive heads, and
             required_commands: agent-doc finalize ...
                 --done lzfakepublisherproof
                 --done lzartifactpurgeexec
                 --done lzdocorphanresidue

Preflight had already resolved the queue as inactive with ZERO drainable heads.
`plan` derived its contract from the queue PROSE instead, so three items no turn
executed were pre-marked complete — including an `[operator-verify]` item that
no agent turn can ever satisfy, which is the sharpest form of the defect: the
contract asserts a completion that is not merely unproven but impossible.

THE RULE
--------
A plan may pre-fill `--done <id>` only for an id this turn actually dispatched.
"Present in the queue component" is not dispatch. An inactive queue dispatches
nothing, so its residue — however many id-backed heads it holds — contributes no
`--done` at all.

`GateDoneOnDispatch` gates that rule so the same model checks both directions.
With it FALSE, `NeverClosesUnexecutedWork` must be VIOLATED — the non-vacuity
obligation, enforced by `PlanClosureContractWedge.cfg` and `scripts/run_tla.sh`'s
must-violate list. `PlanClosureContractReach.cfg` asserts the negation of a
genuine closeout and must also be violated, proving the fix did not simply stop
emitting `--done` for everything, which would satisfy the safety invariant
vacuously by making the plan useless.

Note what is deliberately ABSENT: any action that dispatches a head while the
queue is inactive. That is the whole point of the resolved activity state, and
modelling it would assume away the precondition the fix rests on.
***************************************************************************)

CONSTANTS
    Ids,                (* the id-backed heads a queue may hold *)
    OperatorGated,      (* the subset no agent turn can ever execute *)
    GateDoneOnDispatch  (* the fix under test *)

ASSUME OperatorGated \subseteq Ids

VARIABLES
    queueActive,  (* the resolved queue-activity state preflight reports *)
    residue,      (* id-backed heads still sitting in the queue component *)
    dispatched,   (* heads this turn actually executed *)
    planned,      (* the `--done` set the plan pre-filled *)
    completed     (* what closeout marked done by following the contract *)

vars == << queueActive, residue, dispatched, planned, completed >>

Init ==
    /\ queueActive = TRUE
    /\ residue = Ids
    /\ dispatched = {}
    /\ planned = {}
    /\ completed = {}

TypeOK ==
    /\ queueActive \in BOOLEAN
    /\ residue \subseteq Ids
    /\ dispatched \subseteq Ids
    /\ planned \subseteq Ids
    /\ completed \subseteq Ids

(*************************************************************************)
(* ENVIRONMENT. The operator stops the queue. The residue is untouched -   *)
(* that divergence between "resolved inactive" and "heads still present"   *)
(* IS the state the defect lives in.                                       *)
(*************************************************************************)
OperatorStopsQueue ==
    /\ queueActive
    /\ queueActive' = FALSE
    /\ UNCHANGED << residue, dispatched, planned, completed >>

(*************************************************************************)
(* A head is dispatched only from an ACTIVE queue, and never one the       *)
(* operator must satisfy in person. Both are properties of the resolved    *)
(* state, not of the queue prose.                                          *)
(*************************************************************************)
DispatchHead(id) ==
    /\ queueActive
    /\ id \in residue
    /\ id \notin OperatorGated
    /\ residue' = residue \ {id}
    /\ dispatched' = dispatched \union {id}
    /\ UNCHANGED << queueActive, planned, completed >>

(*************************************************************************)
(* THE DERIVATION UNDER TEST.                                             *)
(*                                                                        *)
(* Gated: the contract names exactly what this turn executed.             *)
(* Ungated: the contract is derived from the queue prose, so every head    *)
(* still sitting in the component is pre-marked complete - which is what   *)
(* produced three `--done` flags on a queue preflight had already resolved *)
(* as inactive with zero drainable heads.                                  *)
(*************************************************************************)
DerivePlan ==
    /\ planned' = IF GateDoneOnDispatch
                  THEN dispatched
                  ELSE dispatched \union residue
    /\ UNCHANGED << queueActive, residue, dispatched, completed >>

(* The agent follows the contract. This is not optional: SKILL.md step 0d *)
(* makes `required_commands` authoritative, so whatever the plan pre-fills *)
(* is what gets closed.                                                    *)
Closeout ==
    /\ completed' = completed \union planned
    /\ UNCHANGED << queueActive, residue, dispatched, planned >>

Next ==
    \/ OperatorStopsQueue
    \/ \E id \in Ids : DispatchHead(id)
    \/ DerivePlan
    \/ Closeout

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(DerivePlan)
    /\ WF_vars(Closeout)

(*************************************************************************)
(* SAFETY                                                                 *)
(*************************************************************************)

(* The load-bearing one. Closing work the turn did not execute destroys the *)
(* evidence that it was never done, which is why this is worse than a wedge.*)
NeverClosesUnexecutedWork == completed \subseteq dispatched

(* The sharpest case: an `[operator-verify]` id can never enter `dispatched` *)
(* by construction, so closing one is not merely unproven but impossible.    *)
NeverClosesOperatorGatedWork == completed \intersect OperatorGated = {}

(* A plan never asserts a completion for a head that is still queued and     *)
(* undispatched - "present in the component" is not dispatch.                *)
QueueResidueIsNotDispatch == planned \intersect residue = {}

(*************************************************************************)
(* REACH obligation (asserted negated in `PlanClosureContractReach.cfg`,   *)
(* which must be VIOLATED): a genuinely dispatched head still closes out.  *)
(* Without this, a plan that simply never emitted `--done` would satisfy   *)
(* every invariant above while being useless.                              *)
(*************************************************************************)
NeverClosesAnything == completed = {}

=============================================================================
