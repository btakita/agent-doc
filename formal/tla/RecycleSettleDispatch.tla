---------------------------- MODULE RecycleSettleDispatch ----------------------------
(***************************************************************************)
(* `#recyclesettlewaitshort` — a dispatch arriving while the project         *)
(* supervisor is mid-upgrade.                                               *)
(*                                                                          *)
(* An `auto_install_reexec` recycle publishes `InFlight` before it `execve`s *)
(* onto the freshly installed binary, and the replacement's watch loop       *)
(* clears it. A dispatch-only reopen must not inject across that hot-reload  *)
(* boundary — a trigger typed there is dropped before submit — so it waits.  *)
(*                                                                          *)
(* TWO budgets govern that wait, and they were incoherent:                   *)
(*                                                                          *)
(*   * the controller's blocking `supervisor_recycle_wait_settled` RPC gives *)
(*     up after `SUPERVISOR_RECYCLE_SETTLE_WAIT` (10s) — `WaitBudget` here;  *)
(*   * abandonment is only declared at `RECYCLE_INFLIGHT_SETTLE_TTL_SECS`    *)
(*     (120s) — `Ttl` here.                                                  *)
(*                                                                          *)
(* The shipped gate read ONE RPC timeout as a verdict (`SingleShotVerdict`), *)
(* so every recycle whose age fell strictly between the two budgets was      *)
(* refused while it was still, by the system's own TTL, merely pending. The  *)
(* refusal named `unblocker=wait_for_supervisor_recycle_settle` — the very   *)
(* thing the binary had just stopped doing — so the remedy and the action    *)
(* contradicted each other and the operator's only move was to re-run the    *)
(* command that had just refused.                                            *)
(*                                                                          *)
(* Observed live 2026-09-28 on `src/haiven-dev/tasks/infra.md`: the wait     *)
(* opened at 17:00:48, gave up at 17:00:58 (`waited_ms=10050`,               *)
(* `recycle_epoch=8443`), and the recycle settled and injected normally at   *)
(* 17:01:47 — 59s in, well inside the TTL and long past the wait. The 10s    *)
(* budget was calibrated against a bare `execve` (observed at 3-7s), but the *)
(* reason that actually gates dispatch here spans a whole `make install`     *)
(* BEFORE the `execve`.                                                      *)
(*                                                                          *)
(* Raising the 10s constant only moves the window. The fix is to stop        *)
(* treating one RPC timeout as a verdict and re-arm the wait until the TTL   *)
(* decides, which makes the two budgets coherent by construction rather than *)
(* by a pair of constants that can drift apart again — and that is the       *)
(* property checked here.                                                    *)
(*                                                                          *)
(* `#netadv5` R9 / `#netadv3`: the TTL is NOT a fact. An elapsed TTL used to *)
(* be read as "the settle transition was lost" and dispatch proceeded, even  *)
(* while the recycling supervisor was alive and about to settle, injecting   *)
(* across the very boundary this gate protects. Abandonment now needs        *)
(* positive evidence that the supervisor is gone; past the TTL with the      *)
(* supervisor alive the gate stops waiting with a RETRYABLE refusal          *)
(* (`RefuseOwnerStillRecycling`). The TTL therefore only ever ends a wait in *)
(* a refusal (safe), never in an injection. `TtlIsProof = TRUE` restores the *)
(* old reading and MUST violate `NeverProceedsPastALiveRecycle`              *)
(* (`RecycleSettleDispatchTtlWedge`).                                        *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS
    Horizon,            \* bounded clock, in whatever unit the two budgets share
    Ttl,                \* abandonment threshold (RECYCLE_INFLIGHT_SETTLE_TTL_SECS)
    WaitBudget,         \* one settle-wait RPC (SUPERVISOR_RECYCLE_SETTLE_WAIT)
    SingleShotVerdict,  \* TRUE models the shipped gate: one timeout is a verdict
    Stamped,            \* FALSE models an unstamped projection (marked_secs = 0)
    TtlIsProof          \* TRUE models pre-R9: an elapsed TTL proves the settle was lost

(* The incoherence this module is about only exists when the wait is shorter  *)
(* than the TTL, so the model insists on that ordering: a configuration that   *)
(* "fixed" the bug by making the wait longer than the TTL would be checking    *)
(* a different system.                                                         *)
ASSUME /\ Horizon \in Nat /\ Ttl \in Nat /\ WaitBudget \in Nat
       /\ WaitBudget > 0
       /\ Ttl > WaitBudget
       /\ Horizon > Ttl

VARIABLES
    age,        \* time since the recycle was marked InFlight
    recycle,    \* "inflight" | "settled"
    settleAge,  \* the age at which the replacement reaches its watch loop
    gate,       \* "waiting" | "delivered" | "proceeded" | "refused"
    waitStart,  \* age at which the current settle-wait RPC was armed
    delivered   \* count of injected triggers; exactly-once across the boundary

vars == <<age, recycle, settleAge, gate, waitStart, delivered>>

(* A recycle that never settles: the supervisor died between publishing      *)
(* InFlight and its replacement reaching the watch loop (`#recycleinflightwedge`). *)
Never == Horizon + 1

(* Positive evidence the gate can read: is a supervisor process still running  *)
(* for this document? It is gone exactly when the settle will never come.      *)
SupervisorAlive == settleAge # Never

Terminal == gate \in {"delivered", "proceeded", "refused"}

Init ==
    /\ age = 0
    /\ recycle = "inflight"
    /\ settleAge \in (1..Horizon) \cup {Never}
    /\ gate = "waiting"
    /\ waitStart = 0
    /\ delivered = 0

Tick ==
    /\ ~Terminal
    /\ age < Horizon
    /\ age' = age + 1
    /\ recycle' = IF age + 1 = settleAge THEN "settled" ELSE recycle
    /\ UNCHANGED <<settleAge, gate, waitStart, delivered>>

(* One settle-wait RPC returning. It returns early when the recycle settles,  *)
(* and otherwise when its budget is spent.                                    *)
ObserveEnabled ==
    /\ gate = "waiting"
    /\ \/ recycle = "settled"
       \/ age - waitStart >= WaitBudget

Settled ==
    /\ recycle = "settled"
    /\ gate' = "delivered"
    /\ delivered' = delivered + 1
    /\ UNCHANGED waitStart

(* An unstamped projection is unknown, not stale. An unbounded wait on an     *)
(* unknown mark cannot terminate, so this stays fail-closed — the one         *)
(* surviving refusal.                                                          *)
Unstamped ==
    /\ recycle = "inflight"
    /\ ~Stamped
    /\ gate' = "refused"
    /\ UNCHANGED <<waitStart, delivered>>

(* Past the TTL the settle transition was LOST. Waiting for it is waiting for *)
(* an event that will never arrive, so proceed loudly: the hot-reload boundary *)
(* this gate protects is long over.                                            *)
Abandoned ==
    /\ recycle = "inflight"
    /\ Stamped
    /\ age > Ttl
    /\ TtlIsProof \/ ~SupervisorAlive   \* R9: the supervisor must be GONE
    /\ gate' = "proceeded"
    /\ UNCHANGED <<waitStart, delivered>>

(* Past the TTL with the supervisor still ALIVE: it is still recycling. Stop  *)
(* waiting with a retryable refusal; never inject across a live boundary.     *)
RefuseOwnerStillRecycling ==
    /\ recycle = "inflight"
    /\ Stamped
    /\ age > Ttl
    /\ ~TtlIsProof
    /\ SupervisorAlive
    /\ gate' = "refused"
    /\ UNCHANGED <<waitStart, delivered>>

(* THE DEFECT. One RPC timeout read as a verdict, inside the TTL. *)
SingleShotRefusal ==
    /\ recycle = "inflight"
    /\ Stamped
    /\ age <= Ttl
    /\ SingleShotVerdict
    /\ gate' = "refused"
    /\ UNCHANGED <<waitStart, delivered>>

(* THE FIX. Still inside the TTL, so the recycle is pending, not lost: re-arm *)
(* the same blocking RPC and let the TTL be the only thing that decides.      *)
ReArm ==
    /\ recycle = "inflight"
    /\ Stamped
    /\ age <= Ttl
    /\ ~SingleShotVerdict
    /\ gate' = "waiting"
    /\ waitStart' = age
    /\ UNCHANGED delivered

Observe ==
    /\ ObserveEnabled
    /\ \/ Settled
       \/ Unstamped
       \/ Abandoned
       \/ RefuseOwnerStillRecycling
       \/ SingleShotRefusal
       \/ ReArm
    /\ UNCHANGED <<age, recycle, settleAge>>

Done ==
    /\ Terminal
    /\ UNCHANGED vars

Next == Tick \/ Observe \/ Done

Spec == Init /\ [][Next]_vars /\ WF_vars(Tick) /\ WF_vars(Observe)

----------------------------------------------------------------------------

TypeOK ==
    /\ age \in 0..Horizon
    /\ recycle \in {"inflight", "settled"}
    /\ settleAge \in (1..Horizon) \cup {Never}
    /\ gate \in {"waiting", "delivered", "proceeded", "refused"}
    /\ waitStart \in 0..Horizon
    /\ delivered \in 0..1

(***************************************************************************)
(* The fix, as one sentence: a STAMPED recycle is never refused while it is  *)
(* still pending. It settles and the trigger is delivered, or its supervisor *)
(* is gone and dispatch proceeds, or (R9) the TTL ends the wait with a       *)
(* retryable refusal while the supervisor is still recycling.               *)
(*                                                                          *)
(* Stated over the whole run rather than over the observed window, so a      *)
(* future change to either budget cannot silently reopen the gap: there is   *)
(* no age at which a stamped recycle may refuse.                             *)
(***************************************************************************)
StampedRecycleNeverRefuses ==
    (gate = "refused") => (~Stamped \/ (age > Ttl /\ SupervisorAlive))

(* R9: dispatch never proceeds past a recycle whose supervisor is alive: the  *)
(* TTL is a bound on waiting, never evidence that the settle was lost.        *)
NeverProceedsPastALiveRecycle == (gate = "proceeded") => ~SupervisorAlive

(***************************************************************************)
(* The boundary is a hot-reload: a trigger delivered twice is as wrong as a  *)
(* trigger dropped. Re-arming the wait must not re-inject.                   *)
(***************************************************************************)
DeliveredAtMostOnce == delivered <= 1

(***************************************************************************)
(* No refusal may name an unblocker the caller itself just declined to       *)
(* perform. `wait_for_supervisor_recycle_settle` is exactly that unblocker,  *)
(* so refusing while the recycle is still inside its own pending window is   *)
(* self-contradictory advice.                                                *)
(***************************************************************************)
NoSelfContradictoryUnblocker ==
    (gate = "refused" /\ Stamped) => age > Ttl

(* Liveness: the gate always reaches a verdict. A wait that can re-arm        *)
(* forever would trade the false refusal for a hang, which is not a fix.      *)
GateEventuallyResolves == <>Terminal

(* Reach obligations — asserted so a config can require them VIOLATED.        *)
NeverDelivered == gate # "delivered"
NeverRefused == gate # "refused"

============================================================================
