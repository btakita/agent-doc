------------------------- MODULE TransientRefusalLatch -------------------------
EXTENDS Naturals, TLC

(***************************************************************************
The refusal-classification ladder: what a caller must do with a refusal it did
not author.

WHY THIS MODULE EXISTS
----------------------
`EditorReplicaStrand` models ONE wedge of this shape - an attachment latch that
outlives the endpoint's own refusal to serve, leaving a state with no outgoing
edge. It is about what the RESPONDER latched. Four more instances landed within
a single session on 2026-09-27, and they are not that bug: they are about what
the CALLER does with a refusal, and each one ended with an agent correctly
stopping on a refusal whose cause had already cleared or was about to.

  1. `route_queue_activation` on `monsterrodholders.md`. The resolver refused
     with `... missing_replica recovery exhausted and disk read authority is
     refused`. The caller's retry predicate matched only the `sync_pending`
     spelling of that one message, so the entire missing-replica family read as
     an authored verdict and was never re-asked. JetBrains `Run Agent Doc`
     failed outright.

  2. `agent-doc tasks/software/lazily.md`. The shared project controller
     panicked mid-request - a byte-index slice landing inside `❯`, agent-doc's
     own prompt marker - so the client saw `project controller closed connection
     without a response`. Preflight admission surfaced that transport fact as a
     refusal WITH A REASON, and the agent, correctly trusting a reasoned
     refusal, stopped. A whole turn was lost to a crash nobody classified.

  3. The same `route_queue_activation` refusal after the predicate was widened.
     Re-asking immediately still failed: every missing-replica recovery latch is
     keyed on the editor replica LIVENESS WITNESS, and at an unchanged witness
     the second attempt skips the re-registration loop AND the terminal rebuild
     and bails on the identical refusal. The budget is bounded, so a blind retry
     spends it on a provable no-op and then fails closed for good.

  4. `monsterrodholders.md` and `devops.md` session-check INTERRUPTED: "the
     controller owns the generation-fenced editor save", do not resubmit, patch,
     or force disk. True while something durable holds the write - but the
     holder named was the controller that had just crashed, and a dead holder
     fires no state edge. Two sessions obeyed and stranded their writes.

THE CLASS
---------
A caller-visible refusal must be classified, and each class has exactly one
correct caller response:

  * AUTHORED - the responder decided. Re-asking is forbidden; retrying a verdict
    turns a fail-closed guard into a retry storm.

  * TRANSIENT - the cause can clear on its own. The refusal must name a WITNESS
    that clearing advances, the caller must WAIT for that witness rather than
    failing closed, and it must spend an attempt only once the witness moved.

  * CRASH - the responder died before answering, so no verdict exists. A
    replacement serves again, which makes the replacement the crash's witness.

A transient refusal whose caller fails closed while the witness could still
advance, or which spends a bounded budget against unchanged state, is a LATCH: a
reachable state with no path back to progress.

THE THREE KNOBS
---------------
  * `ClassifyTransientRefusal` - FALSE is instance 1: a predicate that matched
    one family's spelling and read every other transient refusal as a verdict.

  * `WaitForWitness` - FALSE is instance 3: the caller neither waits for the
    witness nor gates its attempts on it, so it burns the bounded budget against
    unchanged state and then fails closed. This is the knob that makes widening
    the predicate alone INSUFFICIENT, which is why the production fix blocks on
    `wait_for_editor_replica_liveness_change` instead of just re-asking.

  * `ClassifyCrashAsTransient` - FALSE is instances 2 and 4: a responder that
    died before answering, reported as though it had ruled.

Each FALSE must VIOLATE `StableResponderIsEventuallyServed`, and each has its own
must-violate config in `scripts/run_tla.sh`. A property that cannot fail is not
evidence - the lesson `EditorReplicaStrand` encodes - applied per knob so no one
fix can quietly make the others vacuous.

WHY THE PROPERTY IS CONDITIONAL
-------------------------------
An adversary that degrades the responder forever defeats any caller, and a model
that pretended otherwise would be proving something false. So the obligation is
stated where it belongs: once the responder is permanently serving, the caller
must get through. That makes the REACH obligation load-bearing rather than
ceremonial, which is why `TransientRefusalLatchReach.cfg` exists.

WHAT IS DELIBERATELY ABSENT
---------------------------
No action lets the caller progress without the responder actually serving. These
refusals are CORRECT as safety; widening them into "proceed anyway" is
`--force-disk`, which loses the operator's unsaved work. Progress must come from
a witness advancing, never from a caller deciding the refusal did not apply.

Also absent: every operator action. Reopening a tab, hand-recycling, re-typing a
trigger - those are the remedies this class must stop requiring.
***************************************************************************)

CONSTANTS
    MaxRetryAttempts,           (* bounded caller budget per invocation *)
    MaxFaults,                  (* bound the adversary so the model is finite *)
    ClassifyTransientRefusal,   (* fix under test: recognise a transient refusal *)
    WaitForWitness,             (* fix under test: hold for a real witness change *)
    ClassifyCrashAsTransient    (* fix under test: a crash is not a verdict *)

VARIABLES
    responder,  (* "serving" | "degraded" | "dead" *)
    caller,     (* "asking" | "refused" | "progressed" | "gaveup" *)
    kind,       (* "none" | "transient" | "authored" | "crash" *)
    rearmed,    (* TRUE iff a registration landed since this refusal was recorded *)
    attempts,   (* attempts spent in the current invocation *)
    faults      (* environment faults spent *)

vars == << responder, caller, kind, rearmed, attempts, faults >>

Init ==
    /\ responder = "serving"
    /\ caller = "asking"
    /\ kind = "none"
    /\ rearmed = FALSE
    /\ attempts = 0
    /\ faults = 0

TypeOK ==
    /\ responder \in {"serving", "degraded", "dead"}
    /\ caller \in {"asking", "refused", "progressed", "gaveup"}
    /\ kind \in {"none", "transient", "authored", "crash"}
    /\ rearmed \in BOOLEAN
    /\ attempts \in 0..MaxRetryAttempts
    /\ faults \in 0..MaxFaults

(*************************************************************************)
(* ENVIRONMENT - not fair, and bounded. TLC explores every interleaving,  *)
(* so a passing run never rests on the environment being kind; the bound   *)
(* only stops an infinite adversary from making the obligation vacuous.    *)
(*************************************************************************)

(* The replica is dropped: a cdylib generation swap, a recycle, a lost model. *)
Degrade ==
    /\ responder = "serving"
    /\ faults < MaxFaults
    /\ responder' = "degraded"
    /\ faults' = faults + 1
    /\ UNCHANGED << caller, kind, rearmed, attempts >>

(* The responder dies mid-request - a panic. It never answers at all. *)
Crash ==
    /\ responder \in {"serving", "degraded"}
    /\ faults < MaxFaults
    /\ responder' = "dead"
    /\ faults' = faults + 1
    /\ UNCHANGED << caller, kind, rearmed, attempts >>

(*************************************************************************)
(* RECOVERY - fair. This is the machinery the design owes the caller, and  *)
(* the only thing that makes a recorded refusal re-armable.                *)
(*************************************************************************)

(* A registration lands: the replica is rebuilt and the witness advances, which *)
(* re-arms every recovery latch keyed on it.                                    *)
Reregister ==
    /\ responder = "degraded"
    /\ responder' = "serving"
    /\ rearmed' = TRUE
    /\ UNCHANGED << caller, kind, attempts, faults >>

(* A dead responder is replaced. The replacement is a different registration, *)
(* so it re-arms too - a replacement is exactly the crash's witness.          *)
Replace ==
    /\ responder = "dead"
    /\ responder' = "serving"
    /\ rearmed' = TRUE
    /\ UNCHANGED << caller, kind, attempts, faults >>

(*************************************************************************)
(* THE CALLER                                                            *)
(*************************************************************************)

Served ==
    /\ caller = "asking"
    /\ responder = "serving"
    /\ caller' = "progressed"
    /\ kind' = "none"
    /\ UNCHANGED << responder, rearmed, attempts, faults >>

(* A degraded responder refuses, and the refusal is transient by construction:  *)
(* a re-registration clears it. Recording it resets `rearmed` - the refusal is  *)
(* observed against the CURRENT witness, so nothing has moved since.            *)
(*                                                                             *)
(* `attempts` is deliberately UNCHANGED: the budget belongs to the invocation,  *)
(* not to one refusal. Resetting it here would model an outer loop that does    *)
(* not exist - `route` fails its whole dispatch when the budget is spent.       *)
RefusedTransient ==
    /\ caller = "asking"
    /\ responder = "degraded"
    /\ caller' = "refused"
    /\ kind' = "transient"
    /\ rearmed' = FALSE
    /\ UNCHANGED << responder, attempts, faults >>

(* The responder died before answering: no verdict was ever authored. *)
RefusedByCrash ==
    /\ caller = "asking"
    /\ responder = "dead"
    /\ caller' = "refused"
    /\ kind' = "crash"
    /\ rearmed' = FALSE
    /\ UNCHANGED << responder, attempts, faults >>

(* A serving responder that RULES against the request. This is the only refusal *)
(* class whose correct handling is to stop, and it is modelled so the           *)
(* obligation below ("never re-ask a verdict") constrains a reachable action    *)
(* rather than an absent one.                                                   *)
RefusedAuthored ==
    /\ caller = "asking"
    /\ responder = "serving"
    /\ caller' = "refused"
    /\ kind' = "authored"
    /\ rearmed' = FALSE
    /\ UNCHANGED << responder, attempts, faults >>

(* Is this a refusal the caller is willing to re-ask at all? *)
Retryable ==
    \/ (kind = "transient" /\ ClassifyTransientRefusal)
    \/ (kind = "crash" /\ ClassifyCrashAsTransient)

(* May an attempt be spent now? With the witness rule on, only once a           *)
(* registration has actually landed - precisely when the latches keyed on it    *)
(* have re-armed. Without it the caller re-asks into unchanged state.           *)
MayRetryNow ==
    /\ Retryable
    /\ attempts < MaxRetryAttempts
    /\ (WaitForWitness => rearmed)

(* The edge the class turns on. A caller holding a retryable refusal whose       *)
(* witness has not moved yet must WAIT, not fail closed - waiting is what makes  *)
(* the recovery machinery's fairness reachable at all. Failing closed here is    *)
(* the latch, and it is what every one of the four instances actually did.       *)
MayWait ==
    /\ WaitForWitness
    /\ Retryable
    /\ attempts < MaxRetryAttempts
    /\ ~rearmed

Retry ==
    /\ caller = "refused"
    /\ MayRetryNow
    /\ caller' = "asking"
    /\ kind' = "none"
    /\ attempts' = attempts + 1
    /\ UNCHANGED << responder, rearmed, faults >>

(* Fail closed - correct for an authored verdict, and the latch for anything *)
(* else. `gaveup` has no outgoing edge on purpose: it models the work being  *)
(* handed back to the operator, which is the outcome under test.            *)
GiveUp ==
    /\ caller = "refused"
    /\ ~MayRetryNow
    /\ ~MayWait
    /\ caller' = "gaveup"
    /\ UNCHANGED << responder, kind, rearmed, attempts, faults >>

(* A document gets many turns; each starts with a fresh budget. Without this *)
(* the happy path is a dead end and the model reports a deadlock instead of  *)
(* checking anything.                                                       *)
NewTurn ==
    /\ caller = "progressed"
    /\ caller' = "asking"
    /\ attempts' = 0
    /\ rearmed' = FALSE
    /\ UNCHANGED << responder, kind, faults >>

Next ==
    \/ Degrade
    \/ Crash
    \/ Reregister
    \/ Replace
    \/ Served
    \/ RefusedTransient
    \/ RefusedByCrash
    \/ RefusedAuthored
    \/ Retry
    \/ GiveUp
    \/ NewTurn

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Reregister)
    /\ WF_vars(Replace)
    /\ WF_vars(Served)
    /\ WF_vars(RefusedTransient)
    /\ WF_vars(RefusedByCrash)
    /\ WF_vars(Retry)
    /\ WF_vars(GiveUp)
    /\ WF_vars(NewTurn)

(*************************************************************************)
(* SAFETY                                                               *)
(*************************************************************************)

(* The caller never fabricates progress. An ACTION property, not an invariant:  *)
(* `progressed` is a latched state, so the responder may legitimately degrade    *)
(* afterwards - what must never happen is ENTERING it without a service. Stating *)
(* it as an invariant made it fail on every trace where a later fault arrived,   *)
(* which is not the property anyone wants.                                       *)
(*                                                                              *)
(* This is what stops the module from being a proof that `--force-disk` is fine. *)
ProgressOnlyWhenServed ==
    [][(caller' = "progressed" /\ caller # "progressed") => (responder = "serving")]_vars

(* An authored verdict is never re-asked, so the fix for this class cannot      *)
(* collapse into "retry everything". `RefusedAuthored` makes this reachable, so *)
(* the obligation constrains a real edge.                                       *)
NeverRetriesAnAuthoredVerdict ==
    [][(caller = "refused" /\ caller' = "asking") => (kind # "authored")]_vars

(* The sharp safety statement of the whole class: work is handed back to the    *)
(* operator ONLY because the responder ruled. A `gaveup` carrying a transient or *)
(* crash refusal is a latch, by definition.                                     *)
(*                                                                              *)
(* This replaces TLC's deadlock check, which cannot serve here: `gaveup` is      *)
(* genuinely terminal for an authored verdict - stopping is the correct outcome  *)
(* - so a reachable dead end is expected and deadlock-checking would fail the    *)
(* positive run for the one behaviour that is right. Stating the property        *)
(* directly says what deadlock-checking was standing in for, and says it better: *)
(* it names WHICH dead ends are legitimate instead of forbidding all of them.    *)
OnlyAuthoredVerdictsStop ==
    (caller = "gaveup") => (kind = "authored")

(*************************************************************************)
(* LIVENESS - the property the whole class violates.                      *)
(*                                                                       *)
(* Once the responder is permanently serving, the caller must get through, *)
(* with no operator action. Conditional because an adversary that degrades  *)
(* forever defeats any caller; see "WHY THE PROPERTY IS CONDITIONAL".      *)
(*************************************************************************)
(* The authored-verdict exclusion is not a loophole: stopping on a verdict is   *)
(* the CORRECT outcome, so a run in which one is issued owes no progress.       *)
StableResponderIsEventuallyServed ==
    ([](kind # "authored") /\ <>[](responder = "serving"))
        => []<>(caller = "progressed")

(*************************************************************************)
(* REACH obligation. Asserting progress never happens must itself be       *)
(* violated, or the conditional property above could pass because its      *)
(* consequent is unreachable rather than because the design delivers it.    *)
(*************************************************************************)
ProgressIsUnreachable ==
    [](caller # "progressed")

=============================================================================
