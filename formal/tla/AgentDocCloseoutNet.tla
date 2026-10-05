-------------------------- MODULE AgentDocCloseoutNet --------------------------
EXTENDS Naturals, TLC

(***************************************************************************
The closeout OWNER LEASE over the adversarial `NetChannel` (`#netadv3`,
audit R4). `AgentDocCloseout` decrees its delivery ACK after a bounded number
of failures and never models the lease that keeps two closeouts of one cycle
apart. The lease is a controller CAS (`decide_owner_claim`) reached by RPC from
the foreground closeout (`agent-doc-write-runtime-io`), refreshed by a
heartbeat thread, and expired by the clock. Every request and reply here may
be delayed, reordered, dropped, duplicated, or cut by a reconnect, and the
clock is an arbitrary `Expire` event: no timeout value appears in any safety
argument.

ACTORS
------
  A       the foreground closeout under test (claim, heartbeat, commit)
  B       any competing closeout of the same cycle (a captured-finalize
          resume, another CLI); its own RPCs are atomic here because only A's
          protocol is under test
  server  the controller's lease CAS; same `owner_id` re-claims idempotently

KNOBS (one wedge each; scripts/run_tla.sh asserts every wedge MUST violate)
---------------------------------------------------------------------------
  StableOwnerId     F4 FIX: the owner id is stable for one closeout owner
                    (process + thread + role), and a claim whose reply was lost
                    is re-sent with it (the CAS is idempotent for the same id).
                    FALSE = the shipped one-shot claim with an id minted per
                    call: the closeout fails, and the next attempt in the same
                    live process mints a fresh id and is refused
                    `HeldByOther` by its own orphaned lease. The orphan can
                    also come from a retried claim that the server applies
                    AFTER answering a later copy, so retrying alone is not the
                    fix: the id must be stable across attempts.
  HeartbeatRetries  F5 FIX (a): a refresh whose reply was lost is retried.
                    FALSE = the shipped heartbeat `break`s after one failed
                    refresh while the foreground keeps working.
  AbortOnLeaseLoss  F5 FIX (b): a refresh ANSWERED with "not the owner" stops
                    the foreground before it commits. FALSE = the foreground
                    never learns and commits anyway.
  CommitFenced      NOT IN CODE (remaining item ADC-fence): the commit carries
                    the owner id and the server refuses it unless that id
                    still holds the lease. Without it no heartbeat can keep
                    two closeouts apart, because a lease can always expire
                    between the last refresh the owner saw and its commit.
                    `AgentDocCloseoutNetFenceWedge` (FALSE) MUST violate
                    `AtMostOneCommit`; `AgentDocCloseoutNetFencedTarget` (TRUE)
                    must pass it.
***************************************************************************)

CONSTANTS MaxCopies, MaxGen, StableOwnerId, HeartbeatRetries, AbortOnLeaseLoss,
          CommitFenced

Ids == {"a1", "a2"}

VARIABLES
    net, gen, delivered,   \* NetChannel (A <-> server)
    owner,         \* server: "none" | "a1" | "a2" | "b"
    aPhase,        \* "claim" | "work" | "done" | "failed" | "aborted"
    aId,           \* A's current owner id
    rerun,         \* the pre-fix second attempt (fresh id) was taken
    claimOut,      \* a claim is outstanding
    refreshOut,    \* a refresh is outstanding
    hbAlive,       \* the heartbeat thread is running
    lostLease,     \* A received an answered "not the owner"
    bPhase,        \* "idle" | "work" | "done"
    commits,       \* commits of this cycle (capped at 2)
    selfConflict,  \* history: A was refused by its own orphaned lease
    commitAfterLoss \* history: A committed after an answered lease loss

C == INSTANCE NetChannel

protoVars == <<owner, aPhase, aId, rerun, claimOut, refreshOut, hbAlive,
               lostLease, bPhase, commits, selfConflict, commitAfterLoss>>
vars == <<net, gen, delivered, protoVars>>

Claim(id) == [t |-> "claim", id |-> id, holder |-> "none"]
Acq(id) == [t |-> "acq", id |-> id, holder |-> id]
Held(id, h) == [t |-> "held", id |-> id, holder |-> h]
Superseded(id) == [t |-> "superseded", id |-> id, holder |-> "none"]
Refresh(id) == [t |-> "refresh", id |-> id, holder |-> "none"]
RefOk(id) == [t |-> "refok", id |-> id, holder |-> id]
RefLost(id) == [t |-> "reflost", id |-> id, holder |-> "none"]
Holders == {"a1", "a2", "b"}
Msgs == {Claim(i) : i \in Ids} \cup {Acq(i) : i \in Ids}
        \cup {Held(i, h) : i \in Ids, h \in Holders}
        \cup {Superseded(i) : i \in Ids}
        \cup {Refresh(i) : i \in Ids} \cup {RefOk(i) : i \in Ids}
        \cup {RefLost(i) : i \in Ids}
ClaimMsgs(id) == {Claim(id), Acq(id), Superseded(id)} \cup {Held(id, h) : h \in Holders}
RefreshMsgs(id) == {Refresh(id), RefOk(id), RefLost(id)}

Bump(x) == IF x >= 2 THEN 2 ELSE x + 1

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ owner \in {"none"} \cup Holders
    /\ aPhase \in {"claim", "work", "done", "failed", "aborted"}
    /\ aId \in Ids
    /\ rerun \in BOOLEAN
    /\ claimOut \in BOOLEAN
    /\ refreshOut \in BOOLEAN
    /\ hbAlive \in BOOLEAN
    /\ lostLease \in BOOLEAN
    /\ bPhase \in {"idle", "work", "done"}
    /\ commits \in 0..2
    /\ selfConflict \in BOOLEAN
    /\ commitAfterLoss \in BOOLEAN

Init ==
    /\ C!ChannelInit
    /\ owner = "none"
    /\ aPhase = "claim"
    /\ aId = "a1"
    /\ rerun = FALSE
    /\ claimOut = FALSE
    /\ refreshOut = FALSE
    /\ hbAlive = FALSE
    /\ lostLease = FALSE
    /\ bPhase = "idle"
    /\ commits = 0
    /\ selfConflict = FALSE
    /\ commitAfterLoss = FALSE

---------------------------------------------------------------------------
(* ENVIRONMENT (no fairness).                                               *)

\* The lease runs out. No clock: it may happen between ANY two steps.
Expire ==
    /\ owner # "none"
    /\ owner' = "none"
    /\ UNCHANGED <<net, gen, delivered, aPhase, aId, rerun, claimOut,
                   refreshOut, hbAlive, lostLease, bPhase, commits,
                   selfConflict, commitAfterLoss>>

\* A competing closeout of the same, still open, cycle claims a free lease.
BClaim ==
    /\ bPhase = "idle"
    /\ owner = "none"
    /\ commits = 0
    /\ owner' = "b"
    /\ bPhase' = "work"
    /\ UNCHANGED <<net, gen, delivered, aPhase, aId, rerun, claimOut,
                   refreshOut, hbAlive, lostLease, commits, selfConflict,
                   commitAfterLoss>>

BCommit ==
    /\ bPhase = "work"
    /\ CommitFenced => owner = "b"
    /\ bPhase' = "done"
    /\ commits' = Bump(commits)
    /\ owner' = IF owner = "b" THEN "none" ELSE owner
    /\ UNCHANGED <<net, gen, delivered, aPhase, aId, rerun, claimOut,
                   refreshOut, hbAlive, lostLease, selfConflict, commitAfterLoss>>

\* The failed closeout is re-run by the same live process: with a freshly
\* minted owner id pre-fix, with the same stable id under the fix.
Rerun ==
    /\ aPhase = "failed"
    /\ ~rerun
    /\ rerun' = TRUE
    /\ aPhase' = "claim"
    /\ aId' = IF StableOwnerId THEN aId ELSE "a2"
    /\ claimOut' = FALSE
    /\ UNCHANGED <<net, gen, delivered, owner, refreshOut, hbAlive, lostLease,
                   bPhase, commits, selfConflict, commitAfterLoss>>

AdversaryStep == C!Adversary /\ UNCHANGED protoVars

---------------------------------------------------------------------------
(* CLOSEOUT A (weakly fair).                                                *)

SendClaim ==
    /\ aPhase = "claim"
    /\ ~claimOut
    /\ claimOut' = TRUE
    /\ C!Send(Claim(aId))
    /\ UNCHANGED <<owner, aPhase, aId, rerun, refreshOut, hbAlive, lostLease,
                   bPhase, commits, selfConflict, commitAfterLoss>>

\* The claim's reply never came (nothing of it is still in flight).
ClaimLost ==
    /\ aPhase = "claim"
    /\ claimOut
    /\ C!InFlight \cap ClaimMsgs(aId) = {}
    /\ claimOut' = FALSE
    /\ aPhase' = IF StableOwnerId THEN "claim" ELSE "failed"
    /\ UNCHANGED <<net, gen, delivered, owner, aId, rerun, refreshOut, hbAlive,
                   lostLease, bPhase, commits, selfConflict, commitAfterLoss>>

SendRefresh ==
    /\ aPhase = "work"
    /\ hbAlive
    /\ ~refreshOut
    /\ refreshOut' = TRUE
    /\ C!Send(Refresh(aId))
    /\ UNCHANGED <<owner, aPhase, aId, rerun, claimOut, hbAlive, lostLease,
                   bPhase, commits, selfConflict, commitAfterLoss>>

\* The refresh's reply never came: a transport failure, not a lease verdict.
RefreshLost ==
    /\ refreshOut
    /\ C!InFlight \cap RefreshMsgs(aId) = {}
    /\ refreshOut' = FALSE
    /\ hbAlive' = (hbAlive /\ HeartbeatRetries)
    /\ UNCHANGED <<net, gen, delivered, owner, aPhase, aId, rerun, claimOut,
                   lostLease, bPhase, commits, selfConflict, commitAfterLoss>>

CommitA ==
    /\ aPhase = "work"
    /\ AbortOnLeaseLoss => ~lostLease
    /\ CommitFenced => owner = aId
    /\ aPhase' = "done"
    /\ hbAlive' = FALSE
    /\ commits' = Bump(commits)
    /\ commitAfterLoss' = (commitAfterLoss \/ lostLease)
    /\ owner' = IF owner = aId THEN "none" ELSE owner
    /\ UNCHANGED <<net, gen, delivered, aId, rerun, claimOut, refreshOut,
                   lostLease, bPhase, selfConflict>>

AbortA ==
    /\ aPhase = "work"
    /\ AbortOnLeaseLoss
    /\ lostLease
    /\ aPhase' = "aborted"
    /\ hbAlive' = FALSE
    /\ UNCHANGED <<net, gen, delivered, owner, aId, rerun, claimOut, refreshOut,
                   lostLease, bPhase, commits, selfConflict, commitAfterLoss>>

---------------------------------------------------------------------------
(* RECEIVERS (each always takes its message off the wire).                  *)

\* `decide_owner_claim`: a committed cycle is superseded; a free lease or the
\* same owner id is (re)acquired; anything else is held by another.
ServerRecvClaim(id) ==
    /\ Claim(id) \in C!InFlight
    /\ IF commits > 0
          THEN /\ C!DeliverAndSend(Claim(id), Superseded(id))
               /\ UNCHANGED owner
       ELSE IF owner \in {"none", id}
          THEN /\ owner' = id
               /\ C!DeliverAndSend(Claim(id), Acq(id))
          ELSE /\ C!DeliverAndSend(Claim(id), Held(id, owner))
               /\ UNCHANGED owner
    /\ UNCHANGED <<aPhase, aId, rerun, claimOut, refreshOut, hbAlive, lostLease,
                   bPhase, commits, selfConflict, commitAfterLoss>>

ServerRecvRefresh(id) ==
    /\ Refresh(id) \in C!InFlight
    /\ IF owner = id
          THEN C!DeliverAndSend(Refresh(id), RefOk(id))
          ELSE C!DeliverAndSend(Refresh(id), RefLost(id))
    /\ UNCHANGED protoVars

ARecvAcq(id) ==
    /\ Acq(id) \in C!InFlight
    /\ C!Deliver(Acq(id))
    /\ IF aPhase = "claim" /\ claimOut /\ id = aId
          THEN /\ aPhase' = "work"
               /\ claimOut' = FALSE
               /\ hbAlive' = TRUE
          ELSE UNCHANGED <<aPhase, claimOut, hbAlive>>
    /\ UNCHANGED <<owner, aId, rerun, refreshOut, lostLease, bPhase, commits,
                   selfConflict, commitAfterLoss>>

ARecvHeld(id, h) ==
    /\ Held(id, h) \in C!InFlight
    /\ C!Deliver(Held(id, h))
    /\ IF aPhase = "claim" /\ claimOut /\ id = aId
          THEN /\ aPhase' = "failed"
               /\ claimOut' = FALSE
               /\ selfConflict' = (selfConflict \/ (h \in Ids /\ h # id))
          ELSE UNCHANGED <<aPhase, claimOut, selfConflict>>
    /\ UNCHANGED <<owner, aId, rerun, refreshOut, hbAlive, lostLease, bPhase,
                   commits, commitAfterLoss>>

ARecvSuperseded(id) ==
    /\ Superseded(id) \in C!InFlight
    /\ C!Deliver(Superseded(id))
    /\ IF aPhase = "claim" /\ claimOut /\ id = aId
          THEN /\ aPhase' = "aborted"
               /\ claimOut' = FALSE
          ELSE UNCHANGED <<aPhase, claimOut>>
    /\ UNCHANGED <<owner, aId, rerun, refreshOut, hbAlive, lostLease, bPhase,
                   commits, selfConflict, commitAfterLoss>>

ARecvRefOk(id) ==
    /\ RefOk(id) \in C!InFlight
    /\ C!Deliver(RefOk(id))
    /\ refreshOut' = IF id = aId THEN FALSE ELSE refreshOut
    /\ UNCHANGED <<owner, aPhase, aId, rerun, claimOut, hbAlive, lostLease,
                   bPhase, commits, selfConflict, commitAfterLoss>>

\* An ANSWERED "you are not the owner": the lease really is gone.
ARecvRefLost(id) ==
    /\ RefLost(id) \in C!InFlight
    /\ C!Deliver(RefLost(id))
    /\ IF id = aId /\ aPhase = "work"
          THEN /\ refreshOut' = FALSE
               /\ hbAlive' = FALSE
               /\ lostLease' = TRUE
          ELSE UNCHANGED <<refreshOut, hbAlive, lostLease>>
    /\ UNCHANGED <<owner, aPhase, aId, rerun, claimOut, bPhase, commits,
                   selfConflict, commitAfterLoss>>

Recv ==
    \/ \E i \in Ids : ServerRecvClaim(i) \/ ServerRecvRefresh(i)
                      \/ ARecvAcq(i) \/ ARecvSuperseded(i)
                      \/ ARecvRefOk(i) \/ ARecvRefLost(i)
    \/ \E i \in Ids, h \in Holders : ARecvHeld(i, h)

Next ==
    \/ Expire \/ BClaim \/ BCommit \/ Rerun \/ AdversaryStep
    \/ SendClaim \/ ClaimLost \/ SendRefresh \/ RefreshLost \/ CommitA \/ AbortA
    \/ Recv

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(SendClaim)
    /\ WF_vars(ClaimLost)
    /\ WF_vars(SendRefresh)
    /\ WF_vars(RefreshLost)
    \* Fair-lossy for the claim exchange only: the one liveness property is
    \* about the claim, and SF over every message is too slow for make check.
    /\ C!FairLossy(ClaimMsgs("a1") \cup ClaimMsgs("a2"))

---------------------------------------------------------------------------
(* SAFETY (no fairness, full adversary).                                    *)

\* F4: a lost claim reply never leaves an orphan that refuses its own owner.
NoSelfConflict == ~selfConflict

\* F5 (a): a transport failure never silently stops the heartbeat of a
\* closeout that still believes it owns the lease.
HeartbeatAliveWhileOwning == (aPhase = "work" /\ ~lostLease) => hbAlive

\* F5 (b): an answered lease loss is never followed by this closeout's commit.
NoCommitAfterLostLease == ~commitAfterLoss

\* ADC-fence: one commit per cycle. Needs a fenced commit (not in code).
AtMostOneCommit == commits <= 1

(* LIVENESS (FairLossy + A's own steps): a claim is eventually answered.    *)
ClaimEventuallyAnswered == <>(aPhase # "claim")

(* REACH (asserted negated; MUST be violated).                              *)
NeverCommitsA == aPhase # "done"
=============================================================================
