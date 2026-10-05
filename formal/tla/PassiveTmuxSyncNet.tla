--------------------------- MODULE PassiveTmuxSyncNet ---------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Editor focus -> tmux visible pane over the adversarial `NetChannel`
(`#netadv3`, audit P13, F14, F15). `PassiveTmuxSync` proves the
controller-local swap itself (the actor-binding proof, the unique
visible/stashed partition, no autostart) with the request delivered exactly
once. This module keeps that swap atomic and controller-local, and makes the
editor -> controller focus observation a message that may be delayed,
reordered, dropped, duplicated, or cut by a reconnect. The tmux effect itself
may fail transiently (`select-pane` against a busy server).

THE PROTOCOL (`agent-doc-editor-surface` `SurfaceTracking::advance`,
`rpc.rs` surface observe, `JB/CpRouteClient.kt`)
---------------------------------------------------------------
The editor sends `Focus(doc, seq)` stamped with a per-client sequence. The
controller folds it into `SurfaceTracking`: an observation whose document
equals `focused_document` is `Idle`; otherwise the intent is `Focus` and the
controller runs the swap. The reply tells the editor whether to keep the
intent.

KNOBS (one wedge each; scripts/run_tla.sh asserts every wedge MUST violate)
---------------------------------------------------------------------------
  AdvanceOnEffect  F14/F15 FIX: `focused_document` advances only when the swap
                   took effect, and a failed swap answers "retry" so the
                   editor keeps re-sending the intent. FALSE = the shipped
                   graph advances BEFORE the effect and answers Ok either way,
                   so one transient tmux failure leaves the wrong pane visible
                   and every re-send of the same focus folds to `Idle`
                   (PassiveTmuxSyncNetAdvanceFirstWedge).
  SeqFence         the controller discards an observation older than the
                   latest one it applied (`(client_id, generation, sequence)`,
                   already in code). FALSE = a reordered older focus is applied
                   after a newer one (PassiveTmuxSyncNetReorderWedge).
***************************************************************************)

CONSTANTS MaxCopies, MaxGen, MaxFails, AdvanceOnEffect, SeqFence

Docs == {"a", "b"}
Seqs == 1..2

VARIABLES
    net, gen, delivered,  \* NetChannel (editor -> controller and back)
    desired,      \* editor: the focused document
    seq,          \* editor: sequence of the current focus observation
    pendingIntent,\* editor: still re-sending the current observation
    tracked,      \* controller: SurfaceTracking.focused_document
    lastSeq,      \* controller: latest sequence applied
    visible,      \* tmux: document in the visible pane
    stashed,      \* tmux: document in the stash window
    failsLeft,    \* environment: transient tmux failures still allowed
    staleApplied  \* history: an older observation was applied after a newer

C == INSTANCE NetChannel

protoVars == <<desired, seq, pendingIntent, tracked, lastSeq, visible, stashed,
               failsLeft, staleApplied>>
vars == <<net, gen, delivered, protoVars>>

\* One record shape for every message.
F(d, s) == [t |-> "focus", doc |-> d, seq |-> s, ok |-> TRUE]
Reply(s, ok) == [t |-> "reply", doc |-> "a", seq |-> s, ok |-> ok]
Msgs == {F(d, s) : d \in Docs, s \in Seqs} \cup {Reply(s, ok) : s \in Seqs, ok \in BOOLEAN}

Other(d) == IF d = "a" THEN "b" ELSE "a"

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ desired \in Docs
    /\ seq \in Seqs
    /\ pendingIntent \in BOOLEAN
    /\ tracked \in Docs
    /\ lastSeq \in 0..2
    /\ visible \in Docs
    /\ stashed \in Docs
    /\ failsLeft \in 0..MaxFails
    /\ staleApplied \in BOOLEAN

\* The editor just focused "b"; tmux still shows "a".
Init ==
    /\ net = (F("b", 1) :> 1)
    /\ gen = 0
    /\ delivered = {}
    /\ desired = "b"
    /\ seq = 1
    /\ pendingIntent = TRUE
    /\ tracked = "a"
    /\ lastSeq = 0
    /\ visible = "a"
    /\ stashed = "b"
    /\ failsLeft = MaxFails
    /\ staleApplied = FALSE

---------------------------------------------------------------------------
(* ENVIRONMENT (no fairness).                                               *)

\* The operator focuses the other tab once.
Refocus ==
    /\ seq < 2
    /\ desired' = Other(desired)
    /\ seq' = seq + 1
    /\ pendingIntent' = TRUE
    /\ C!Send(F(Other(desired), seq + 1))
    /\ UNCHANGED <<tracked, lastSeq, visible, stashed, failsLeft, staleApplied>>

AdversaryStep == C!Adversary /\ UNCHANGED protoVars

---------------------------------------------------------------------------
(* EDITOR (weakly fair): re-send the current observation while it is kept. *)
ResendFocus ==
    /\ pendingIntent
    /\ C!Send(F(desired, seq))
    /\ UNCHANGED protoVars

EditorRecvReply(s, ok) ==
    /\ Reply(s, ok) \in C!InFlight
    /\ C!Deliver(Reply(s, ok))
    /\ pendingIntent' = IF s = seq /\ ok THEN FALSE ELSE pendingIntent
    /\ UNCHANGED <<desired, seq, tracked, lastSeq, visible, stashed, failsLeft,
                   staleApplied>>

---------------------------------------------------------------------------
(* CONTROLLER: fold the observation, run the swap, answer.                 *)
ControllerRecvFocus(d, s) ==
    /\ F(d, s) \in C!InFlight
    /\ IF SeqFence /\ s < lastSeq
          THEN \* Older than what was applied: discard, but still answer it
               \* so a stale re-send cannot keep an editor intent alive.
               /\ C!DeliverAndSend(F(d, s), Reply(s, TRUE))
               /\ UNCHANGED <<tracked, lastSeq, visible, stashed, failsLeft,
                              staleApplied>>
          ELSE
            /\ lastSeq' = IF s > lastSeq THEN s ELSE lastSeq
            /\ staleApplied' = (staleApplied \/ s < lastSeq)
            /\ IF tracked = d
                  THEN \* Idle: nothing observable changed.
                       /\ C!DeliverAndSend(F(d, s), Reply(s, TRUE))
                       /\ UNCHANGED <<tracked, visible, stashed, failsLeft>>
                  ELSE \/ \* The swap takes effect (atomic, controller-local).
                          /\ visible' = d
                          /\ stashed' = Other(d)
                          /\ tracked' = d
                          /\ C!DeliverAndSend(F(d, s), Reply(s, TRUE))
                          /\ UNCHANGED failsLeft
                       \/ \* tmux refused transiently: nothing moved.
                          /\ failsLeft > 0
                          /\ failsLeft' = failsLeft - 1
                          /\ tracked' = IF AdvanceOnEffect THEN tracked ELSE d
                          /\ C!DeliverAndSend(F(d, s), Reply(s, ~AdvanceOnEffect))
                          /\ UNCHANGED <<visible, stashed>>
    /\ UNCHANGED <<desired, seq, pendingIntent>>

Recv ==
    \/ \E d \in Docs, s \in Seqs : ControllerRecvFocus(d, s)
    \/ \E s \in Seqs, ok \in BOOLEAN : EditorRecvReply(s, ok)

Next == Refocus \/ AdversaryStep \/ ResendFocus \/ Recv

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ResendFocus)
    /\ C!FairLossy(Msgs)

---------------------------------------------------------------------------
(* SAFETY (no fairness, full adversary).                                    *)
VisibleAndStashedRemainAUniquePartition ==
    visible # stashed /\ {visible, stashed} = Docs
NoStaleFocusApplied == ~staleApplied

(* LIVENESS: the editor's focus eventually shows in tmux and stays.         *)
VisibleEventuallyMatchesEditor == <>[](visible = desired)

(* REACH (asserted negated; MUST be violated).                              *)
NeverSwaps == visible = "a"
=============================================================================
