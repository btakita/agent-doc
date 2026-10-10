------------------------- MODULE ControllerLifecycle -------------------------
EXTENDS Naturals, TLC

(***************************************************************************
`#supthrash` — project-controller PROCESS lifecycle for one project root.

A lazy controller is launched by the first client, may be superseded by an
install (its binary goes stale), hands off to a replacement on the installed
binary, and retires when nothing uses its root. The incident this model pins
(2026-10-10): idle roots kept a controller forever and every install handed
it off to another idle one (generations 175, 361, ...), and a handoff that
could never succeed (`sun_path` overflow) was retried every debounce for six
days.

Environment edges: an install, a client/editor/supervisor starting or stopping
use of the root, and an actor opening or closing. `ClosedHistory` is the
`closed` actor rows a root accumulates forever.

Controller edges and their guards mirror agent-doc-controller::recycle:
  RetireStale     — idle_stale_binary_controller_should_retire (StaleBinaryRecycle)
  RetireIdleTick  — idle_controller_should_retire (IdleTick)
  HandoffSucceed / HandoffFail — the self-handoff, bounded by
                    HandoffRetryBackoff (the variant).

Switches (TRUE in the fixed model; each wedge config turns ONE off):
  BackoffGuard        — failures advance a bounded variant and abandon.
  LiveOwnersOnly      — `closed` rows do not count as ownership.
  IdleTickRetire      — a current-binary controller retires when idle.
***************************************************************************)

CONSTANTS MaxInstalls, MaxAttempts, BackoffGuard, LiveOwnersOnly, IdleTickRetire

ClosedHistory == TRUE

VARIABLES ctl,        \* "absent" | "stable" (current binary) | "stale"
          inUse,      \* a live editor endpoint, supervisor, or client
          liveActors, \* a non-closed actor row exists
          installs,
          attempts,   \* consecutive failed handoffs against the current target
          abandoned,  \* the current target was abandoned (keep serving)
          feasible    \* can a handoff on this root ever succeed?

vars == <<ctl, inUse, liveActors, installs, attempts, abandoned, feasible>>

\* What the controller BELIEVES pins it to the root.
Owned == liveActors \/ (~LiveOwnersOnly /\ ClosedHistory)
Idle == ~inUse /\ ~Owned
\* Ground truth: nothing uses the root.
RootUnused == ~inUse /\ ~liveActors

CanAttempt == ~BackoffGuard \/ attempts < MaxAttempts

Init ==
    /\ ctl = "stable"
    /\ inUse \in BOOLEAN
    /\ liveActors \in BOOLEAN
    /\ installs = 0
    /\ attempts = 0
    /\ abandoned = FALSE
    /\ feasible \in BOOLEAN

\* ---------------------------------------------------------------- environment
Install ==
    /\ installs < MaxInstalls
    /\ installs' = installs + 1
    /\ ctl' = IF ctl = "absent" THEN "absent" ELSE "stale"
    /\ attempts' = 0
    /\ abandoned' = FALSE
    /\ UNCHANGED <<inUse, liveActors, feasible>>

\* Lazy launch: the first client of an absent root starts a current controller.
UseStart ==
    /\ ~inUse
    /\ inUse' = TRUE
    /\ ctl' = IF ctl = "absent" THEN "stable" ELSE ctl
    /\ attempts' = IF ctl = "absent" THEN 0 ELSE attempts
    /\ abandoned' = IF ctl = "absent" THEN FALSE ELSE abandoned
    /\ UNCHANGED <<liveActors, installs, feasible>>

UseStop ==
    /\ inUse
    /\ inUse' = FALSE
    /\ UNCHANGED <<ctl, liveActors, installs, attempts, abandoned, feasible>>

ActorOpen ==
    /\ inUse
    /\ ~liveActors
    /\ liveActors' = TRUE
    /\ UNCHANGED <<ctl, inUse, installs, attempts, abandoned, feasible>>

ActorClose ==
    /\ liveActors
    /\ liveActors' = FALSE
    /\ UNCHANGED <<ctl, inUse, installs, attempts, abandoned, feasible>>

\* ----------------------------------------------------------------- controller
RetireStale ==
    /\ ctl = "stale"
    /\ ~abandoned
    /\ Idle
    /\ ctl' = "absent"
    /\ attempts' = 0
    /\ UNCHANGED <<inUse, liveActors, installs, abandoned, feasible>>

RetireIdleTick ==
    /\ IdleTickRetire
    /\ ctl \in {"stable", "stale"}
    /\ Idle
    /\ ctl' = "absent"
    /\ attempts' = 0
    /\ abandoned' = FALSE
    /\ UNCHANGED <<inUse, liveActors, installs, feasible>>

HandoffSucceed ==
    /\ ctl = "stale"
    /\ ~abandoned
    /\ ~Idle
    /\ CanAttempt
    /\ feasible
    /\ ctl' = "stable"
    /\ attempts' = 0
    /\ UNCHANGED <<inUse, liveActors, installs, abandoned, feasible>>

\* A failure is always possible (transient), and the only outcome on an
\* infeasible root (permanent).
HandoffFail ==
    /\ ctl = "stale"
    /\ ~abandoned
    /\ ~Idle
    /\ CanAttempt
    /\ attempts' = attempts + 1
    /\ abandoned' = (BackoffGuard /\ (~feasible \/ attempts + 1 >= MaxAttempts))
    /\ UNCHANGED <<ctl, inUse, liveActors, installs, feasible>>

Environment == Install \/ UseStart \/ UseStop \/ ActorOpen \/ ActorClose
Controller == RetireStale \/ RetireIdleTick \/ HandoffSucceed \/ HandoffFail

Next == Environment \/ Controller

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Controller)

\* ----------------------------------------------------------------- properties
TypeOK ==
    /\ ctl \in {"absent", "stable", "stale"}
    /\ inUse \in BOOLEAN
    /\ liveActors \in BOOLEAN
    /\ installs \in 0..MaxInstalls
    /\ attempts \in Nat
    /\ abandoned \in BOOLEAN
    /\ feasible \in BOOLEAN

\* L4 — every retry loop has a variant: failed handoffs against one target are
\* bounded. Without the backoff the incident loop made 104,411 attempts.
BoundedHandoffAttempts == attempts <= MaxAttempts

\* L4 — an abandoned target is quiescent: no further attempts.
AbandonedIsQuiescent == abandoned => ~ENABLED (HandoffSucceed \/ HandoffFail)

\* L3 — a controller never retires from a root something is using.
NeverRetiresInUse ==
    [][(ctl # "absent" /\ ctl' = "absent") => (~inUse /\ ~liveActors)]_vars

\* L2 — a root nothing uses eventually has no controller process.
IdleRootEventuallyRetires == <>[]RootUnused => <>[](ctl = "absent")

\* L4 — a superseded controller eventually stops being an open retry loop:
\* it is replaced, retires, or abandons the target and keeps serving.
StaleEventuallyQuiescent ==
    [](ctl = "stale" => <>(ctl # "stale" \/ abandoned))

\* Reachability witnesses (each must be VIOLATED by its Reach config).
NeverRetired == ctl # "absent"
NeverAbandoned == ~abandoned
NeverPromoted == ~(ctl = "stable" /\ installs > 0 /\ inUse)

=============================================================================
