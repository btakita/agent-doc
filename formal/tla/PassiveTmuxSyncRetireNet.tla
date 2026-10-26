----------------------- MODULE PassiveTmuxSyncRetireNet -----------------------
EXTENDS Naturals, TLC

(***************************************************************************
The editor-surface RETIREMENT channel over the adversarial `NetChannel`
(`#tmuxautosyncretired`, `#surfaceretireformal`).

`PassiveTmuxSyncNet` proves editor focus -> tmux over a lossy channel, but it
has no retire/forget transition, so it could not see the v0.35.502/503 wedge:
the JetBrains plugin sent `editor_surface_forget` for its OWN live frame when a
delivery returned after a newer observation had been admitted, the controller
fenced that endpoint forever at the plugin's per-process connection generation
(`retired && generation <= current`), and every later tab switch was
`accepted=false` until a plugin reload or a controller recycle. The shipped
code took an action the model did not contain. This module adds it. The tmux
swap stays atomic and controller-local here (its transient failure is the
parent model's subject).

ONE FRAME ENDPOINT
------------------
One plugin process publishes one frame endpoint `(client, surface)` to one
controller. `cgen` is the plugin's connection generation for that client
(`CpRouteClient` seeds it from `System.nanoTime()` once per process); only a
plugin reload or a RE-REGISTRATION changes it. `seq` is the per-client
observation sequence, monotone across generations. The controller's
`ControllerEditorSurfaceRoot` for the endpoint is `rgen`/`rseq`/`rretired`.
`docAt[s]` is the payload of observation `s` (each `s` is assigned once, so it
stands for the document the message carries).

MESSAGES (all over the same adversarial channel)
------------------------------------------------
  Obs(g, s)       editor_surface_observe at generation g, sequence s
  Retire(g)       editor_surface_forget of the exact endpoint at generation g
  Reply(g, s, v)  the observation receipt: v = "ok" | "retired". "ok" is an
                  accepted observation or a stale fence (stale_generation /
                  stale_sequence: a duplicate or reordered copy of something
                  already applied). "retired" is the typed
                  `rejected=retired_endpoint` verdict, which carries
                  `reregister_above_generation`.

THE CONTROLLER FENCE, exactly as coded (`SurfaceObservationRejection::classify`)
  retired /\ g <= rgen      -> "retired"
  g < rgen                  -> "ok"      (stale_generation, nothing applied)
  g = rgen /\ s <= rseq     -> "ok"      (stale_sequence, nothing applied)
  otherwise                 -> accepted; g > rgen replaces the root (re-registration)

KNOBS (each fix has a wedge config; scripts/run_tla.sh asserts it MUST violate)
-------------------------------------------------------------------------------
  RetireOnNewerReply   THE SHIPPED BUG (TRUE = v0.35.502/503): a delivery whose
                       receipt returns after a newer observation was admitted
                       retires its own endpoint at the live generation.
                       Fixed (FALSE): only a frontend handoff or the end of the
                       frame retires. Wedge: PassiveTmuxSyncRetireNetWrongRetireWedge
                       (the 02:54:03Z trace).
  ReregisterOnRetired  THE RECOVERY: a live endpoint that receives the typed
                       retired rejection re-registers at a generation strictly
                       above the retired one and re-sends the same observation
                       (`CpRouteClient` + `EditorTabSyncListener.projectionLive`).
                       FALSE = the shipped plugin, which ignored the receipt.
                       Wedge: PassiveTmuxSyncRetireNetFallbackMuteWedge (the
                       historical Remote Dev sibling: the former private-API
                       backend fallback resumed a handoff-retired frame at the
                       same process generation). Marketplace builds now fail
                       closed without that fallback; the action remains a
                       regression model for the listener liveness transition.
                       With the recovery ON, even the shipped bug self-heals
                       (PassiveTmuxSyncRetireNetSelfHeal must PASS).
  RetireRaisesFence    `forget_surface` / `forget` / `forget_client_family` retire
                       a root at generation <= g and RAISE its fence to g. FALSE =
                       the shipped exact-generation forget, which ignored a forget
                       newer than the root; harmless while one process had one
                       generation, a resurrection once re-registration exists
                       (PassiveTmuxSyncRetireNetExactForgetWedge).
  RetiredFence         the controller's retired-endpoint fence itself. FALSE =
                       remove it; an observation in flight across a handoff then
                       drives tmux from a frame the editor declared ended
                       (PassiveTmuxSyncRetireNetNoFenceWedge). The recovery must
                       not be bought by weakening the fence.

ENVIRONMENT
-----------
  Handoff   the historical private-API backend fallback yields authority to an
            authenticated Remote Dev frontend (`retireControllerProjection`).
            Marketplace builds no longer call this transition. Bounded by MaxHandoffs.
  Fallback  regression-only inverse transition for the removed private-API
            backend fallback (`resumeControllerProjection` + `selectionChanged`).
  EndFrame  the frame closes, the project closes, or its root is superseded
            (`retireClosedSurfaceEndpoints`, `shutdown`): retired for good.
  FrontendSync  while the frame is not live, another publisher (the frontend)
            keeps tmux on the editor's focus. In code that publisher is the
            Remote Dev backend's forward of the JetBrains Client's measured
            columns (GH #320 option B), on its own `jetbrains-rd` endpoint and
            family, so it never moves this frame's fence.

CONFIGS (scripts/run_tla.sh)
----------------------------
  PassiveTmuxSyncRetireNet               fixed design, liveness + safety (pass)
  PassiveTmuxSyncRetireNetDuplicateLive  the same with MaxCopies = 2 (pass)
  PassiveTmuxSyncRetireNetSafety         reconnect, two handoffs (pass)
  PassiveTmuxSyncRetireNetDuplicateSafety  duplication, two handoffs (pass)
  PassiveTmuxSyncRetireNetSelfHeal       shipped bug + recovery (pass)
  ...WrongRetireWedge / ...FallbackMuteWedge / ...ExactForgetWedge /
  ...NoFenceWedge / ...Reach             MUST violate

CODE BINDING
------------
`formal/tla/PassiveTmuxSyncRetireNet.conformance.tsv` maps every code site that
mutates editor-surface endpoint state (derived from the sources, not listed by
hand) to the actions below; agent-doc-controller-io's
`surface_retire_conformance` checks it, transcribes `ControllerRecvObs` /
`ControllerRecvRetire` against the production graph, and replays the two real
wedge traces in `formal/traces/passive_tmux_sync_retire/`. The controller's
runtime monitor of `LiveEndpointNeverMuted` reports
`controller_editor_surface_muted_live_endpoint` after
`RETIRED_ENDPOINT_MUTE_THRESHOLD` (3) consecutive retired rejections.
***************************************************************************)

CONSTANTS MaxCopies, MaxGen, MaxSeq, MaxCGen, MaxHandoffs,
          RetireOnNewerReply, ReregisterOnRetired, RetireRaisesFence, RetiredFence

ASSUME MaxSeq >= 1 /\ MaxCGen >= 1

Docs == {"a", "b"}
Seqs == 1..MaxSeq
CGens == 1..MaxCGen
Verdicts == {"ok", "retired"}

VARIABLES
    net, gen, delivered,  \* NetChannel
    \* editor (plugin process, one frame endpoint)
    desired,       \* the document the operator focused
    seq,           \* sequence of the latest observation
    docAt,         \* payload of each sequence
    pendingIntent, \* still re-sending the latest observation
    live,          \* the frame is this listener's to publish (projectionLive)
    ended,         \* the frame is gone for good
    cgen,          \* connection generation (re-registration bumps it)
    handoffs,      \* environment: frontend handoffs taken
    offSwitches,   \* environment: tab switches while handed off (at most one)
    \* controller (`ControllerEditorSurfaceRoot` of the endpoint)
    rgen, rseq, rretired,
    \* tmux
    visible,
    \* history
    retiredAt,     \* highest generation the editor declared ended (processed Retire)
    acceptedSeq,   \* highest sequence the controller accepted
    resurrected    \* an observation at a declared-ended generation was accepted

C == INSTANCE NetChannel

editorVars == <<desired, seq, docAt, pendingIntent, live, ended, cgen, handoffs, offSwitches>>
ctlVars == <<rgen, rseq, rretired, visible, retiredAt, acceptedSeq, resurrected>>
protoVars == <<editorVars, ctlVars>>
vars == <<net, gen, delivered, protoVars>>

\* One record shape for every message.
Obs(g, s) == [t |-> "obs", g |-> g, s |-> s, v |-> "none"]
Retire(g) == [t |-> "retire", g |-> g, s |-> 0, v |-> "none"]
Reply(g, s, v) == [t |-> "reply", g |-> g, s |-> s, v |-> v]
Msgs ==
    {Obs(g, s) : g \in CGens, s \in Seqs}
        \cup {Retire(g) : g \in CGens}
        \cup {Reply(g, s, v) : g \in CGens, s \in Seqs, v \in Verdicts}

Other(d) == IF d = "a" THEN "b" ELSE "a"
Max(x, y) == IF x > y THEN x ELSE y

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ desired \in Docs
    /\ seq \in Seqs
    /\ docAt \in [Seqs -> Docs]
    /\ pendingIntent \in BOOLEAN
    /\ live \in BOOLEAN
    /\ ended \in BOOLEAN
    /\ cgen \in CGens
    /\ handoffs \in 0..MaxHandoffs
    /\ offSwitches \in 0..1
    /\ rgen \in CGens
    /\ rseq \in 0..MaxSeq
    /\ rretired \in BOOLEAN
    /\ visible \in Docs
    /\ retiredAt \in 0..MaxCGen
    /\ acceptedSeq \in Seqs
    /\ resurrected \in BOOLEAN

\* The frame is registered at generation 1 and tmux shows its focus.
Init ==
    /\ C!ChannelInit
    /\ desired = "a"
    /\ seq = 1
    /\ docAt = [s \in Seqs |-> "a"]
    /\ pendingIntent = FALSE
    /\ live = TRUE
    /\ ended = FALSE
    /\ cgen = 1
    /\ handoffs = 0
    /\ offSwitches = 0
    /\ rgen = 1
    /\ rseq = 1
    /\ rretired = FALSE
    /\ visible = "a"
    /\ retiredAt = 0
    /\ acceptedSeq = 1
    /\ resurrected = FALSE

---------------------------------------------------------------------------
(* ENVIRONMENT (no fairness).                                               *)

\* The operator switches tabs on the live frame: a new observation.
RefocusLive ==
    /\ live
    /\ seq < MaxSeq
    /\ desired' = Other(desired)
    /\ seq' = seq + 1
    /\ docAt' = [docAt EXCEPT ![seq + 1] = Other(desired)]
    /\ pendingIntent' = TRUE
    /\ C!Send(Obs(cgen, seq + 1))
    /\ UNCHANGED <<live, ended, cgen, handoffs, offSwitches>>
    /\ UNCHANGED ctlVars

\* The operator switches tabs while another publisher owns the surface.
RefocusHandedOff ==
    /\ ~live
    /\ offSwitches = 0
    /\ offSwitches' = 1
    /\ desired' = Other(desired)
    /\ UNCHANGED <<seq, docAt, pendingIntent, live, ended, cgen, handoffs>>
    /\ UNCHANGED ctlVars
    /\ UNCHANGED <<net, gen, delivered>>

\* Historical private-API fallback handoff (`retireControllerProjection`).
Handoff ==
    /\ live
    /\ ~ended
    /\ handoffs < MaxHandoffs
    /\ live' = FALSE
    /\ handoffs' = handoffs + 1
    /\ pendingIntent' = FALSE
    /\ C!Send(Retire(cgen))
    /\ UNCHANGED <<desired, seq, docAt, ended, cgen, offSwitches>>
    /\ UNCHANGED ctlVars

\* Regression-only removed fallback: resume the SAME frame at the SAME process
\* generation (`resumeControllerProjection` + `selectionChanged`).
Fallback ==
    /\ ~live
    /\ ~ended
    /\ seq < MaxSeq
    /\ live' = TRUE
    /\ seq' = seq + 1
    /\ docAt' = [docAt EXCEPT ![seq + 1] = desired]
    /\ pendingIntent' = TRUE
    /\ C!Send(Obs(cgen, seq + 1))
    /\ UNCHANGED <<desired, ended, cgen, handoffs, offSwitches>>
    /\ UNCHANGED ctlVars

\* The frame closes, or its project/root does (`retireClosedSurfaceEndpoints`,
\* `shutdown`, a superseded root). Never resumed.
EndFrame ==
    /\ live
    /\ ~ended
    /\ live' = FALSE
    /\ ended' = TRUE
    /\ pendingIntent' = FALSE
    /\ C!Send(Retire(cgen))
    /\ UNCHANGED <<desired, seq, docAt, cgen, handoffs, offSwitches>>
    /\ UNCHANGED ctlVars

AdversaryStep == C!Adversary /\ UNCHANGED protoVars

---------------------------------------------------------------------------
(* OTHER PUBLISHER (weakly fair): the frontend keeps tmux on the focus while *)
(* this frame is not live.                                                  *)
FrontendSync ==
    /\ ~live
    /\ visible # desired
    /\ visible' = desired
    /\ UNCHANGED editorVars
    /\ UNCHANGED <<rgen, rseq, rretired, retiredAt, acceptedSeq, resurrected>>
    /\ UNCHANGED <<net, gen, delivered>>

---------------------------------------------------------------------------
(* EDITOR (weakly fair): re-send the latest observation while it is kept.  *)
ResendFocus ==
    /\ live
    /\ pendingIntent
    /\ C!Send(Obs(cgen, seq))
    /\ UNCHANGED protoVars

\* A typed `retired_endpoint` rejection reached a live frame at its current
\* generation: re-register strictly above the retired generation and re-send
\* the same observation (one round trip).
ReregisterEnabled(g, v) == ReregisterOnRetired /\ v = "retired" /\ live /\ g = cgen

EditorReregister(g, s, v) ==
    /\ ReregisterEnabled(g, v)
    /\ cgen' = cgen + 1
    /\ C!DeliverAndSend(Reply(g, s, v), Obs(cgen + 1, seq))
    /\ UNCHANGED <<desired, seq, docAt, pendingIntent, live, ended, handoffs, offSwitches>>
    /\ UNCHANGED ctlVars

\* THE SHIPPED BUG: a delivery whose receipt returns (any verdict: the shipped
\* check was the transport exit code) after a newer observation was admitted
\* retires its own endpoint at the live generation.
WrongRetireEnabled(g, s, v) ==
    RetireOnNewerReply /\ live /\ g = cgen /\ s < seq

EditorRetireSuperseded(g, s, v) ==
    /\ WrongRetireEnabled(g, s, v)
    /\ C!DeliverAndSend(Reply(g, s, v), Retire(cgen))
    /\ UNCHANGED protoVars

\* Every other receipt. The latest observation is done when its receipt at
\* the current generation is "ok" (accepted, or a sequence fence meaning it
\* already was). The shipped plugin read a retired receipt as done too.
EditorAck(g, s, v) ==
    /\ ~ReregisterEnabled(g, v)
    /\ ~WrongRetireEnabled(g, s, v)
    /\ C!Deliver(Reply(g, s, v))
    /\ pendingIntent' =
        IF s = seq /\ g = cgen /\ (v = "ok" \/ ~ReregisterOnRetired)
            THEN FALSE
            ELSE pendingIntent
    /\ UNCHANGED <<desired, seq, docAt, live, ended, cgen, handoffs, offSwitches>>
    /\ UNCHANGED ctlVars

EditorRecvReply(g, s, v) ==
    /\ Reply(g, s, v) \in C!InFlight
    /\ \/ EditorReregister(g, s, v)
       \/ EditorRetireSuperseded(g, s, v)
       \/ EditorAck(g, s, v)

---------------------------------------------------------------------------
(* CONTROLLER: `ControllerEditorSurfaceGraph`.                              *)

\* `observe_classified`.
ControllerRecvObs(g, s) ==
    /\ Obs(g, s) \in C!InFlight
    /\ IF RetiredFence /\ rretired /\ g <= rgen
          THEN /\ C!DeliverAndSend(Obs(g, s), Reply(g, s, "retired"))
               /\ UNCHANGED ctlVars
       ELSE IF g < rgen \/ (g = rgen /\ s <= rseq)
          THEN /\ C!DeliverAndSend(Obs(g, s), Reply(g, s, "ok"))
               /\ UNCHANGED ctlVars
       ELSE /\ rgen' = g
            /\ rseq' = s
            /\ rretired' = IF g > rgen THEN FALSE ELSE rretired
            /\ visible' = docAt[s]
            /\ acceptedSeq' = Max(acceptedSeq, s)
            /\ resurrected' = (resurrected \/ g <= retiredAt)
            /\ C!DeliverAndSend(Obs(g, s), Reply(g, s, "ok"))
            /\ UNCHANGED retiredAt
    /\ UNCHANGED editorVars

\* `forget_surface` (and `forget` / `forget_client_family` for the same root).
ControllerRecvRetire(g) ==
    /\ Retire(g) \in C!InFlight
    /\ C!Deliver(Retire(g))
    /\ retiredAt' = Max(retiredAt, g)
    /\ IF RetireRaisesFence
          THEN IF rgen <= g
                  THEN rgen' = g /\ rretired' = TRUE
                  ELSE UNCHANGED <<rgen, rretired>>
          ELSE IF rgen = g
                  THEN rretired' = TRUE /\ UNCHANGED rgen
                  ELSE UNCHANGED <<rgen, rretired>>
    /\ UNCHANGED <<rseq, visible, acceptedSeq, resurrected>>
    /\ UNCHANGED editorVars

Recv ==
    \/ \E g \in CGens, s \in Seqs : ControllerRecvObs(g, s)
    \/ \E g \in CGens : ControllerRecvRetire(g)
    \/ \E g \in CGens, s \in Seqs, v \in Verdicts : EditorRecvReply(g, s, v)

Next ==
    \/ RefocusLive \/ RefocusHandedOff \/ Handoff \/ Fallback \/ EndFrame
    \/ AdversaryStep
    \/ FrontendSync
    \/ ResendFocus
    \/ Recv

\* FAIRNESS. `C!FairLossy(Msgs)` is one strong-fairness conjunct PER MESSAGE,
\* and TLC's liveness tableau is exponential in the number of fairness
\* conjuncts (it reported this model's ~40 as a "tautology" with 0 branches).
\* So the channel assumption is stated for the two messages progress needs:
\* the editor's CURRENT observation and a receipt for it. That is weaker than
\* `C!FairLossy` (every behaviour FairLossy admits is admitted here, and the
\* adversary may starve every other message, including every Retire, forever),
\* so the liveness proof is stronger, not weaker.
CurrentObsDelivered == C!Delivered(Obs(cgen, seq), Msgs)
CurrentReplyDelivered == \E v \in Verdicts : C!Delivered(Reply(cgen, seq, v), Msgs)

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ResendFocus)
    /\ WF_vars(FrontendSync)
    /\ SF_<<net, gen, delivered>>(CurrentObsDelivered)
    /\ SF_<<net, gen, delivered>>(CurrentReplyDelivered)

---------------------------------------------------------------------------
(* SAFETY (no fairness, full adversary).                                    *)

\* Retirement stays a correct fence: once the editor declared a generation of
\* this frame ended, nothing from that generation drives tmux again. The
\* recovery re-registers ABOVE it instead of lifting the fence.
NoRetiredResurrection == ~resurrected

\* A re-registration always lands strictly above the controller's fence.
ControllerGenerationNeverAheadOfEditor == rgen <= cgen

(* LIVENESS.                                                                *)

\* A live endpoint that keeps refocusing is eventually accepted.
LiveEndpointNeverMuted == (live /\ pendingIntent) ~> (~live \/ acceptedSeq >= seq)

\* The parent model's property, kept alongside.
VisibleEventuallyMatchesEditor == <>[](visible = desired)

(* REACH (asserted negated; MUST be violated).                              *)
NeverReregisters == cgen = 1
=============================================================================
