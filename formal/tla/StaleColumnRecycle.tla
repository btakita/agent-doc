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
(* invariants below cannot lean on ordering, timing, or delivery. The link  *)
(* is the shared adversarial `NetChannel` (`#netadv3`): a bag with Drop,    *)
(* Duplicate and Reconnect taken with no fairness at all. The pre-fix       *)
(* fire-and-forget notification rides the same channel, and only it gets   *)
(* `FairLossy`: a layout publication needs no delivery guarantee.           *)
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
(*                                                                          *)
(* `#stalesupervisorhidden` (2026-11-05): `GuardOverdue` alone HID an       *)
(* overdue pane until its request was consumed — for a supervisor that can  *)
(* never consume it, forever, while the same pane, had it been visible,     *)
(* stayed visible. `PromoteOverdue` promotes it FLAGGED instead: its own    *)
(* record, realising exactly the published layout. I1/I2 now bind the       *)
(* unflagged (recycle-backed) promotion; I4 forbids hiding an overdue pane. *)
(*                                                                          *)
(* `#stalewidenrestore` (2026-11-05, GH #343): `GuardWiden` measured every  *)
(* publication against the target window's CURRENT pane count. After an   *)
(* IDE restart's brief 1-window frame the window held one pane while the    *)
(* editor's own split showed two, and a stale stash pane whose recycle was  *)
(* still pending stayed hidden. The bound exists for a DERIVED column count *)
(* (the `ensure`-route `observed_panes + 1` feedback loop); an editor split *)
(* observation is the authority for its own width. `editorSplit` marks a    *)
(* publication whose column count is the editor's own; with                 *)
(* `EditorWidthAuthority` the gate admits such a publication's pending      *)
(* stash pane up to exactly that width (I6), and I2 still binds every       *)
(* derived publication.                                                     *)
(***************************************************************************)
EXTENDS Naturals, TLC

CONSTANTS
    MaxWidth,           \* widest layout the editor may publish
    MaxPublications,    \* editor publications (bounds the state space)
    MaxFanouts,         \* install fan-outs
    GuardOverdue,       \* FIX: no stash promotion while the request is overdue
    GuardWiden,         \* FIX: a stash promotion never widens the window
    DurableRequest,     \* FIX: level-triggered state.db request, re-read each boundary
    LapseFanout,        \* PRE-FIX: an install fan-out request lapses for the consumer
    RefreshResetsClock, \* PRE-FIX: a refresh restarts the consumption clock
    PromoteOverdue,     \* FIX: an overdue stale stash pane is promoted flagged, not hidden
    EditorWidthAuthority, \* FIX: an editor split's own width bounds a pending stash promotion
    EditorSplits,       \* `editorSplit` values publications may carry (bounds the state space)
    MaxCopies,          \* NetChannel: copies of one in-flight message
    MaxGen              \* NetChannel: reconnects (bounds the state space)

ASSUME /\ MaxWidth \in Nat /\ MaxWidth >= 2
       /\ MaxPublications \in Nat /\ MaxFanouts \in Nat
       /\ PromoteOverdue \in BOOLEAN
       /\ EditorWidthAuthority \in BOOLEAN
       /\ EditorSplits \subseteq BOOLEAN /\ EditorSplits # {}

VARIABLES
    stale,          \* the 1099.md supervisor runs replaced bytes
    req,            \* durable request: "none" | "pending" | "overdue"
    reason,         \* latest request reason: "stale" | "fanout"
    lapsed,         \* the latest fan-out request lapsed (pre-fix consumer)
    notified,       \* the consumer received the fire-and-forget notification
    cycleOpen,      \* the supervisor's document cycle is open (defers recycle)
    net, gen, delivered, \* NetChannel: layout publications + pre-fix notification
    published,      \* publications sent so far
    fanouts,        \* install fan-outs so far
    window,         \* panes in the target tmux window
    promoted,       \* the 1099.md pane is in the target window
    refusedOverdue, \* a stash promotion was refused as overdue, unconsumed since
    lastStep        \* what the last layout effect did (for step invariants)

C == INSTANCE NetChannel

protoVars == <<stale, req, reason, lapsed, notified, cycleOpen, published,
               fanouts, window, promoted, refusedOverdue, lastStep>>
vars == <<net, gen, delivered, protoVars>>

Max(a, b) == IF a > b THEN a ELSE b

(* One record shape for every message so the bag compares them uniformly.  *)
(* A publication carries no id: the controller applies any delivered layout *)
(* in any order, so two equal publications are just two copies in the bag.  *)
(* `editorSplit`: the publication's column count is a positive editor split *)
(* observation (plugin publication, `exact` route, editor-surface Sync), not *)
(* a count derived from retained controller state.                         *)
LayoutMsgs == [kind : {"layout"}, width : 1..MaxWidth, focus : BOOLEAN,
               editorSplit : BOOLEAN]
Notify == [kind |-> "notify", width |-> 1, focus |-> FALSE, editorSplit |-> FALSE]
Msgs == LayoutMsgs \cup {Notify}

NoStep == [promotedStale |-> FALSE, flagged |-> FALSE, hiddenOverdue |-> FALSE,
           width |-> 0, reqBefore |-> "none", windowBefore |-> 0,
           windowAfter |-> 0, refusedBefore |-> FALSE, editorSplit |-> FALSE,
           hiddenEditorRequested |-> FALSE]

TypeOK ==
    /\ stale \in BOOLEAN
    /\ req \in {"none", "pending", "overdue"}
    /\ reason \in {"stale", "fanout"}
    /\ lapsed \in BOOLEAN
    /\ notified \in BOOLEAN
    /\ cycleOpen \in BOOLEAN
    /\ C!ChannelTypeOK(Msgs)
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
    /\ notified = FALSE
    /\ cycleOpen \in BOOLEAN
    /\ C!ChannelInit
    /\ published = 0
    /\ fanouts = 0
    /\ window \in 1..MaxWidth
    /\ promoted = FALSE
    /\ refusedOverdue = FALSE
    /\ lastStep = NoStep

(* The editor publishes a layout: `width` columns, with or without 1099.md  *)
(* focused in one of them, as its own split observation or as a count the   *)
(* controller derived. A reconnect to a new editor generation is just more  *)
(* publications; nothing below trusts their order.                          *)
Publish(width, focus, editorSplit) ==
    /\ published < MaxPublications
    /\ published' = published + 1
    /\ C!Send([kind |-> "layout", width |-> width, focus |-> focus,
               editorSplit |-> editorSplit])
    /\ UNCHANGED <<stale, req, reason, lapsed, notified,
                   cycleOpen, fanouts, window, promoted, refusedOverdue, lastStep>>

(* The network misbehaves: Drop, Duplicate, Reconnect, in any order, with   *)
(* no fairness. The gate's facts are controller-local and durable, so a    *)
(* reconnect needs no resync here.                                         *)
AdversaryStep ==
    /\ C!Adversary
    /\ UNCHANGED protoVars

(* `stale_focus_admission` for a stale focused pane (no live turn here: the  *)
(* GH #124 guard is orthogonal and checked in Rust).                        *)
(* The flagged promotion: its recycle is not coming, so it is not hidden.   *)
FlaggedAdmit == PromoteOverdue /\ req = "overdue"

(* `#stalewidenrestore`: the editor's own split names this many columns,   *)
(* so realising exactly that many is not a widening the gate must refuse.  *)
EditorRequestedWidth(m) == EditorWidthAuthority /\ m.editorSplit

StaleAdmitted(m) ==
    \/ promoted                                   \* visible: never moved (no flap)
    \/ FlaggedAdmit
    \/ /\ GuardOverdue => req # "overdue"
       /\ GuardWiden => (m.width <= Max(window, 1) \/ EditorRequestedWidth(m))

(* One layout effect: the gate, tmux-router, and the request effect, all    *)
(* local to the controller. A duplicate is NetChannel's `Duplicate`.        *)
Deliver(m) ==
    /\ m.kind = "layout"
    /\ LET staleFocus == m.focus /\ stale
           admitted == ~staleFocus \/ StaleAdmitted(m)
           realised == IF admitted THEN m.width ELSE m.width - 1
           stalePromotion == staleFocus /\ ~promoted /\ admitted
           flaggedPromotion == stalePromotion /\ FlaggedAdmit
           requestNow == staleFocus /\ req = "none"
       IN
       /\ promoted' = (m.focus /\ admitted)
       \* Every column gated out: the current layout is preserved (GH #124).
       /\ window' = IF realised = 0 THEN window ELSE realised
       /\ refusedOverdue' = (refusedOverdue \/ (staleFocus /\ ~admitted /\ req = "overdue"))
       /\ lastStep' = [promotedStale |-> stalePromotion,
                       flagged |-> flaggedPromotion,
                       hiddenOverdue |-> staleFocus /\ ~admitted /\ req = "overdue",
                       width |-> m.width, reqBefore |-> req,
                       windowBefore |-> window, windowAfter |-> window',
                       refusedBefore |-> refusedOverdue,
                       editorSplit |-> m.editorSplit,
                       hiddenEditorRequested |->
                           staleFocus /\ ~admitted /\ m.editorSplit /\ req # "overdue"]
       \* The gate requests on the first stale pass and refreshes after.
       /\ req' = IF requestNow THEN "pending"
                 ELSE IF staleFocus /\ RefreshResetsClock THEN "pending"
                 ELSE req
       /\ reason' = IF staleFocus THEN "stale" ELSE reason
       /\ lapsed' = IF staleFocus THEN FALSE ELSE lapsed
       \* PRE-FIX: the first request is a one-shot notification on the wire.
       /\ IF requestNow /\ ~DurableRequest
             THEN C!DeliverAndSend(m, Notify)
             ELSE C!Deliver(m)
    /\ UNCHANGED <<stale, notified, cycleOpen, published, fanouts>>

(* The consumption bound elapsed while the consumer sat idle. Arbitrary:   *)
(* the safety invariants must hold however early or late it fires.         *)
Expire ==
    /\ req = "pending"
    /\ req' = "overdue"
    /\ UNCHANGED <<net, gen, delivered, stale, reason, lapsed, notified,
                   cycleOpen, published, fanouts, window, promoted,
                   refusedOverdue, lastStep>>

(* An install fan-out writes a request for every open supervisor. It        *)
(* refreshes the latest reason; the first-unconsumed time is kept.         *)
Fanout ==
    /\ stale
    /\ fanouts < MaxFanouts
    /\ fanouts' = fanouts + 1
    /\ reason' = "fanout"
    /\ lapsed' = FALSE
    /\ req' = IF req = "none" \/ RefreshResetsClock THEN "pending" ELSE req
    /\ IF DurableRequest THEN UNCHANGED <<net, gen, delivered>> ELSE C!Send(Notify)
    /\ UNCHANGED <<stale, notified, cycleOpen, published, window,
                   promoted, refusedOverdue, lastStep>>

(* PRE-FIX: the consumer stops honouring a fan-out request after its TTL.  *)
Lapse ==
    /\ LapseFanout
    /\ reason = "fanout"
    /\ ~lapsed
    /\ lapsed' = TRUE
    /\ UNCHANGED <<net, gen, delivered, stale, req, reason, notified,
                   cycleOpen, published, fanouts, window, promoted,
                   refusedOverdue, lastStep>>

(* The consumer takes the pre-fix notification off the wire. Always enabled *)
(* while one is in flight, so `FairLossy` applies to it.                    *)
DeliverNotify(m) ==
    /\ m = Notify
    /\ C!Deliver(m)
    /\ notified' = TRUE
    /\ UNCHANGED <<stale, req, reason, lapsed, cycleOpen, published,
                   fanouts, window, promoted, refusedOverdue, lastStep>>

CloseCycle ==
    /\ cycleOpen
    /\ cycleOpen' = FALSE
    /\ UNCHANGED <<net, gen, delivered, stale, req, reason, lapsed, notified,
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
    /\ UNCHANGED <<net, gen, delivered, reason, lapsed, cycleOpen, published,
                   fanouts, window, promoted, lastStep>>

Quiescent == UNCHANGED vars

Next ==
    \/ \E w \in 1..MaxWidth, f \in BOOLEAN, e \in EditorSplits : Publish(w, f, e)
    \/ \E m \in C!InFlight : Deliver(m) \/ DeliverNotify(m)
    \/ AdversaryStep
    \/ Expire
    \/ Fanout
    \/ Lapse
    \/ CloseCycle
    \/ Consume
    \/ Quiescent

(* Fairness only where the system owns the step: the supervisor's own idle  *)
(* boundary and cycle closure. The channel is fair-lossy for the pre-fix    *)
(* notification only (a message sent finitely often may still be lost      *)
(* forever); the editor, the layout publications, and the clock get no      *)
(* fairness at all.                                                         *)
Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(CloseCycle)
    /\ WF_vars(Consume)
    /\ C!FairLossy({Notify})

-----------------------------------------------------------------------------
(* Safety. Every one is a step invariant over `lastStep`, independent of    *)
(* message order, delivery, duplication, and when `Expire` fires.           *)

(* I1: a stale pane is never promoted out of the stash on the strength of a *)
(* request its consumer already let go overdue: an overdue promotion is     *)
(* always the flagged one.                                                  *)
NoStashPromotionWhileOverdue ==
    lastStep.promotedStale /\ ~lastStep.flagged => lastStep.reqBefore # "overdue"

(* I2: promoting a stale stash pane never widens the target window — the    *)
(* `columns = observed_panes + 1` flap — beyond what the editor's own split *)
(* shows (`#stalewidenrestore`): a derived count never widens it at all.    *)
StashPromotionNeverWidens ==
    lastStep.promotedStale /\ ~lastStep.flagged
        => \/ lastStep.windowAfter <= Max(lastStep.windowBefore, 1)
           \/ lastStep.editorSplit /\ lastStep.windowAfter = lastStep.width

(* I4 (`#stalesupervisorhidden`): a layout naming the stale pane while its  *)
(* recycle is overdue never hides it.                                       *)
OverdueStalePaneNeverHidden == ~lastStep.hiddenOverdue

(* I6 (`#stalewidenrestore`, GH #343): a stale pane named by the editor's   *)
(* own split is never kept in the stash for width while its recycle is not *)
(* overdue — the editor's column list is authoritative.                     *)
EditorRequestedStalePaneNeverHidden == ~lastStep.hiddenEditorRequested

(* I5: a flagged promotion realises exactly the published layout.          *)
FlaggedPromotionRealisesThePublication ==
    lastStep.flagged => lastStep.windowAfter = lastStep.width

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
NeverPromotesFlagged == ~lastStep.flagged
NeverWidensForEditor ==
    ~(lastStep.promotedStale /\ ~lastStep.flagged
      /\ lastStep.windowAfter > Max(lastStep.windowBefore, 1))
NeverConsumed == stale
=============================================================================
