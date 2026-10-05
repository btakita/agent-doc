----------------------- MODULE RecycleSettleDispatchNet -----------------------
EXTENDS Naturals, TLC

(***************************************************************************
`RecycleSettleDispatch` re-checked over the adversarial `NetChannel`
(`#netadv3`). The atomic module reads the settle-wait RPC as a variable read
that always returns by `WaitBudget`, and lets an elapsed `Ttl` stand for
"the settle transition was lost". Here the RPC is a request and a reply that
may be delayed, reordered, dropped, duplicated, or cut by a reconnect, and
there is no clock at all.

THE GATE (`agent-doc-route-io/src/dispatch_only/proof.rs`)
-----------------------------------------------------------
A dispatch-only reopen that finds the project supervisor mid-recycle waits on
`supervisor_recycle_wait_settled` (CLI -> controller). The reply carries the
recycle phase at reply time, a level read of durable state, so a duplicate or
reordered reply is harmless: an old `inflight` only re-arms, and a recycle
never goes back from `settled` to `inflight` within one epoch. When the RPC
fails, the gate re-reads the status (another round trip); when that ALSO
fails it refused with `controller unreachable`.

KNOBS (one wedge each; scripts/run_tla.sh asserts every wedge MUST violate)
---------------------------------------------------------------------------
  UnreachableIsRefusal  RSD-1 (pre-fix TRUE): two lost round trips are read
                        as a verdict and a stamped, live, still-pending
                        recycle is refused, which the atomic module's own
                        `StampedRecycleNeverRefuses` forbids. FIX: an
                        unreachable controller is not a pending-recycle
                        verdict; back off and re-arm
                        (RecycleSettleDispatchNetUnreachableWedge).
  TtlIsProof            R9 (code owned by #netadv5): an elapsed TTL is read as
                        "the settle was lost" while the supervisor is alive and
                        about to settle, so the trigger is injected across the
                        hot-reload boundary (RecycleSettleDispatchNetTtlWedge).
                        FALSE = abandon only on positive evidence that the
                        supervisor died (`#recycleinflightwedge`).

Each RPC is one request per connection (`rpc.rs`), so a reply can only answer
the request that opened its connection. `Arm` opens a new connection: it flips
`conn` and discards everything still in flight on the old one (closing a
socket loses both directions). A reply from a closed connection therefore never
answers a newer request, which is what lets a stale `inflight` reply only
re-arm. `GiveUp` (the RPC wait expired) is enabled only once nothing of the
current connection is in flight: the liveness-side abstraction of "the wait is
long enough". Safety never depends on it.
***************************************************************************)

CONSTANTS MaxCopies, MaxGen, Stamped, UnreachableIsRefusal, TtlIsProof

VARIABLES
    net, gen, delivered,   \* NetChannel (CLI <-> controller)
    recycle,      \* durable: "inflight" | "settled"
    supAlive,     \* the recycling supervisor is alive (it will settle)
    ttlFired,     \* PRE-FIX only: an arbitrary local "TTL elapsed" event
    gate,         \* "waiting" | "delivered" | "proceeded" | "refused"
    outstanding,  \* the CLI has an RPC in flight
    conn,         \* the current RPC connection (alternating identity)
    lost,         \* consecutive lost round trips (0..2)
    injectedLive  \* history: a trigger was injected while a LIVE recycle was inflight

C == INSTANCE NetChannel

protoVars == <<recycle, supAlive, ttlFired, gate, outstanding, conn, lost, injectedLive>>
vars == <<net, gen, delivered, protoVars>>

Conns == {0, 1}
Req(c) == [t |-> "req", c |-> c, phase |-> "none"]
Reply(c, p) == [t |-> "reply", c |-> c, phase |-> p]
Msgs == {Req(c) : c \in Conns} \cup {Reply(c, p) : c \in Conns, p \in {"inflight", "settled"}}
OnConn(c) == {m \in Msgs : m.c = c}

Terminal == gate \in {"delivered", "proceeded", "refused"}

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ recycle \in {"inflight", "settled"}
    /\ supAlive \in BOOLEAN
    /\ ttlFired \in BOOLEAN
    /\ gate \in {"waiting", "delivered", "proceeded", "refused"}
    /\ outstanding \in BOOLEAN
    /\ conn \in Conns
    /\ lost \in 0..2
    /\ injectedLive \in BOOLEAN

Init ==
    /\ C!ChannelInit
    /\ recycle = "inflight"
    /\ supAlive = TRUE
    /\ ttlFired = FALSE
    /\ gate = "waiting"
    /\ outstanding = FALSE
    /\ conn = 0
    /\ lost = 0
    /\ injectedLive = FALSE

---------------------------------------------------------------------------
(* SUPERVISOR. It settles (weakly fair) unless it dies first (environment). *)
Settle ==
    /\ recycle = "inflight"
    /\ supAlive
    /\ recycle' = "settled"
    /\ UNCHANGED <<net, gen, delivered, supAlive, ttlFired, gate, outstanding,
                   conn, lost, injectedLive>>

SupervisorDies ==
    /\ recycle = "inflight"
    /\ supAlive
    /\ supAlive' = FALSE
    /\ UNCHANGED <<net, gen, delivered, recycle, ttlFired, gate, outstanding,
                   conn, lost, injectedLive>>

\* PRE-FIX clock: the TTL may elapse at ANY point, alive supervisor or not.
TtlElapses ==
    /\ TtlIsProof
    /\ ~ttlFired
    /\ ttlFired' = TRUE
    /\ UNCHANGED <<net, gen, delivered, recycle, supAlive, gate, outstanding,
                   conn, lost, injectedLive>>

AdversaryStep == C!Adversary /\ UNCHANGED protoVars

---------------------------------------------------------------------------
(* THE GATE (CLI).                                                          *)

\* Arm (or re-arm) the blocking settle-wait RPC on a NEW connection; the old
\* connection's in-flight messages die with it.
Arm ==
    /\ gate = "waiting"
    /\ ~outstanding
    /\ outstanding' = TRUE
    /\ conn' = 1 - conn
    /\ net' = C!Put([m \in DOMAIN net \ OnConn(conn) |-> net[m]], Req(1 - conn))
    /\ delivered' = {}
    /\ UNCHANGED <<gen, recycle, supAlive, ttlFired, gate, lost, injectedLive>>

\* The RPC wait expired with nothing left in flight: a lost round trip.
GiveUp ==
    /\ gate = "waiting"
    /\ outstanding
    /\ C!InFlight \cap OnConn(conn) = {}
    /\ outstanding' = FALSE
    /\ lost' = IF lost < 2 THEN lost + 1 ELSE 2
    /\ gate' = IF UnreachableIsRefusal /\ lost + 1 >= 2 THEN "refused" ELSE gate
    /\ UNCHANGED <<net, gen, delivered, recycle, supAlive, ttlFired, conn,
                   injectedLive>>

\* An unstamped mark is unknown, not stale: the one surviving refusal.
Unstamped ==
    /\ gate = "waiting"
    /\ ~Stamped
    /\ recycle = "inflight"
    /\ gate' = "refused"
    /\ UNCHANGED <<net, gen, delivered, recycle, supAlive, ttlFired,
                   outstanding, conn, lost, injectedLive>>

\* Proceed past a recycle whose settle was LOST. Fixed: only on positive
\* evidence the supervisor died. Pre-fix: on the TTL alone.
Abandon ==
    /\ gate = "waiting"
    /\ Stamped
    /\ recycle = "inflight"
    /\ IF TtlIsProof THEN ttlFired ELSE ~supAlive
    /\ gate' = "proceeded"
    /\ injectedLive' = (injectedLive \/ supAlive)
    /\ UNCHANGED <<net, gen, delivered, recycle, supAlive, ttlFired,
                   outstanding, conn, lost>>

---------------------------------------------------------------------------
(* RECEIVERS.                                                               *)

\* The controller answers with the CURRENT phase (level read of state.db).
ControllerRecvReq(c) ==
    /\ Req(c) \in C!InFlight
    /\ C!DeliverAndSend(Req(c), Reply(c, recycle))
    /\ UNCHANGED protoVars

GateRecvReply(c, p) ==
    /\ Reply(c, p) \in C!InFlight
    /\ C!Deliver(Reply(c, p))
    /\ IF gate = "waiting" /\ outstanding /\ c = conn
          THEN /\ outstanding' = FALSE
               /\ lost' = 0
               /\ IF p = "settled"
                     THEN /\ gate' = "delivered"
                          /\ injectedLive' = (injectedLive \/ recycle = "inflight")
                     ELSE UNCHANGED <<gate, injectedLive>>
          ELSE UNCHANGED <<outstanding, lost, gate, injectedLive>>
    /\ UNCHANGED <<recycle, supAlive, ttlFired, conn>>

Done == Terminal /\ UNCHANGED vars

Next ==
    \/ Settle
    \/ SupervisorDies
    \/ TtlElapses
    \/ AdversaryStep
    \/ Arm
    \/ GiveUp
    \/ Unstamped
    \/ Abandon
    \/ \E c \in Conns : ControllerRecvReq(c)
    \/ \E c \in Conns, p \in {"inflight", "settled"} : GateRecvReply(c, p)
    \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Settle)
    /\ WF_vars(Arm)
    /\ WF_vars(GiveUp)
    /\ WF_vars(Unstamped)
    /\ WF_vars(Abandon)
    /\ C!FairLossy(Msgs)

---------------------------------------------------------------------------
(* SAFETY (no fairness, full adversary).                                    *)

\* The atomic module's main invariant survives a lossy network.
StampedRecycleNeverRefuses == gate = "refused" => ~Stamped

\* The gate exists so no trigger is typed across a LIVE hot-reload boundary.
NeverInjectsAcrossLiveRecycle == ~injectedLive

(* LIVENESS: the gate always reaches a verdict.                             *)
GateEventuallyResolves == <>Terminal

(* REACH (asserted negated; MUST be violated).                              *)
NeverDelivered == gate # "delivered"
NeverProceeds == gate # "proceeded"
NeverRefused == gate # "refused"
=============================================================================
