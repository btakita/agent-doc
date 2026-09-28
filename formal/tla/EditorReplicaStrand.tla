-------------------------- MODULE EditorReplicaStrand --------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Authority resolution for an attached document whose Lazily replica was dropped
by a cdylib generation swap.

WHY THIS MODULE EXISTS
----------------------
`JetBrainsFileCache` claimed `EventuallyConverged` for editor-first
reload/reregister and passed, while production wedged for months. Its
`ReregisterFromExactEditorCut` step was an unconditional assignment:
re-registration could not fail there, so convergence was an axiom of the model
rather than a property of the design, and every fix aimed at a symptom kept being
certified by a model that had assumed the failure away.

That module now admits the rejection too, and ships its own must-violate config.
The two are kept separate because they model different scopes: `JetBrainsFileCache`
covers the editor-first reconnect and granular retained-intent replay — what a
published cut must contain — while this module covers the authority-resolution
ladder underneath it: the bounded re-registration budget, the attachment latch,
and the precedence between editor and disk. The wedge is only a state-graph
deadlock at THIS granularity, because only here are the retry budget and the
latch explicit.

The failure this module admits is the one the logs actually show. Measured
2026-09-27 18:28:54-18:28:58Z on `tasks/agent-doc/agent-doc-bugs.md` (and four
sibling documents) one minute after a mid-session `make install`:

  editor_replica_rebuild_requested pid=3129637
  crdt_replica_notify_deferred reason=editor_replica_reregister
      error=IPC receipt rejected: {"type":"receipt","status":"rejected"}   x3
  editor_replica_reregister_attempt attempt=1/3 .. 3/3 reregister=not_delivered
  realtime_doc_resolve_missing_replica_terminal_rebuild_failed
  realtime_doc_resolve_disk_read_refused editor_open=true
      invariant=attached_editor_never_descends_to_disk

The endpoint is ALIVE — it returns a receipt — and it REJECTS. That is a
definitive negative answer, not a timeout, and it is the state the design has no
transition out of.

THE DEAD END
------------
Three facts coexist:

  * `registration = "attached"`   - the attachment latch still says attached
  * `replica = "missing"`         - the generation swap dropped the replica
  * `~endpointServes`             - the old instance refuses this document

The disk refusal is CORRECT: unsaved editor content is not on disk, so reading
disk while something is serving the buffer would drop it. But the refusal is
keyed on the *latch*, and the latch outlives the endpoint's own refusal to
serve. So `ResolveOnEditor` is disabled (no replica) and `DescendToDisk` is
disabled (latch says attached) and nothing else is enabled: a reachable state
with no outgoing transition. Only an operator reopening the editor tab clears
it, which is why every in-binary "recovery" was a band-aid — the state machine
is not total, and no amount of retrying an exhausted path adds an edge.

THE MISSING EDGE
----------------
A definitive rejection from a live endpoint is PROOF that the endpoint no longer
serves this document. That proof is exactly what the latch is missing, so it may
demote the stale registration to detached. Disk descent then becomes legal by the
invariant's own terms - the editor is *proven* not serving, not merely assumed
detached - and the operator's unsaved buffer is not lost: it is still in the IDE,
and re-registration merges it back (see the admission three-way merge, which reconciles
a disk/authority split by three-way merge instead of refusing).

`DemoteOnDefinitiveRejection` gates that edge so the same model checks both
directions. With it FALSE, `AuthorityAlwaysEventuallyResolves` must be VIOLATED -
that is the non-vacuity obligation, enforced by `EditorReplicaStrandWedge.cfg`
and `scripts/run_tla.sh`'s must-violate list. A property that cannot fail is not
evidence, which is the lesson this whole module encodes.

2026-09-28 -- THE SAME FLAW, ON THE ACCEPTANCE EDGE
--------------------------------------------------
This module was written because `JetBrainsFileCache` made re-registration an
unconditional assignment, so convergence was an axiom rather than a property. It
then did the same thing one edge over: `ReregisterAccepted` was guarded by
`endpointServes`, which makes "the endpoint accepted" imply "the replica was
rebuilt" BY CONSTRUCTION. The production shape it assumed away is the common one
after a cdylib reload -- the editor process is alive, answers its socket, serves
every other document, accepts each re-registration request, and never produces a
model for THIS document, because the one it would serve from belonged to the
retired native generation.

That lands in neither existing bucket. It is not a refusal, so no proof is
recorded; it is not silence, so `notified > 0` reads as progress and the caller
waits. In the binary the re-registration loop spent its whole budget, wrote
`self_heal_exhausted` (which only stops the retrying), and nothing ever promoted
that to a fact `decide_authority_recovery` could leave `FailClosed` on. Measured
2026-09-28 on four attached documents after one `make install`; the recovery was
an operator reopening each tab, which is the action this module's own closing
note says the design must not require.

`endpointAccepts` is now separate from `endpointServes` so the two can disagree,
which is the whole point. `ReregisterUnanswered` keeps the other half honest: an
endpoint that never answers is never demoted, however long it stays silent, and
it deliberately does not spend the budget.

Note what is deliberately ABSENT: any action that restores `endpointServes`.
Reopening the editor tab is the operator action the design must not require, so
modelling it would reintroduce the same assumption that made the existing model
vacuous. Progress here must come from agent-doc alone.
***************************************************************************)

CONSTANTS
    MaxReregisterAttempts,         (* bounded re-registration budget, 3 in production *)
    DemoteOnDefinitiveRejection,   (* the original fix: an ANSWERED refusal *)
    DemoteOnAcceptedWithoutServing (* `#acceptedneverserved`: an answered YES that never lands *)

(*************************************************************************)
(* How the live endpoint behaves toward THIS document. One mode, because  *)
(* these are mutually exclusive observations: a request is refused, or    *)
(* accepted, or unanswered. Modelling "accepts" and "serves" as separate  *)
(* booleans let both the rejection and the acceptance action fire in the  *)
(* same state, and the rejection edge then rescued the acceptance case -  *)
(* which would have made the per-edge wedge below pass vacuously.         *)
(*                                                                        *)
(*   "serves"   - answers and rebuilds the replica                        *)
(*   "refuses"  - answers with a rejection (receipt, exhausted mismatch)  *)
(*   "accepts"  - answers YES and never produces a model for this doc     *)
(*   "silent"   - nothing answers                                         *)
(*************************************************************************)
Modes == {"serves", "refuses", "accepts", "silent"}
NotServing == {"refuses", "accepts", "silent"}

VARIABLES
    replica,     (* "present" | "missing" - agent-doc's Lazily replica *)
    mode,        (* how the endpoint behaves toward this document *)
    registration,(* "attached" | "detached" - the attachment latch *)
    attempts,    (* re-registration attempts spent *)
    refusals,    (* attempts the endpoint ANSWERED with a rejection *)
    accepted,    (* attempts the endpoint ANSWERED with a yes *)
    authority    (* "none" | "editor" | "disk" - what a resolve produced *)

vars == << replica, mode, registration, attempts, refusals, accepted, authority >>

Init ==
    /\ replica = "present"
    /\ mode = "serves"
    /\ registration = "attached"
    /\ attempts = 0
    /\ refusals = 0
    /\ accepted = 0
    /\ authority = "editor"

TypeOK ==
    /\ replica \in {"present", "missing"}
    /\ mode \in Modes
    /\ registration \in {"attached", "detached"}
    /\ attempts \in 0..MaxReregisterAttempts
    /\ refusals \in 0..MaxReregisterAttempts
    /\ accepted \in 0..MaxReregisterAttempts
    /\ authority \in {"none", "editor", "disk"}

(*************************************************************************)
(* A cdylib generation swap drops the replica and leaves the OLD endpoint  *)
(* unable to serve this document. The attachment latch is untouched -      *)
(* that divergence between latch and endpoint IS the bug. Which non-serving*)
(* mode it lands in is nondeterministic: TLC explores all three.           *)
(*************************************************************************)
LibraryReload ==
    /\ replica = "present"
    /\ replica' = "missing"
    /\ mode' \in NotServing
    /\ authority' = "none"
    /\ UNCHANGED << registration, attempts, refusals, accepted >>

(* There is deliberately NO action that changes the failure mode, and none that
   restores serving. `LibraryReload` already picks nondeterministically among the
   three non-serving modes, so every one is explored with no coverage lost; an
   additional shift action only added an infinite oscillation that starves the
   fair recovery actions and reports a liveness violation which is an artifact of
   the model rather than a fact about the design. The design must recover from a
   failure mode that never changes, which is the observed case: a retired native
   generation does not start serving again on its own, and reopening the tab is
   the operator action this module exists to eliminate. *)

(* The endpoint answers and rebuilds. Unreachable after a reload, by design. *)
ReregisterAccepted ==
    /\ replica = "missing"
    /\ mode = "serves"
    /\ replica' = "present"
    /\ attempts' = 0
    /\ refusals' = 0
    /\ accepted' = 0
    /\ UNCHANGED << mode, registration, authority >>

(* The endpoint answers and REJECTS - a receipt, not a timeout. *)
ReregisterRejected ==
    /\ replica = "missing"
    /\ mode = "refuses"
    /\ attempts < MaxReregisterAttempts
    /\ attempts' = attempts + 1
    /\ refusals' = refusals + 1
    /\ UNCHANGED << replica, mode, registration, accepted, authority >>

(* `#acceptedneverserved` - the shape this module used to assume away.
   The endpoint answers YES and the replica never appears, because the model it
   would serve from belonged to the retired native generation. Not a refusal, so
   nothing was proven; not silence, so `notified > 0` read as progress. *)
ReregisterAcceptedWithoutServing ==
    /\ replica = "missing"
    /\ mode = "accepts"
    /\ attempts < MaxReregisterAttempts
    /\ attempts' = attempts + 1
    /\ accepted' = accepted + 1
    /\ UNCHANGED << replica, mode, registration, refusals, authority >>

(* Nothing answered. Spends a loop iteration exactly like the others - the
   binary's budget counts attempts, not answers - but increments NEITHER counter.
   That is the whole safety margin: absence of an answer is not evidence that no
   answer will come, so a budget spent entirely on silence proves nothing and
   demoting on it would make the latch's exit a silent `--force-disk`. *)
ReregisterUnanswered ==
    /\ replica = "missing"
    /\ mode = "silent"
    /\ attempts < MaxReregisterAttempts
    /\ attempts' = attempts + 1
    /\ UNCHANGED << replica, mode, registration, refusals, accepted, authority >>

ResolveOnEditor ==
    /\ replica = "present"
    /\ authority' = "editor"
    /\ UNCHANGED << replica, mode, registration, attempts, refusals, accepted >>

(* Legal only once the editor is PROVEN not serving. *)
DescendToDisk ==
    /\ replica = "missing"
    /\ registration = "detached"
    /\ authority' = "disk"
    /\ UNCHANGED << replica, mode, registration, attempts, refusals, accepted >>

(* The original edge: an answered rejection demotes the stale latch. *)
DemoteOnRejection ==
    /\ DemoteOnDefinitiveRejection
    /\ registration = "attached"
    /\ replica = "missing"
    /\ refusals > 0
    /\ attempts = MaxReregisterAttempts
    /\ registration' = "detached"
    /\ UNCHANGED << replica, mode, attempts, refusals, accepted, authority >>

(* The second edge. The endpoint ANSWERED - repeatedly - and still holds no
   model for this document across a spent budget. A statement about what the
   endpoint reported, not about how long we waited: `accepted > 0` is reachable
   only through `ReregisterAcceptedWithoutServing`, never through silence. *)
DemoteOnAcceptanceWithoutService ==
    /\ DemoteOnAcceptedWithoutServing
    /\ registration = "attached"
    /\ replica = "missing"
    /\ accepted > 0
    /\ attempts = MaxReregisterAttempts
    /\ registration' = "detached"
    /\ UNCHANGED << replica, mode, attempts, refusals, accepted, authority >>

Next ==
    \/ LibraryReload
    \/ ReregisterAccepted
    \/ ReregisterRejected
    \/ ReregisterAcceptedWithoutServing
    \/ ReregisterUnanswered
    \/ ResolveOnEditor
    \/ DescendToDisk
    \/ DemoteOnRejection
    \/ DemoteOnAcceptanceWithoutService

(*************************************************************************)
(* Every recovery action is weakly fair. `LibraryReload` is NOT fair - it *)
(* is an environment event that may or may not happen; TLC explores both. *)
(*************************************************************************)
Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ReregisterAccepted)
    /\ WF_vars(ReregisterRejected)
    /\ WF_vars(ReregisterAcceptedWithoutServing)
    /\ WF_vars(ReregisterUnanswered)
    /\ WF_vars(ResolveOnEditor)
    /\ WF_vars(DescendToDisk)
    /\ WF_vars(DemoteOnRejection)
    /\ WF_vars(DemoteOnAcceptanceWithoutService)

(*************************************************************************)
(* SAFETY                                                                *)
(*************************************************************************)

(* The invariant the disk refusal exists to protect, stated in terms of what  *)
(* is actually being protected (a serving endpoint) rather than the latch.    *)
NeverReadsDiskWhileEndpointServes ==
    (authority = "disk") => mode # "serves"

(* Demotion is only ever justified by proof. This is what stops either edge   *)
(* from becoming a disguised `--force-disk`.                                  *)
DemotionRequiresProof ==
    (registration = "detached") => mode # "serves"

(* The sharper half, and what the new edge has to earn: an endpoint that never *)
(* answered is never demoted. Both counters are reachable only through an      *)
(* action the endpoint participated in, so this says demotion follows          *)
(* something REPORTED, never a spent clock.                                    *)
DemotionRequiresAnAnswer ==
    (registration = "detached") => (refusals > 0 \/ accepted > 0)

(* A present replica always outranks disk. *)
EditorReplicaOutranksDisk ==
    (authority = "disk") => (replica = "missing")

(*************************************************************************)
(* LIVENESS - the property the whole class of wedges violates.            *)
(*                                                                       *)
(* Resolution must follow WITHOUT any operator action, once the endpoint  *)
(* has answered at all. Conditional on having answered, and that is not a *)
(* weakening: an endpoint that stays silent forever legitimately never    *)
(* resolves, because silence is not proof and this design will not demote *)
(* on it. Stating it unconditionally would demand exactly the             *)
(* `--force-disk` the invariants above forbid.                            *)
(*                                                                       *)
(* Both counters are reachable only through an action the endpoint took,  *)
(* so the antecedent cannot be satisfied by waiting.                      *)
(*************************************************************************)
EndpointHasAnswered == refusals > 0 \/ accepted > 0

AnsweredEndpointAlwaysEventuallyResolves ==
    EndpointHasAnswered ~> (authority # "none")

=============================================================================
