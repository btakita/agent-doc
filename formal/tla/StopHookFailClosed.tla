-------------------------- MODULE StopHookFailClosed --------------------------
EXTENDS Naturals, TLC

(***************************************************************************
The Claude Stop hook's OTHER way of refusing a final answer.

WHY THIS IS A SEPARATE MODULE
-----------------------------
`StopHookContinuation` models the decision the hook is for: the queue owes
another item, so do not answer yet. Its bound is the run-keyed request ledger.

This module covers the path taken when that decision cannot be reached at all --
the hook errored -- and it is deliberately a different scope, because the two
bounds are different facts. The continuation bound is durable and cross-turn
(a ledger row). This one is within a single stop sequence, and its only carrier
is `stop_hook_active`, the flag Claude Code sets when a Stop hook is re-entered
after its own block.

Modelling them together would let one bound stand in for the other, which is the
error this whole series keeps finding: a guard that looks armed because some
neighbouring state happens to be present.

WHAT WAS WRONG
--------------
`handle_claude_stop` failed closed on any error:

    Err(err) => block("agent-doc Claude Stop hook failed closed ... report this
                       hook failure to the operator")

Failing closed is right the first time -- the continuation check did not run and
the operator needs to hear it. But this arm never consulted `stop_hook_active`,
and it was the only path through the hook that did not. Hook errors are
overwhelmingly PERSISTENT: an unreadable document, an authority resolve that
refuses while an editor replica is stranded, a state ledger that will not open.
Re-blocking re-runs the same failing check against the same inputs and gets the
same error. `HookRecovers` is therefore deliberately NOT fair here -- a fix that
only works when the fault clears is not a bound.

A third shape has no bound available at all: a payload that is not JSON carries
no `stop_hook_active` to be bounded by. It also names no session and no
document, so there is no continuation to protect -- a refusal issued on no
evidence about a document is not failing closed, it is just a loop. That branch
allows and reports.

THE PROPERTY
------------
`AtMostOneErrorBlockPerStop`. The operator must hear about a broken hook once
per stop; hearing it forever is the same outcome as the hook never answering.
***************************************************************************)

CONSTANTS
    BoundErrorPath  (* the fix under test *)

VARIABLES
    healthy,      (* whether the continuation check can run at all *)
    parseable,    (* whether the payload is JSON *)
    stopActive,   (* Claude's `stop_hook_active`: this stop already blocked *)
    errorBlocks,  (* error-arm refusals issued in the current stop sequence *)
    decision,     (* "none" | "block" | "allow" *)
    decidedOn     (* parseability AS JUDGED when `decision` was reached *)

vars == << healthy, parseable, stopActive, errorBlocks, decision, decidedOn >>

Init ==
    /\ healthy = TRUE
    /\ parseable = TRUE
    /\ stopActive = FALSE
    /\ errorBlocks = 0
    /\ decision = "none"
    /\ decidedOn = TRUE

(* Capped at 2: reaching 2 has already violated the property. *)
TypeOK ==
    /\ healthy \in BOOLEAN
    /\ parseable \in BOOLEAN
    /\ stopActive \in BOOLEAN
    /\ errorBlocks \in 0..2
    /\ decision \in {"none", "block", "allow"}
    /\ decidedOn \in BOOLEAN

(*************************************************************************)
(* ENVIRONMENT. `HookRecovers` exists so the model does not assume the    *)
(* fault is permanent, but nothing makes it happen: the bound must hold   *)
(* against a fault that never clears, which is the observed case.         *)
(*************************************************************************)
HookBreaks ==
    /\ healthy
    /\ healthy' = FALSE
    /\ UNCHANGED << parseable, stopActive, errorBlocks, decision, decidedOn >>

HookRecovers ==
    /\ ~healthy
    /\ healthy' = TRUE
    /\ UNCHANGED << parseable, stopActive, errorBlocks, decision, decidedOn >>

PayloadBecomesUnparseable ==
    /\ parseable
    /\ parseable' = FALSE
    /\ UNCHANGED << healthy, stopActive, errorBlocks, decision, decidedOn >>

(*************************************************************************)
(* THE HOOK                                                               *)
(*************************************************************************)

(* An unparseable payload names no document. Nothing to protect, nothing to
   bound a refusal with, so it does not refuse. *)
StopUnparseable ==
    /\ ~parseable
    /\ decision' = "allow"
    /\ stopActive' = FALSE
    /\ errorBlocks' = 0
    /\ decidedOn' = parseable
    /\ UNCHANGED << healthy, parseable >>

(* The check errored. Fail closed once; a repeat within the same stop cannot
   produce a different error. *)
StopErrored ==
    /\ parseable
    /\ ~healthy
    /\ IF BoundErrorPath /\ stopActive
         THEN /\ decision' = "allow"
              /\ stopActive' = FALSE
              /\ errorBlocks' = 0
         ELSE /\ decision' = "block"
              /\ stopActive' = TRUE
              /\ errorBlocks' = IF errorBlocks < 2 THEN errorBlocks + 1 ELSE 2
    /\ decidedOn' = parseable
    /\ UNCHANGED << healthy, parseable >>

(* The check ran. Whatever it decided, it is not an error-arm refusal, and it
   ends any error sequence. *)
StopHealthy ==
    /\ parseable
    /\ healthy
    /\ decision' = "allow"
    /\ stopActive' = FALSE
    /\ errorBlocks' = 0
    /\ decidedOn' = parseable
    /\ UNCHANGED << healthy, parseable >>

Next ==
    \/ HookBreaks
    \/ HookRecovers
    \/ PayloadBecomesUnparseable
    \/ StopUnparseable
    \/ StopErrored
    \/ StopHealthy

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(StopUnparseable)
    /\ WF_vars(StopErrored)
    /\ WF_vars(StopHealthy)

(*************************************************************************)
(* SAFETY                                                                 *)
(*************************************************************************)

(* THE property. Report a broken hook once per stop; reporting it forever is
   indistinguishable from the hook never answering. *)
AtMostOneErrorBlockPerStop == errorBlocks <= 1

(* A refusal must be about a document. With no parseable payload there is no
   session, no document, and no continuation, so there is nothing to refuse on
   behalf of -- and no `stop_hook_active` to bound the refusal with either. *)
NeverRefusesWithoutADocument == (decision = "block") => decidedOn

(*************************************************************************)
(* NON-VACUITY. A hook that never refuses satisfies both properties, and  *)
(* silently never telling the operator about a broken hook is its own     *)
(* failure. `...Reach.cfg` requires this to be VIOLATED under the fix.    *)
(*************************************************************************)
NeverReportsABrokenHook == errorBlocks = 0

=============================================================================
