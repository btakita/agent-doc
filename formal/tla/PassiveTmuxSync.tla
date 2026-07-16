--------------------------- MODULE PassiveTmuxSync ---------------------------
EXTENDS Naturals, TLC

(***************************************************************************
The PlusCal algorithm models the safe-passive exact-visible sync used by
editor tab selection.  Both requests know that the target actor exists, but
only the request executing inside the owning Project Controller may consume
the authoritative SQLite actor binding.  That controller-local proof permits
an atomic visible/stashed pane swap.  A standalone passive request remains
blocked, performs no nested actor lookup, and neither path autostarts an actor.
***************************************************************************)

Requests == {"controller", "external"}
Actors == {"old", "target"}
RequestStates == {"pending", "proved", "applied", "blocked"}
ControllerLocal == [request \in Requests |-> request = "controller"]

(* --fair algorithm SafePassiveTmuxSync
variables
    requestState = [request \in Requests |-> "pending"],
    actorBinding = [request \in Requests |-> TRUE],
    actorLookupCount = [request \in Requests |-> 0],
    visibleActor = "old",
    stashedActor = "target",
    autostartCount = 0;

process ControllerSync = "controller"
begin
ControllerLocalProof:
    assert ControllerLocal["controller"] /\ actorBinding["controller"];
    actorLookupCount := [actorLookupCount EXCEPT !["controller"] = @ + 1];
    requestState := [requestState EXCEPT !["controller"] = "proved"];
ControllerAtomicSwap:
    assert requestState["controller"] = "proved" /\ actorLookupCount["controller"] = 1;
    visibleActor := "target";
    stashedActor := "old";
    requestState := [requestState EXCEPT !["controller"] = "applied"];
ControllerDone:
    while TRUE do
        skip;
    end while;
end process;

process ExternalSync = "external"
begin
ExternalPassiveGuard:
    assert ~ControllerLocal["external"];
    requestState := [requestState EXCEPT !["external"] = "blocked"];
ExternalDone:
    while TRUE do
        skip;
    end while;
end process;
end algorithm; *)

TypeOK ==
    /\ requestState \in [Requests -> RequestStates]
    /\ actorBinding \in [Requests -> BOOLEAN]
    /\ actorLookupCount \in [Requests -> Nat]
    /\ visibleActor \in Actors
    /\ stashedActor \in Actors
    /\ autostartCount \in Nat

LookupRequiresControllerLocalAuthority ==
    \A request \in Requests :
        actorLookupCount[request] > 0 => ControllerLocal[request]

ControllerProofIsExact ==
    /\ actorLookupCount["controller"] <= 1
    /\ requestState["controller"] \in {"proved", "applied"}
        => actorLookupCount["controller"] = 1

ExternalPassiveRequestNeverUsesActorLookup ==
    /\ actorLookupCount["external"] = 0
    /\ requestState["external"] \in {"pending", "blocked"}

AppliedSyncRequiresProof ==
    requestState["controller"] = "applied" =>
        /\ actorBinding["controller"]
        /\ actorLookupCount["controller"] = 1
        /\ visibleActor = "target"
        /\ stashedActor = "old"

VisibleAndStashedActorsRemainAUniquePartition ==
    /\ visibleActor # stashedActor
    /\ {visibleActor, stashedActor} = Actors

TargetVisibleIffControllerSyncApplied ==
    (visibleActor = "target") <=> requestState["controller"] = "applied"

PassiveSyncNeverAutostarts == autostartCount = 0

EventuallyControllerLocalSyncApplies ==
    <> (requestState["controller"] = "applied" /\ visibleActor = "target")

EventuallyExternalPassiveSyncBlocks ==
    <> (requestState["external"] = "blocked")

=============================================================================
