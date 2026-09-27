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

Note what is deliberately ABSENT: any action that restores `endpointServes`.
Reopening the editor tab is the operator action the design must not require, so
modelling it would reintroduce the same assumption that made the existing model
vacuous. Progress here must come from agent-doc alone.
***************************************************************************)

CONSTANTS
    MaxReregisterAttempts,       (* bounded re-registration budget, 3 in production *)
    DemoteOnDefinitiveRejection  (* the fix under test *)

VARIABLES
    replica,        (* "present" | "missing" - agent-doc's Lazily replica *)
    endpointServes, (* TRUE iff the live editor endpoint will serve this document *)
    registration,   (* "attached" | "detached" - the attachment latch *)
    attempts,       (* re-registration attempts spent against a definitive refusal *)
    authority       (* "none" | "editor" | "disk" - what a resolve produced *)

vars == << replica, endpointServes, registration, attempts, authority >>

Init ==
    /\ replica = "present"
    /\ endpointServes = TRUE
    /\ registration = "attached"
    /\ attempts = 0
    /\ authority = "editor"

TypeOK ==
    /\ replica \in {"present", "missing"}
    /\ endpointServes \in BOOLEAN
    /\ registration \in {"attached", "detached"}
    /\ attempts \in 0..MaxReregisterAttempts
    /\ authority \in {"none", "editor", "disk"}

(*************************************************************************)
(* A cdylib generation swap drops the replica and leaves the OLD endpoint *)
(* unable to serve this document. The attachment latch is untouched -     *)
(* that divergence between latch and endpoint IS the bug.                *)
(*************************************************************************)
LibraryReload ==
    /\ replica = "present"
    /\ replica' = "missing"
    /\ endpointServes' = FALSE
    /\ authority' = "none"
    /\ UNCHANGED << registration, attempts >>

(* The endpoint answers and accepts: the replica is rebuilt. *)
ReregisterAccepted ==
    /\ replica = "missing"
    /\ endpointServes
    /\ replica' = "present"
    /\ attempts' = 0
    /\ UNCHANGED << endpointServes, registration, authority >>

(* The endpoint answers and REJECTS - a receipt, not a timeout. Bounded budget. *)
ReregisterRejected ==
    /\ replica = "missing"
    /\ ~endpointServes
    /\ attempts < MaxReregisterAttempts
    /\ attempts' = attempts + 1
    /\ UNCHANGED << replica, endpointServes, registration, authority >>

ResolveOnEditor ==
    /\ replica = "present"
    /\ authority' = "editor"
    /\ UNCHANGED << replica, endpointServes, registration, attempts >>

(* Legal only once the editor is PROVEN not serving. *)
DescendToDisk ==
    /\ replica = "missing"
    /\ registration = "detached"
    /\ authority' = "disk"
    /\ UNCHANGED << replica, endpointServes, registration, attempts >>

(* The missing edge: a definitive rejection demotes the stale latch. *)
DemoteStaleRegistration ==
    /\ DemoteOnDefinitiveRejection
    /\ registration = "attached"
    /\ replica = "missing"
    /\ ~endpointServes
    /\ attempts = MaxReregisterAttempts
    /\ registration' = "detached"
    /\ UNCHANGED << replica, endpointServes, attempts, authority >>

Next ==
    \/ LibraryReload
    \/ ReregisterAccepted
    \/ ReregisterRejected
    \/ ResolveOnEditor
    \/ DescendToDisk
    \/ DemoteStaleRegistration

(*************************************************************************)
(* Every recovery action is weakly fair. `LibraryReload` is NOT fair - it *)
(* is an environment event that may or may not happen; TLC explores both. *)
(*************************************************************************)
Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ReregisterAccepted)
    /\ WF_vars(ReregisterRejected)
    /\ WF_vars(ResolveOnEditor)
    /\ WF_vars(DescendToDisk)
    /\ WF_vars(DemoteStaleRegistration)

(*************************************************************************)
(* SAFETY                                                                *)
(*************************************************************************)

(* The invariant the disk refusal exists to protect, stated in terms of what  *)
(* is actually being protected (a serving endpoint) rather than the latch.    *)
NeverReadsDiskWhileEndpointServes ==
    (authority = "disk") => ~endpointServes

(* Demotion is only ever justified by a definitive refusal. This is what stops *)
(* the fix from becoming a disguised `--force-disk`.                           *)
DemotionRequiresDefinitiveRefusal ==
    (registration = "detached") => ~endpointServes

(* A present replica always outranks disk. *)
EditorReplicaOutranksDisk ==
    (authority = "disk") => (replica = "missing")

(*************************************************************************)
(* LIVENESS - the property the whole class of wedges violates.            *)
(*                                                                       *)
(* Resolution must remain reachable forever, WITHOUT any operator action. *)
(* With `DemoteOnDefinitiveRejection = FALSE` this is violated by the     *)
(* exact production trace above, which is the point.                     *)
(*************************************************************************)
AuthorityAlwaysEventuallyResolves ==
    []<>(authority # "none")

=============================================================================
