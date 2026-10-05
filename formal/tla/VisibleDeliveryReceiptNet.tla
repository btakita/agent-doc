----------------------- MODULE VisibleDeliveryReceiptNet -----------------------
EXTENDS Naturals, TLC

(***************************************************************************
`VisibleDeliveryReceipt` re-checked over the adversarial `NetChannel`
(`#netadv3`). The atomic module proves the receipt ladder's DECISIONS (the
availability/receipt firewall and the drop-on-definitive-refusal edge) with
every answer instant and truthful. This module keeps those decisions and
replaces every cross-process step with messages that may be delayed,
reordered, dropped, duplicated, or stranded across a reconnect.

THE HOPS (audit P3, P4, P2/P8, docs/reference/network-channel-audit.md)
---------------------------------------------------------------------
  controller --Wake-------> editor   `deliver_crdt_remote` wake; the editor
                                     pulls the CURRENT canonical (level read)
  editor     --Ack(v)-----> controller `replica_projection` ACK, keyed by the
                                     content it proves (cumulative hash)
  controller --Recover----> editor   `claim_nonconverging_recovery` re-register
  editor     --Refuse-----> controller the endpoint ANSWERED and refused
  controller --Save(v)----> editor   `persist_current` for the receipted cut
  editor     --Saved(v)---> controller native-save receipt (`disk_persisted`)

A refusing endpoint still pulls on every wake and never ACKs: that pull is
the production `PullWithoutAck` loop and it charges the non-convergence
streak. A foreground waiter (preflight, the CLI receipt wait) may also charge
the streak, but nothing guarantees a waiter exists, so `ChargeWait` has NO
fairness: progress must come from controller-owned resends.

KNOBS (one wedge each; scripts/run_tla.sh asserts every wedge MUST violate)
---------------------------------------------------------------------------
  WakeResend        F9  FIX: the controller re-sends the wake until the
                        projection ACK clears `pending`
                        (`agent-doc-crdt-relay-io` re-wake scheduler).
                        FALSE = one wake per publication: a single Drop
                        strands the update with no waiter to notice.
  RecoveryRearm     F10 FIX: an unanswered recovery request is re-sent
                        instead of staying latched. FALSE = the one-shot
                        latch: a dropped Recover never yields the refusal the
                        drop edge needs, and the refusing replica vetoes the
                        receipt forever.
  SaveResend        F13 FIX (and F12 at protocol level): the retained native
                        save is re-requested while the receipted cut is not
                        known saved. FALSE = a bounded retry budget that, once
                        spent, waits for an edge an idle document never makes.
  ReceiptsKeyedByVersion  receipts name the content they prove (already true
                        in code: cumulative content-hash ACK, hash/len CAS on
                        save). FALSE = a stale ACK or save receipt for an older
                        cut is applied to the newer one: SAFETY wedge.
  TimeoutIsRefusal  R2/F2 (owned by #netadv5): an unanswered recovery is
                        classified as a definitive refusal. Safety wedge:
                        a still-serving replica is dropped from the cut.

Safety holds under `[][Next]_vars` alone (no fairness, full adversary).
Liveness holds under `FairLossy` plus weak fairness on the controller's own
resends.
***************************************************************************)

CONSTANTS
    MaxVersion,             \* canonical publications (bounds the state space)
    Budget,                 \* non-convergence streak before barrier release
    MaxCopies, MaxGen,      \* NetChannel bounds
    WakeResend, RecoveryRearm, SaveResend, ReceiptsKeyedByVersion,
    TimeoutIsRefusal

ASSUME MaxVersion \in Nat /\ MaxVersion >= 1 /\ Budget \in Nat /\ Budget >= 1

VARIABLES
    net, gen, delivered,    \* NetChannel
    cv,             \* controller: canonical version published to the replica
    pending,        \* hub: the live member has an unacked update queued
    streak,         \* hub: non-convergence evidence (pulls without ack, waits)
    member,         \* hub: "live" | "offline" (delivery-cut membership)
    signaled,       \* hub: recovery claimed for the current non-convergence
    refused,        \* hub: an Refuse ANSWER was received
    saveSentV,      \* controller: latest version a save request was sent for
    settledV,       \* controller: latest version KNOWN on disk (receipt)
    mode,           \* editor endpoint: "serves" | "refuses" this document
    ev,             \* editor: version visible in the editor buffer
    diskV           \* disk: version on disk

C == INSTANCE NetChannel

protoVars == <<cv, pending, streak, member, signaled, refused, saveSentV,
               settledV, mode, ev, diskV>>
vars == <<net, gen, delivered, protoVars>>

Versions == 1..MaxVersion
Wake == [t |-> "wake", v |-> 0]
Recover == [t |-> "recover", v |-> 0]
Refuse == [t |-> "refuse", v |-> 0]
Ack(v) == [t |-> "ack", v |-> v]
Save(v) == [t |-> "save", v |-> v]
Saved(v) == [t |-> "saved", v |-> v]
Msgs == {Wake, Recover, Refuse}
        \cup {Ack(v) : v \in Versions}
        \cup {Save(v) : v \in Versions}
        \cup {Saved(v) : v \in Versions}

HoldsBarrier == member = "live" /\ pending /\ streak < Budget
DeliveryConverged == ~HoldsBarrier
VisibleDeliveryProjected == (member = "live") => ~pending

\* Does a receipt for version v prove the CURRENT cut?
Proves(v) == IF ReceiptsKeyedByVersion THEN v = cv ELSE TRUE

Min(a, b) == IF a < b THEN a ELSE b

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ cv \in Versions
    /\ pending \in BOOLEAN
    /\ streak \in 0..Budget
    /\ member \in {"live", "offline"}
    /\ signaled \in BOOLEAN
    /\ refused \in BOOLEAN
    /\ saveSentV \in 0..MaxVersion
    /\ settledV \in 0..MaxVersion
    /\ mode \in {"serves", "refuses"}
    /\ ev \in 0..MaxVersion
    /\ diskV \in 0..MaxVersion

\* Version 1 is published with its wake already on the wire.
Init ==
    /\ net = (Wake :> 1)
    /\ gen = 0
    /\ delivered = {}
    /\ cv = 1
    /\ pending = TRUE
    /\ streak = 0
    /\ member = "live"
    /\ signaled = FALSE
    /\ refused = FALSE
    /\ saveSentV = 0
    /\ settledV = 0
    /\ mode = "serves"
    /\ ev = 0
    /\ diskV = 0

---------------------------------------------------------------------------
(* ENVIRONMENT (no fairness).                                              *)

\* The agent publishes a newer canonical cut.
Publish ==
    /\ cv < MaxVersion
    /\ cv' = cv + 1
    /\ pending' = (member = "live")
    /\ streak' = 0
    /\ signaled' = FALSE
    /\ IF member = "live" THEN C!Send(Wake) ELSE UNCHANGED <<net, gen, delivered>>
    /\ UNCHANGED <<member, refused, saveSentV, settledV, mode, ev, diskV>>

\* The endpoint stops serving this document while a delivery is outstanding
\* (a cdylib reload, a retired native generation). It never comes back on
\* its own: reopening the tab is the operator action the design must not need.
\* Scoped like the atomic module: it stops BEFORE showing the current cut.
\* An endpoint that stops after its ACK left but before the native save is
\* `RefusedSaveOperatorAction`'s territory (see HANDOFF.md, VDRN-post-ack).
EndpointStopsServing ==
    /\ mode = "serves"
    /\ member = "live"
    /\ pending
    /\ ev # cv
    /\ mode' = "refuses"
    /\ UNCHANGED <<net, gen, delivered, cv, pending, streak, member, signaled,
                   refused, saveSentV, settledV, ev, diskV>>

\* A foreground waiter's bounded wait expired with no delivery progress.
ChargeWait ==
    /\ HoldsBarrier
    /\ streak' = streak + 1
    /\ UNCHANGED <<net, gen, delivered, cv, pending, member, signaled, refused,
                   saveSentV, settledV, mode, ev, diskV>>

\* R2/F2 (#netadv5): an unanswered recovery read as a definitive refusal.
TimeoutRefusal ==
    /\ TimeoutIsRefusal
    /\ signaled
    /\ ~refused
    /\ refused' = TRUE
    /\ UNCHANGED <<net, gen, delivered, cv, pending, streak, member, signaled,
                   saveSentV, settledV, mode, ev, diskV>>

AdversaryStep == C!Adversary /\ UNCHANGED protoVars

---------------------------------------------------------------------------
(* CONTROLLER (its own steps get weak fairness).                           *)

ResendWake ==
    /\ WakeResend
    /\ member = "live"
    /\ pending
    /\ C!Send(Wake)
    /\ UNCHANGED protoVars

ClaimRecovery ==
    /\ member = "live"
    /\ pending
    /\ ~HoldsBarrier
    /\ ~signaled
    /\ signaled' = TRUE
    /\ C!Send(Recover)
    /\ UNCHANGED <<cv, pending, streak, member, refused, saveSentV, settledV,
                   mode, ev, diskV>>

ResendRecover ==
    /\ RecoveryRearm
    /\ signaled
    /\ ~refused
    /\ member = "live"
    /\ pending
    /\ C!Send(Recover)
    /\ UNCHANGED protoVars

\* The drop edge from the atomic module, unchanged: only on a received
\* refusal, only once the replica no longer holds the barrier.
DropRefusedFromDeliveryCut ==
    /\ refused
    /\ member = "live"
    /\ pending
    /\ ~HoldsBarrier
    /\ member' = "offline"
    /\ pending' = FALSE
    /\ streak' = 0
    /\ UNCHANGED <<net, gen, delivered, cv, signaled, refused, saveSentV,
                   settledV, mode, ev, diskV>>

\* Request the native save of the receipted cut. Gated on the RECEIPT.
SendSave ==
    /\ member = "live"
    /\ VisibleDeliveryProjected
    /\ settledV < cv
    /\ SaveResend \/ saveSentV # cv
    /\ saveSentV' = cv
    /\ C!Send(Save(cv))
    /\ UNCHANGED <<cv, pending, streak, member, signaled, refused, settledV,
                   mode, ev, diskV>>

\* No live editor in the cut: agent-doc's own authority write owns disk.
DetachedWrite ==
    /\ member = "offline"
    /\ settledV < cv
    /\ diskV' = cv
    /\ settledV' = cv
    /\ UNCHANGED <<net, gen, delivered, cv, pending, streak, member, signaled,
                   refused, saveSentV, mode, ev>>

---------------------------------------------------------------------------
(* RECEIVERS. Each is enabled whenever its message is in flight, so the    *)
(* fair-lossy assumption applies to every message.                         *)

EditorRecvWake ==
    /\ Wake \in C!InFlight
    /\ IF mode = "serves"
          THEN /\ ev' = cv
               /\ C!DeliverAndSend(Wake, Ack(cv))
               /\ UNCHANGED streak
          ELSE \* PullWithoutAck: the pull reaches the hub and charges it.
               /\ C!Deliver(Wake)
               /\ streak' = IF HoldsBarrier THEN streak + 1 ELSE streak
               /\ UNCHANGED ev
    /\ UNCHANGED <<cv, pending, member, signaled, refused, saveSentV, settledV,
                   mode, diskV>>

EditorRecvRecover ==
    /\ Recover \in C!InFlight
    /\ IF mode = "serves"
          THEN /\ ev' = cv     \* re-register: state-vector bootstrap
               /\ C!DeliverAndSend(Recover, Ack(cv))
          ELSE /\ C!DeliverAndSend(Recover, Refuse)
               /\ UNCHANGED ev
    /\ UNCHANGED <<cv, pending, streak, member, signaled, refused, saveSentV,
                   settledV, mode, diskV>>

EditorRecvSave(v) ==
    /\ Save(v) \in C!InFlight
    /\ IF mode = "serves" /\ ev = v     \* hash/len CAS on the editor buffer
          THEN /\ diskV' = v
               /\ C!DeliverAndSend(Save(v), Saved(v))
          ELSE /\ C!Deliver(Save(v))
               /\ UNCHANGED diskV
    /\ UNCHANGED <<cv, pending, streak, member, signaled, refused, saveSentV,
                   settledV, mode, ev>>

ControllerRecvAck(v) ==
    /\ Ack(v) \in C!InFlight
    /\ C!Deliver(Ack(v))
    /\ IF member = "live" /\ pending /\ Proves(v)
          THEN /\ pending' = FALSE
               /\ streak' = 0
               /\ signaled' = FALSE
          ELSE UNCHANGED <<pending, streak, signaled>>
    /\ UNCHANGED <<cv, member, refused, saveSentV, settledV, mode, ev, diskV>>

ControllerRecvRefuse ==
    /\ Refuse \in C!InFlight
    /\ C!Deliver(Refuse)
    /\ refused' = TRUE
    /\ UNCHANGED <<cv, pending, streak, member, signaled, saveSentV, settledV,
                   mode, ev, diskV>>

ControllerRecvSaved(v) ==
    /\ Saved(v) \in C!InFlight
    /\ C!Deliver(Saved(v))
    /\ settledV' = IF Proves(v) THEN cv ELSE settledV
    /\ UNCHANGED <<cv, pending, streak, member, signaled, refused, saveSentV,
                   mode, ev, diskV>>

Recv ==
    \/ EditorRecvWake
    \/ EditorRecvRecover
    \/ \E v \in Versions : EditorRecvSave(v)
    \/ \E v \in Versions : ControllerRecvAck(v)
    \/ ControllerRecvRefuse
    \/ \E v \in Versions : ControllerRecvSaved(v)

Next ==
    \/ Publish
    \/ EndpointStopsServing
    \/ ChargeWait
    \/ TimeoutRefusal
    \/ AdversaryStep
    \/ ResendWake
    \/ ClaimRecovery
    \/ ResendRecover
    \/ DropRefusedFromDeliveryCut
    \/ SendSave
    \/ DetachedWrite
    \/ Recv

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ResendWake)
    /\ WF_vars(ClaimRecovery)
    /\ WF_vars(ResendRecover)
    /\ WF_vars(DropRefusedFromDeliveryCut)
    /\ WF_vars(SendSave)
    /\ WF_vars(DetachedWrite)
    /\ C!FairLossy(Msgs)

---------------------------------------------------------------------------
(* SAFETY: hold with no fairness, under the full adversary.                *)

\* The `#silentreplicabarrier` firewall survives the network.
AvailabilityIsNotAReceipt ==
    (DeliveryConverged /\ member = "live" /\ pending) => ~VisibleDeliveryProjected

\* The receipt is never earned by a stale or reordered ACK: a live member
\* with nothing pending really shows the current cut.
ReceiptImpliesEditorHasCut ==
    (member = "live" /\ ~pending) => ev = cv

\* "Known saved" is never earned by a stale save receipt.
SettledImpliesDiskCurrent == settledV = cv => diskV = cv

\* Dropping is justified only by an ANSWERED refusal, never by silence.
DropRequiresDefinitiveRefusal == member = "offline" => mode = "refuses"

(* LIVENESS: the current canonical cut is always eventually known on disk, *)
(* with no operator action.                                                 *)
CanonicalEventuallySettled == []<>(settledV = cv)

(* REACH (asserted negated; MUST be violated).                              *)
NeverSettlesThroughLiveEditor == ~(settledV = MaxVersion /\ member = "live")
NeverDropsRefusedReplica == member = "live"
=============================================================================
