--------------------------- MODULE StaleColumnRecycle ---------------------------
(***************************************************************************)
(* GH #136 — a stale stash-window supervisor pane admitted as a layout       *)
(* column, and the safe-boundary recycle request that was never consumed.    *)
(*                                                                          *)
(* Actors and links                                                         *)
(*                                                                          *)
(*   editor  --(adversarial channel: delay, reorder, drop, duplicate,       *)
(*              reconnect)-->  controller + tmux (co-located, local)        *)
(*   controller --(durable state.db request, level-triggered)--> supervisor *)
(*                                                                          *)
(* The editor runs behind JetBrains Remote Dev and Zscaler, so a layout     *)
(* publication can arrive late, out of order, twice, or never. The model    *)
(* lets the controller apply ANY delivered publication in ANY order —       *)
(* stronger than the production generation arbiter — so the safety         *)
(* invariants below cannot lean on ordering, timing, or delivery. `chan` is *)
(* a plain set with explicit Drop and duplicate-on-deliver so it can later  *)
(* be swapped for the shared NetChannel module without touching the gate.   *)
(*                                                                          *)
(* The column gate (`stale_focus_admission`) decides only on facts that are *)
(* LOCAL to the controller at effect time: whether the pane is in the       *)
(* stash, the target window's current pane count, and the durable request's *)
(* lifecycle state. `Expire` (request pending -> overdue) abstracts the     *)
(* consumption bound as an arbitrary local event, so no timeout value       *)
(* appears in any safety argument: the invariants hold whenever it fires.   *)
(*                                                                          *)
(* The recycle request is durable, level-triggered state the supervisor     *)
(* re-reads at every idle boundary (`DurableRequest`). Its pre-fix flaws:   *)
(*   * an install fan-out replaced a non-lapsing stale-supervisor request   *)
(*     with one that lapsed after its TTL (`LapseFanout`);                  *)
(*   * the admission had no bound: it promoted a stale stash pane on the    *)
(*     strength of a request that was never consumed (`GuardOverdue`), and  *)
(*     widened the window to do it (`GuardWiden`);                          *)
(*   * a refresh that reset the consumption clock would re-admit an overdue *)
(*     pane — a flap (`RefreshResetsClock`).                                *)
(* A fire-and-forget notification (`DurableRequest = FALSE`) is modelled to *)
(* show why the request must stay durable: under Drop it stalls forever.    *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS
    MaxWidth,           \* widest layout the editor may publish
    MaxPublications,    \* editor publications (bounds the state space)
    MaxFanouts,         \* install fan-outs
    GuardOverdue,       \* FIX: no stash promotion while the request is overdue
    GuardWiden,         \* FIX: a stash promotion never widens the window
    DurableRequest,     \* FIX: level-triggered state.db request, re-read each boundary
    LapseFanout,        \* PRE-FIX: an install fan-out request lapses for the consumer
    RefreshResetsClock  \* PRE-FIX: a refresh restarts the consumption clock

ASSUME /\ MaxWidth \in Nat /\ MaxWidth >= 2
       /\ MaxPublications \in Nat /\ MaxFanouts \in Nat

VARIABLES
    stale,          \* the 1099.md supervisor runs replaced bytes
    req,            \* durable request: "none" | "pending" | "overdue"
    reason,         \* latest request reason: "stale" | "fanout"
    lapsed,         \* the latest fan-out request lapsed (pre-fix consumer)
    notifyInFlight, \* fire-and-forget notification in flight (pre-fix only)
    notified,       \* the consumer received that notification
    cycleOpen,      \* the supervisor's document cycle is open (defers recycle)
    chan,           \* editor -> controller layout publications in flight
    published,      \* publications sent so far
    fanouts,        \* install fan-outs so far
    window,         \* panes in the target tmux window
    promoted,       \* the 1099.md pane is in the target window
    refusedOverdue, \* a stash promotion was refused as overdue, unconsumed since
    lastStep        \* what the last layout effect did (for step invariants)

vars == <<stale, req, reason, lapsed, notifyInFlight, notified, cycleOpen,
          chan, published, fanouts, window, promoted, refusedOverdue, lastStep>>

Max(a, b) == IF a > b THEN a ELSE b

Msgs == [id : 1..MaxPublications, width : 1..MaxWidth, focus : BOOLEAN]

NoStep == [promotedStale |-> FALSE, reqBefore |-> "none", windowBefore |-> 0,
           windowAfter |-> 0, refusedBefore |-> FALSE]

TypeOK ==
    /\ stale \in BOOLEAN
    /\ req \in {"none", "pending", "overdue"}
    /\ reason \in {"stale", "fanout"}
    /\ lapsed \in BOOLEAN
    /\ notifyInFlight \in BOOLEAN
    /\ notified \in BOOLEAN
    /\ cycleOpen \in BOOLEAN
    /\ chan \subseteq Msgs
    /\ published \in 0..MaxPublications
    /\ fanouts \in 0..MaxFanouts
    /\ window \in 1..MaxWidth
    /\ promoted \in BOOLEAN
    /\ refusedOverdue \in BOOLEAN

Init ==
    /\ stale = TRUE
    /\ req = "none"
    /\ reason = "stale"
    /\ lapsed = FALSE
    /\ notifyInFlight = FALSE
    /\ notified = FALSE
    /\ cycleOpen \in BOOLEAN
    /\ chan = {}
    /\ published = 0
    /\ fanouts = 0
    /\ window \in 1..MaxWidth
    /\ promoted = FALSE
    /\ refusedOverdue = FALSE
    /\ lastStep = NoStep

(* The editor publishes a layout: `width` columns, with or without 1099.md  *)
(* focused in one of them. A reconnect to a new editor generation is just   *)
(* more publications; nothing below trusts their order.                     *)
Publish(width, focus) ==
    /\ published < MaxPublications
    /\ published' = published + 1
    /\ chan' = chan \cup {[id |-> published + 1, width |-> width, focus |-> focus]}
    /\ UNCHANGED <<stale, req, reason, lapsed, notifyInFlight, notified,
                   cycleOpen, fanouts, window, promoted, refusedOverdue, lastStep>>

Drop(m) ==
    /\ chan' = chan \ {m}
    /\ UNCHANGED <<stale, req, reason, lapsed, notifyInFlight, notified,
                   cycleOpen, published, fanouts, window, promoted,
                   refusedOverdue, lastStep>>

(* `stale_focus_admission` for a stale focused pane (no live turn here: the  *)
(* GH #124 guard is orthogonal and checked in Rust).                        *)
StaleAdmitted(width) ==
    \/ promoted                                   \* visible: never moved (no flap)
    \/ /\ GuardOverdue => req # "overdue"
       /\ GuardWiden => width <= Max(window, 1)

(* One layout effect: the gate, tmux-router, and the request effect, all    *)
(* local to the controller. A duplicate leaves the message in the channel.  *)
Deliver(m) ==
    /\ \/ chan' = chan \ {m}
       \/ chan' = chan
    /\ LET staleFocus == m.focus /\ stale
           admitted == ~staleFocus \/ StaleAdmitted(m.width)
           realised == IF admitted THEN m.width ELSE m.width - 1
           stalePromotion == staleFocus /\ ~promoted /\ admitted
           requestNow == staleFocus /\ req = "none"
       IN
       /\ promoted' = (m.focus /\ admitted)
       \* Every column gated out: the current layout is preserved (GH #124).
       /\ window' = IF realised = 0 THEN window ELSE realised
       /\ refusedOverdue' = (refusedOverdue \/ (staleFocus /\ ~admitted /\ req = "overdue"))
       /\ lastStep' = [promotedStale |-> stalePromotion, reqBefore |-> req,
                       windowBefore |-> window, windowAfter |-> window',
                       refusedBefore |-> refusedOverdue]
       \* The gate requests on the first stale pass and refreshes after.
       /\ req' = IF requestNow THEN "pending"
                 ELSE IF staleFocus /\ RefreshResetsClock THEN "pending"
                 ELSE req
       /\ reason' = IF staleFocus THEN "stale" ELSE reason
       /\ lapsed' = IF staleFocus THEN FALSE ELSE lapsed
       /\ notifyInFlight' = (notifyInFlight \/ (requestNow /\ ~DurableRequest))
    /\ UNCHANGED <<stale, notified, cycleOpen, published, fanouts>>

(* The consumption bound elapsed while the consumer sat idle. Arbitrary:   *)
(* the safety invariants must hold however early or late it fires.         *)
Expire ==
    /\ req = "pending"
    /\ req' = "overdue"
    /\ UNCHANGED <<stale, reason, lapsed, notifyInFlight, notified, cycleOpen,
                   chan, published, fanouts, window, promoted, refusedOverdue,
                   lastStep>>

(* An install fan-out writes a request for every open supervisor. It        *)
(* refreshes the latest reason; the first-unconsumed time is kept.         *)
Fanout ==
    /\ stale
    /\ fanouts < MaxFanouts
    /\ fanouts' = fanouts + 1
    /\ reason' = "fanout"
    /\ lapsed' = FALSE
    /\ req' = IF req = "none" \/ RefreshResetsClock THEN "pending" ELSE req
    /\ notifyInFlight' = (notifyInFlight \/ ~DurableRequest)
    /\ UNCHANGED <<stale, notified, cycleOpen, chan, published, window,
                   promoted, refusedOverdue, lastStep>>

(* PRE-FIX: the consumer stops honouring a fan-out request after its TTL.  *)
Lapse ==
    /\ LapseFanout
    /\ reason = "fanout"
    /\ ~lapsed
    /\ lapsed' = TRUE
    /\ UNCHANGED <<stale, req, reason, notifyInFlight, notified, cycleOpen,
                   chan, published, fanouts, window, promoted, refusedOverdue,
                   lastStep>>

DropNotify ==
    /\ notifyInFlight
    /\ notifyInFlight' = FALSE
    /\ UNCHANGED <<stale, req, reason, lapsed, notified, cycleOpen, chan,
                   published, fanouts, window, promoted, refusedOverdue, lastStep>>

DeliverNotify ==
    /\ notifyInFlight
    /\ notifyInFlight' = FALSE
    /\ notified' = TRUE
    /\ UNCHANGED <<stale, req, reason, lapsed, cycleOpen, chan, published,
                   fanouts, window, promoted, refusedOverdue, lastStep>>

CloseCycle ==
    /\ cycleOpen
    /\ cycleOpen' = FALSE
    /\ UNCHANGED <<stale, req, reason, lapsed, notifyInFlight, notified, chan,
                   published, fanouts, window, promoted, refusedOverdue, lastStep>>

(* The supervisor's idle boundary. The durable request is re-read on every  *)
(* boundary, so nothing about it can be "missed"; a fire-and-forget         *)
(* consumer acts only on a notification it actually received.               *)
ConsumerSees ==
    IF DurableRequest
    THEN req # "none" /\ ~(reason = "fanout" /\ lapsed)
    ELSE notified

(* Consumption is the acknowledgement: the supervisor re-execs onto the     *)
(* installed build (it then reads fresh) and settles the request epoch.     *)
Consume ==
    /\ stale
    /\ ~cycleOpen
    /\ ConsumerSees
    /\ stale' = FALSE
    /\ req' = "none"
    /\ notified' = FALSE
    /\ refusedOverdue' = FALSE
    /\ UNCHANGED <<reason, lapsed, notifyInFlight, cycleOpen, chan, published,
                   fanouts, window, promoted, lastStep>>

Quiescent == UNCHANGED vars

Next ==
    \/ \E w \in 1..MaxWidth, f \in BOOLEAN : Publish(w, f)
    \/ \E m \in chan : Drop(m) \/ Deliver(m)
    \/ Expire
    \/ Fanout
    \/ Lapse
    \/ DropNotify
    \/ DeliverNotify
    \/ CloseCycle
    \/ Consume
    \/ Quiescent

(* Fairness only where the system owns the step: the supervisor's own idle  *)
(* boundary and cycle closure, and delivery of a message that stays         *)
(* enabled. The editor, the channel, and the clock get no fairness.         *)
Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(CloseCycle)
    /\ WF_vars(Consume)
    /\ WF_vars(DeliverNotify)

-----------------------------------------------------------------------------
(* Safety. Every one is a step invariant over `lastStep`, independent of    *)
(* message order, delivery, duplication, and when `Expire` fires.           *)

(* I1: a stale pane is never promoted out of the stash on the strength of a *)
(* request its consumer already let go overdue.                             *)
NoStashPromotionWhileOverdue ==
    lastStep.promotedStale => lastStep.reqBefore # "overdue"

(* I2: promoting a stale stash pane never widens the target window — the    *)
(* `columns = observed_panes + 1` flap.                                     *)
StashPromotionNeverWidens ==
    lastStep.promotedStale => lastStep.windowAfter <= Max(lastStep.windowBefore, 1)

(* I3: once a promotion was refused as overdue, no later promotion happens  *)
(* until the request is consumed — refreshes and fan-outs cannot re-open    *)
(* the window, so the layout cannot flap.                                   *)
NoFlapAfterOverdueRefusal ==
    lastStep.promotedStale => ~lastStep.refusedBefore

(* Liveness, under the fairness above: an outstanding request is eventually *)
(* consumed. Not implied by the safety invariants — a wedge config shows it *)
(* fails both for a lapsing request and for a fire-and-forget one.          *)
EventuallyConsumed == (stale /\ req # "none") ~> ~stale

(* Reach obligations (asserted negated; MUST be violated).                  *)
NeverPromotesStale == ~lastStep.promotedStale
NeverConsumed == stale
=============================================================================
