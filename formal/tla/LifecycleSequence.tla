---------------------------- MODULE LifecycleSequence ----------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Level-state updates within ONE actor generation, over the adversarial
`NetChannel` (`#netadv3`; SimWorld findings SIM-F1 and SIM-F2 from
`#netadv4`, `src/sim_world/net.rs`).

Supervisor lifecycle (`mark_lifecycle`), the supervisor heartbeat
(`supervisor_heartbeat`, which carries the runtime state) and queue control
(`queue_control` pause/resume/drain) each carry the actor GENERATION and
nothing else. The generation fences a straggler from a previous owner, but
within one generation the controller applies whatever arrives LAST. A
reordered `Busy -> Ready` therefore leaves the controller reading Ready while
the supervisor is busy, and the next dispatch is typed into a busy pane
(SIM-F1); a reordered pause/resume flips queue control back (SIM-F2). Both
have the same shape, so one model covers them.

THE FIX (`SeqFence`)
--------------------
Every update is stamped at SEND time with a per-host monotonic sequence
(`agent_doc_controller::sequence`), the controller keeps the latest stamp it
applied per (family, document, generation), and an update whose stamp is
OLDER is discarded. The sender re-sends its current level state (the
heartbeat), so a dropped update is re-derived rather than retransmitted.

`LifecycleSequenceReorderWedge` (`SeqFence = FALSE`) MUST violate
`NoStaleUpdateApplied` and `ViewNeverRegresses`.
***************************************************************************)

CONSTANTS MaxCopies, MaxGen, MaxStamp, SeqFence

States == {"ready", "busy"}
Stamps == 1..MaxStamp

VARIABLES
    net, gen, delivered,  \* NetChannel (supervisor -> controller)
    actual,        \* supervisor: its real lifecycle state
    stamp,         \* supervisor: stamp of its latest transition
    view,          \* controller: lifecycle it acts on (dispatch reads this)
    lastStamp,     \* controller: newest stamp applied (0 = none)
    viewStamp,     \* controller: stamp the current view came from
    staleApplied,  \* history: an older update was applied after a newer one
    regressed      \* history: the view moved to an older stamp

C == INSTANCE NetChannel

protoVars == <<actual, stamp, view, lastStamp, viewStamp, staleApplied, regressed>>
vars == <<net, gen, delivered, protoVars>>

L(s, n) == [state |-> s, stamp |-> n]
Msgs == {L(s, n) : s \in States, n \in Stamps}

Other(s) == IF s = "ready" THEN "busy" ELSE "ready"

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ actual \in States
    /\ stamp \in Stamps
    /\ view \in States
    /\ lastStamp \in 0..MaxStamp
    /\ viewStamp \in 0..MaxStamp
    /\ staleApplied \in BOOLEAN
    /\ regressed \in BOOLEAN

\* The supervisor is Ready (stamp 1) and the controller already applied it.
Init ==
    /\ C!ChannelInit
    /\ actual = "ready"
    /\ stamp = 1
    /\ view = "ready"
    /\ lastStamp = 1
    /\ viewStamp = 1
    /\ staleApplied = FALSE
    /\ regressed = FALSE

\* The supervisor changes state and reports it (environment: no fairness).
Transition ==
    /\ stamp < MaxStamp
    /\ actual' = Other(actual)
    /\ stamp' = stamp + 1
    /\ C!Send(L(Other(actual), stamp + 1))
    /\ UNCHANGED <<view, lastStamp, viewStamp, staleApplied, regressed>>

\* The heartbeat re-sends the CURRENT level state (weakly fair).
Heartbeat ==
    /\ C!Send(L(actual, stamp))
    /\ UNCHANGED protoVars

ControllerRecv(m) ==
    /\ m \in C!InFlight
    /\ C!Deliver(m)
    /\ IF SeqFence /\ m.stamp < lastStamp
          THEN UNCHANGED <<view, lastStamp, viewStamp, staleApplied, regressed>>
          ELSE /\ view' = m.state
               /\ viewStamp' = m.stamp
               /\ lastStamp' = IF m.stamp > lastStamp THEN m.stamp ELSE lastStamp
               /\ staleApplied' = (staleApplied \/ m.stamp < lastStamp)
               /\ regressed' = (regressed \/ m.stamp < viewStamp)
    /\ UNCHANGED <<actual, stamp>>

AdversaryStep == C!Adversary /\ UNCHANGED protoVars

Next ==
    \/ Transition
    \/ Heartbeat
    \/ \E m \in Msgs : ControllerRecv(m)
    \/ AdversaryStep

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Heartbeat)
    /\ C!FairLossy(Msgs)

(* SAFETY (no fairness, full adversary).                                    *)
NoStaleUpdateApplied == ~staleApplied
ViewNeverRegresses == ~regressed

(* LIVENESS: the controller's view converges on the supervisor's state.     *)
ViewEventuallyCurrent == <>[](view = actual /\ viewStamp = stamp)

(* REACH (asserted negated; MUST be violated): a newer update does land.    *)
NeverObservesBusy == view = "ready"
=============================================================================
