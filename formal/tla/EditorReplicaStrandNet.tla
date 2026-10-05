------------------------ MODULE EditorReplicaStrandNet ------------------------
EXTENDS Naturals, TLC

(***************************************************************************
`EditorReplicaStrand` re-checked over the adversarial `NetChannel`
(`#netadv3`). The atomic module reads each re-registration request and its
answer as ONE step over the endpoint's `mode`. Here the request and the answer
are messages: either may be delayed, reordered, dropped, duplicated, or
stranded across a reconnect.

THE PROTOCOL (`reobserve_missing_editor_replica_with_reregistration`,
agent-doc-document-realtime-io)
------------------------------------------------------------------
A resolve that finds the replica missing spends a budget of
`MaxReregisterAttempts` synchronous attempts. Each attempt sends `Req` and
waits for its receipt:

  serves  -> the editor re-registers (`Registered`): the replica is back,
             or answers `Deferred` (`{"status":"deferred"}`, #netadv5 R2): a
             slow attach or a coalesced re-register, still trying
  refuses -> `Refuse`  (a rejected receipt: definitive)
  accepts -> `Accept`  (answered YES; the replica never appears)
  silent  -> nothing

A lost request or receipt ends the attempt unanswered (`GiveUp`; the IPC
receipt wait expired). `GiveUp` frees the loop only: it is enabled only once
nothing for the attempt is still in flight, which is the liveness-side
abstraction of "the receipt wait is long enough". No safety argument uses it.

When the budget is spent the exhaustion is latched against the liveness
witness, which never changes for a stuck endpoint.

KNOBS (one wedge each; scripts/run_tla.sh asserts every wedge MUST violate)
---------------------------------------------------------------------------
  RearmUnanswered  ERS-1 FIX: a budget in which NOTHING answered proves nothing
                   about the endpoint, so it is not latched; a later resolve
                   spends a fresh budget (with backoff in code). FALSE = the
                   shipped latch: one budget whose receipts were all lost
                   pauses self-heal forever at an unchanged witness, so an
                   endpoint that DOES answer is never demoted
                   (EditorReplicaStrandNetLatchWedge).
  GiveUpIsRefusal  the timeout-as-verdict class (R2/R3, fixed in code by
                   #netadv5): an unanswered attempt, or a `deferred` receipt,
                   counted as a refusal demotes a SERVING endpoint whose
                   receipts were merely lost or slow
                   (EditorReplicaStrandNetGiveUpRefusalWedge).

Safety holds under `[][Next]_vars` alone. Liveness holds under `FairLossy`
plus weak fairness on agent-doc's own steps.
***************************************************************************)

CONSTANTS MaxReregisterAttempts, MaxCopies, MaxGen, RearmUnanswered, GiveUpIsRefusal

Modes == {"serves", "refuses", "accepts", "silent"}
Answering == {"refuses", "accepts"}

VARIABLES
    net, gen, delivered,   \* NetChannel
    reloaded,      \* the one cdylib generation swap happened
    replica,       \* "present" | "missing"
    mode,          \* endpoint behaviour toward THIS document after the swap
    registration,  \* "attached" | "detached" - the attachment latch
    attempts,      \* attempts spent in the current budget
    waiting,       \* an attempt is outstanding
    refusals,      \* answered refusals received (capped)
    accepted,      \* answered acceptances received (capped)
    latched,       \* self-heal exhausted at the (unchanged) liveness witness
    corroborated,  \* a second resolve saw the accepted-unserved latch
    authority,     \* "none" | "editor" | "disk"
    attaching      \* editor: a deferred (slow) attach is still in progress

C == INSTANCE NetChannel

protoVars == <<reloaded, replica, mode, registration, attempts, waiting,
               refusals, accepted, latched, corroborated, authority, attaching>>
vars == <<net, gen, delivered, protoVars>>

Msgs == {"req", "refuse", "accept", "registered", "deferred"}

Cap(x) == IF x >= 2 THEN 2 ELSE x + 1

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ reloaded \in BOOLEAN
    /\ replica \in {"present", "missing"}
    /\ mode \in Modes
    /\ registration \in {"attached", "detached"}
    /\ attempts \in 0..MaxReregisterAttempts
    /\ waiting \in BOOLEAN
    /\ refusals \in 0..2
    /\ accepted \in 0..2
    /\ latched \in BOOLEAN
    /\ corroborated \in BOOLEAN
    /\ authority \in {"none", "editor", "disk"}
    /\ attaching \in BOOLEAN

Init ==
    /\ C!ChannelInit
    /\ reloaded = FALSE
    /\ replica = "present"
    /\ mode = "serves"
    /\ registration = "attached"
    /\ attempts = 0
    /\ waiting = FALSE
    /\ refusals = 0
    /\ accepted = 0
    /\ latched = FALSE
    /\ corroborated = FALSE
    /\ authority = "editor"
    /\ attaching = FALSE

---------------------------------------------------------------------------
(* ENVIRONMENT (no fairness). The swap may land in ANY mode, including a   *)
(* new generation that serves: the safety invariants must hold for a       *)
(* serving endpoint whose receipts are lost.                               *)
LibraryReload ==
    /\ ~reloaded
    /\ reloaded' = TRUE
    /\ replica' = "missing"
    /\ mode' \in Modes
    /\ authority' = "none"
    /\ UNCHANGED <<net, gen, delivered, registration, attempts, waiting,
                   refusals, accepted, latched, corroborated, attaching>>

AdversaryStep == C!Adversary /\ UNCHANGED protoVars

---------------------------------------------------------------------------
(* AGENT-DOC (weakly fair).                                                *)

SendAttempt ==
    /\ replica = "missing"
    /\ registration = "attached"
    /\ ~latched
    /\ ~waiting
    /\ attempts < MaxReregisterAttempts
    /\ attempts' = attempts + 1
    /\ waiting' = TRUE
    /\ C!Send("req")
    /\ UNCHANGED <<reloaded, replica, mode, registration, refusals, accepted,
                   latched, corroborated, authority, attaching>>

\* The receipt wait expired with nothing left in flight for the attempt.
GiveUp ==
    /\ waiting
    /\ C!InFlight \cap Msgs = {}
    /\ waiting' = FALSE
    /\ refusals' = IF GiveUpIsRefusal THEN Cap(refusals) ELSE refusals
    /\ UNCHANGED <<net, gen, delivered, reloaded, replica, mode, registration,
                   attempts, accepted, latched, corroborated, authority, attaching>>

\* The budget is spent. An answered budget is latched (the shipped memo);
\* an UNANSWERED one is re-armed for a later resolve under the fix.
Exhaust ==
    /\ replica = "missing"
    /\ ~latched
    /\ ~waiting
    /\ attempts = MaxReregisterAttempts
    /\ IF RearmUnanswered /\ refusals = 0 /\ accepted = 0
          THEN /\ attempts' = 0
               /\ UNCHANGED latched
          ELSE /\ latched' = TRUE
               /\ UNCHANGED attempts
    /\ UNCHANGED <<net, gen, delivered, reloaded, replica, mode, registration,
                   waiting, refusals, accepted, corroborated, authority, attaching>>

\* The next resolve at the same witness corroborates accepted-unserved.
RecordUnservedObservation ==
    /\ latched
    /\ replica = "missing"
    /\ accepted > 0
    /\ ~corroborated
    /\ corroborated' = TRUE
    /\ UNCHANGED <<net, gen, delivered, reloaded, replica, mode, registration,
                   attempts, waiting, refusals, accepted, latched, authority, attaching>>

DemoteOnRejection ==
    /\ registration = "attached"
    /\ replica = "missing"
    /\ latched
    /\ refusals > 0
    /\ registration' = "detached"
    /\ UNCHANGED <<net, gen, delivered, reloaded, replica, mode, attempts,
                   waiting, refusals, accepted, latched, corroborated, authority, attaching>>

DemoteOnAcceptanceWithoutService ==
    /\ registration = "attached"
    /\ replica = "missing"
    /\ accepted > 0
    /\ corroborated
    /\ registration' = "detached"
    /\ UNCHANGED <<net, gen, delivered, reloaded, replica, mode, attempts,
                   waiting, refusals, accepted, latched, corroborated, authority, attaching>>

ResolveOnEditor ==
    /\ replica = "present"
    /\ authority # "editor"
    /\ authority' = "editor"
    /\ UNCHANGED <<net, gen, delivered, reloaded, replica, mode, registration,
                   attempts, waiting, refusals, accepted, latched, corroborated, attaching>>

DescendToDisk ==
    /\ replica = "missing"
    /\ registration = "detached"
    /\ authority # "disk"
    /\ authority' = "disk"
    /\ UNCHANGED <<net, gen, delivered, reloaded, replica, mode, registration,
                   attempts, waiting, refusals, accepted, latched, corroborated, attaching>>

---------------------------------------------------------------------------
(* RECEIVERS: each takes its message off the wire whenever it is in flight. *)

EditorRecvReq ==
    /\ "req" \in C!InFlight
    /\ CASE mode = "serves"  -> \/ C!DeliverAndSend("req", "registered")
                                   /\ UNCHANGED attaching
                                \/ C!DeliverAndSend("req", "deferred")
                                   /\ attaching' = TRUE
         [] mode = "refuses" -> C!DeliverAndSend("req", "refuse") /\ UNCHANGED attaching
         [] mode = "accepts" -> C!DeliverAndSend("req", "accept") /\ UNCHANGED attaching
         [] OTHER            -> C!Deliver("req") /\ UNCHANGED attaching
    /\ UNCHANGED <<reloaded, replica, mode, registration, attempts, waiting,
                   refusals, accepted, latched, corroborated, authority, attaching>>

\* The slow attach a `deferred` receipt announced lands (editor-local, fair).
AttachLands ==
    /\ attaching
    /\ attaching' = FALSE
    /\ C!Send("registered")
    /\ UNCHANGED <<reloaded, replica, mode, registration, attempts, waiting,
                   refusals, accepted, latched, corroborated, authority, attaching>>

\* An answer counts only for the attempt that is waiting for it (the receipt
\* is synchronous on its connection); a straggler is discarded.
AgentRecvRefuse ==
    /\ "refuse" \in C!InFlight
    /\ C!Deliver("refuse")
    /\ IF waiting
          THEN /\ refusals' = Cap(refusals)
               /\ waiting' = FALSE
          ELSE UNCHANGED <<refusals, waiting, attaching>>
    /\ UNCHANGED <<reloaded, replica, mode, registration, attempts, accepted,
                   latched, corroborated, authority, attaching>>

\* `deferred`: an answer, but not a verdict. It ends the attempt like a lost
\* receipt does, so the budget logic treats it as unanswered.
AgentRecvDeferred ==
    /\ "deferred" \in C!InFlight
    /\ C!Deliver("deferred")
    /\ IF waiting
          THEN /\ waiting' = FALSE
               /\ refusals' = IF GiveUpIsRefusal THEN Cap(refusals) ELSE refusals
          ELSE UNCHANGED <<refusals, waiting, attaching>>
    /\ UNCHANGED <<reloaded, replica, mode, registration, attempts, accepted,
                   latched, corroborated, authority, attaching>>

AgentRecvAccept ==
    /\ "accept" \in C!InFlight
    /\ C!Deliver("accept")
    /\ IF waiting
          THEN /\ accepted' = Cap(accepted)
               /\ waiting' = FALSE
          ELSE UNCHANGED <<accepted, waiting, attaching>>
    /\ UNCHANGED <<reloaded, replica, mode, registration, attempts, refusals,
                   latched, corroborated, authority, attaching>>

\* A registration that lands is level state: it heals regardless of which
\* attempt asked, and clears every recovery memo.
AgentRecvRegistered ==
    /\ "registered" \in C!InFlight
    /\ C!Deliver("registered")
    /\ IF registration = "attached"
          THEN /\ replica' = "present"
               /\ attempts' = 0
               /\ waiting' = FALSE
               /\ refusals' = 0
               /\ accepted' = 0
               /\ latched' = FALSE
               /\ corroborated' = FALSE
          ELSE UNCHANGED <<replica, attempts, waiting, refusals, accepted,
                           latched, corroborated, attaching>>
    /\ UNCHANGED <<reloaded, mode, registration, authority, attaching>>

Recv == EditorRecvReq \/ AttachLands \/ AgentRecvRefuse \/ AgentRecvAccept \/ AgentRecvDeferred
        \/ AgentRecvRegistered

Next ==
    \/ LibraryReload
    \/ AdversaryStep
    \/ SendAttempt
    \/ GiveUp
    \/ Exhaust
    \/ RecordUnservedObservation
    \/ DemoteOnRejection
    \/ DemoteOnAcceptanceWithoutService
    \/ ResolveOnEditor
    \/ DescendToDisk
    \/ Recv

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(SendAttempt)
    /\ WF_vars(GiveUp)
    /\ WF_vars(Exhaust)
    /\ WF_vars(RecordUnservedObservation)
    /\ WF_vars(DemoteOnRejection)
    /\ WF_vars(DemoteOnAcceptanceWithoutService)
    /\ WF_vars(ResolveOnEditor)
    /\ WF_vars(DescendToDisk)
    /\ WF_vars(AttachLands)
    /\ C!FairLossy(Msgs)

---------------------------------------------------------------------------
(* SAFETY (no fairness, full adversary).                                   *)
NeverReadsDiskWhileEndpointServes == authority = "disk" => mode # "serves"
DemotionRequiresProof == registration = "detached" => mode # "serves"
DemotionRequiresAnAnswer ==
    registration = "detached" => (refusals > 0 \/ accepted > 0)
EditorReplicaOutranksDisk == authority = "disk" => replica = "missing"

(* LIVENESS (FairLossy + agent-doc's own steps).                           *)
\* An endpoint that WOULD answer is eventually demoted and disk resolves,
\* whatever the network did to its earlier receipts.
AnsweringEndpointResolves ==
    (replica = "missing" /\ mode \in Answering) ~> (authority = "disk")
\* A serving endpoint is never demoted and eventually re-attaches.
ServingEndpointReattaches ==
    (replica = "missing" /\ mode = "serves") ~> (replica = "present")

(* REACH (asserted negated; MUST be violated).                             *)
NeverDescendsToDisk == authority # "disk"
=============================================================================
