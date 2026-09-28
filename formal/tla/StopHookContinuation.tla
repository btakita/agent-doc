------------------------ MODULE StopHookContinuation ------------------------
EXTENDS Naturals, TLC

(***************************************************************************
The Claude Stop hook's continuation request, and what bounds it.

WHY THIS MODULE EXISTS
----------------------
The hook may refuse the agent's final answer and redirect it back through the
auto-queue loop. That refusal has to be bounded: `stop_hook_active` bounds
recursion WITHIN one stop, and `#stopneedsclosedcycle` added a cross-turn bound
by remembering the head last requested. The comparison was right. Its STORAGE
was not, and a bound that is not always armed is not a bound.

The remembered head lived on `ContinuationMarker`, a record owned by queue
reconciliation. Two consequences, both of which happened:

  * The hook can reach its block with NO marker. `continuation_proven` is
    satisfied by the marker OR by the drain-stall projection, and on the
    projection branch the arming write documented itself as a no-op. The guard
    then had nothing to read and nothing to write: absent, not weak.
  * A reconcile between two stops deletes the marker, and the bound with it,
    disarming a guard that had been armed.

Measured on `tasks/agent-doc/agent-doc-bugs.md` 2026-09-28 -- 24 blocks against
2 skips, every repeat inside a single run:

  00:03:27 head=62 turn=cycle-1790552355729  block
  00:05:07 head=62 turn=cycle-1790552355729  repeat
  00:30:47 head=26 turn=cycle-1790552355729  block   <- head moved, no run ran
  00:31:19 head=26 turn=cycle-1790552355729  repeat
  01:12:56 head=22 turn=cycle-1790556757989  block
  01:24:54 head=22 turn=cycle-1790556757989  block   <- nothing to read
  01:33:18 head=22 turn=cycle-1790556757989  block
  01:34:02 head=22 turn=cycle-1790556757989  skip    <- only once a marker existed

RECONCILE AGAINST THE RUN, NOT THE TEXT
---------------------------------------
The 00:30:47 line is the one head-equality can never catch: the head advanced
62 -> 26 bytes while `turn` stayed on `cycle-1790552355729`. A head can change
because the operator edited the document, because a reconcile rewrote the queue,
or because a malformed head was reparsed. None of those is drain progress. Only
a completed run is, so the request is keyed to the run that asked for it.

`HeadChangesWithoutRun` is in this model for exactly that trace, and it is what
makes the two guard modes distinguishable. Head equality is KEPT as a second
clause -- it catches the run that completed and still struck nothing (`#qchurn`,
the case `#stopneedsclosedcycle` was written for) -- but it is no longer the
only clause, and it is no longer the only stored fact.

THE PROPERTY
------------
`AtMostOneRequestPerRun`. A continuation request asks that THIS run be followed
by another. Asking twice asks a run that has already ended to change what it
drained, which it cannot. So the bound is not "eventually stop" -- it is exact:
one request per run, or the hook is asking for something impossible.

Deliberately ABSENT: any action that ends the loop from outside. The supervisor
stall projection and the operator both exist, and modelling either would let the
model satisfy boundedness with something that is not the guard.
***************************************************************************)

CONSTANTS
    MaxRun,           (* bounded run counter *)
    MaxHead,          (* bounded head counter *)
    GuardKeyedToRun   (* the fix under test *)

VARIABLES
    runId,           (* id of the last COMPLETED run; 0 = nothing has run *)
    head,            (* the current queue head *)
    markerPresent,   (* whether queue reconciliation's marker row exists *)
    recordedRun,     (* run of the remembered request; 0 = nothing remembered *)
    recordedHead,    (* head of the remembered request; 0 = nothing remembered *)
    remembered,      (* whether anything is remembered at all *)
    requestsThisRun, (* continuation requests issued since runId last changed *)
    decision         (* "none" | "block" | "allow" *)

vars == << runId, head, markerPresent, recordedRun, recordedHead, remembered,
           requestsThisRun, decision >>

Init ==
    /\ runId = 0
    /\ head = 1
    /\ markerPresent = FALSE
    /\ recordedRun = 0
    /\ recordedHead = 0
    /\ remembered = FALSE
    /\ requestsThisRun = 0
    /\ decision = "none"

(* Capped at 2: reaching 2 has already violated the property, so the counter    *)
(* never needs to climb higher and the state space stays finite.                *)
TypeOK ==
    /\ runId \in 0..MaxRun
    /\ head \in 0..MaxHead
    /\ markerPresent \in BOOLEAN
    /\ recordedRun \in 0..MaxRun
    /\ recordedHead \in 0..MaxHead
    /\ remembered \in BOOLEAN
    /\ requestsThisRun \in 0..2
    /\ decision \in {"none", "block", "allow"}

(*************************************************************************)
(* ENVIRONMENT                                                            *)
(*************************************************************************)

(* A run completed and drained its head: ordinary progress. *)
RunDrains ==
    /\ runId < MaxRun
    /\ head < MaxHead
    /\ runId' = runId + 1
    /\ head' = head + 1
    /\ requestsThisRun' = 0
    /\ UNCHANGED << markerPresent, recordedRun, recordedHead, remembered, decision >>

(* A run completed and the head survived it: `#qchurn`. A malformed head whose
   id-keyed strike matched nothing, or an admission that never opened a cycle. *)
RunWithoutDraining ==
    /\ runId < MaxRun
    /\ runId' = runId + 1
    /\ requestsThisRun' = 0
    /\ UNCHANGED << head, markerPresent, recordedRun, recordedHead, remembered, decision >>

(* The head moves with NO run: an operator edit, a queue rewrite, a reparse.
   Observed 00:30:47 as 62 -> 26 bytes inside one `turn`. Not drain progress,
   and invisible to a guard that compares heads. *)
HeadChangesWithoutRun ==
    /\ head < MaxHead
    /\ head' = head + 1
    /\ UNCHANGED << runId, markerPresent, recordedRun, recordedHead, remembered,
                    requestsThisRun, decision >>

(* Queue reconciliation owns the marker row and may write or clear it at any
   time. Under the shipped guard, clearing it also destroys the bound. *)
WriteMarker ==
    /\ ~markerPresent
    /\ markerPresent' = TRUE
    /\ UNCHANGED << runId, head, recordedRun, recordedHead, remembered,
                    requestsThisRun, decision >>

ClearMarker ==
    /\ markerPresent
    /\ markerPresent' = FALSE
    /\ IF GuardKeyedToRun
         THEN UNCHANGED << recordedRun, recordedHead, remembered >>
         ELSE /\ remembered' = FALSE
              /\ recordedRun' = 0
              /\ recordedHead' = 0
    /\ UNCHANGED << runId, head, requestsThisRun, decision >>

(*************************************************************************)
(* THE HOOK                                                               *)
(*                                                                        *)
(* `Remembers` is where the two modes differ: the shipped bound can only  *)
(* be armed while the marker exists, because it is a field on it.         *)
(*************************************************************************)
CanArm == GuardKeyedToRun \/ markerPresent

GuardFires ==
    /\ remembered
    /\ IF GuardKeyedToRun
         THEN (recordedRun = runId) \/ (recordedHead = head)
         ELSE recordedHead = head

Stop ==
    /\ head > 0
    /\ IF GuardFires
         THEN /\ decision' = "allow"
              /\ UNCHANGED << recordedRun, recordedHead, remembered, requestsThisRun >>
         ELSE /\ decision' = "block"
              /\ requestsThisRun' = IF requestsThisRun < 2
                                      THEN requestsThisRun + 1
                                      ELSE 2
              /\ IF CanArm
                   THEN /\ remembered' = TRUE
                        /\ recordedRun' = runId
                        /\ recordedHead' = head
                   ELSE UNCHANGED << recordedRun, recordedHead, remembered >>
    /\ UNCHANGED << runId, head, markerPresent >>

Next ==
    \/ RunDrains
    \/ RunWithoutDraining
    \/ HeadChangesWithoutRun
    \/ WriteMarker
    \/ ClearMarker
    \/ Stop

Spec == Init /\ [][Next]_vars /\ WF_vars(Stop)

(*************************************************************************)
(* SAFETY                                                                 *)
(*************************************************************************)

(* THE property. A continuation request asks that the run which just closed be
   followed by another. That run has already ended, so a second request against
   it asks for an outcome no longer in reach -- and the loop, having taken the
   first request and produced nothing, will produce nothing again. *)
AtMostOneRequestPerRun == requestsThisRun <= 1

(* The bound must not depend on a record another subsystem owns. Whenever the
   hook blocks, something must have been remembered -- this is the "absent, not
   weak" failure stated directly, and it is what `CanArm` decides. *)
EveryRequestIsRemembered == (decision = "block") => remembered

(*************************************************************************)
(* NON-VACUITY                                                            *)
(*                                                                        *)
(* Both properties above are satisfied by a hook that never blocks, which  *)
(* is a real risk here: the fix ADDS a reason to allow, so "it stopped     *)
(* looping" and "it stopped working" look identical from the outside.      *)
(* `...Reach.cfg` requires each of these to be VIOLATED under the fix.     *)
(*************************************************************************)

(* The hook still refuses a final answer when the queue genuinely owes work. *)
NeverBlocks == decision # "block"

(* And it can do so more than once, so one non-advancing head does not latch
   continuation off for the life of the document. *)
NeverBlocksTwice == runId < 2 \/ recordedRun < 2

=============================================================================
