----------------------------- MODULE NetChannel -----------------------------
EXTENDS Naturals, TLC

(***************************************************************************
A reusable ADVERSARIAL channel. Models import it with INSTANCE instead of
treating an IPC request/response as one atomic step.

WHY
---
Most models in this directory collapse "A sends, B receives" into a single
transition. That silently assumes the network is reliable, ordered, and
instantaneous. Production runs over sockets, proxies (Zscaler can stall a TCP
stream with no reset), reconnects, and process restarts. This module makes the
network an explicit adversary so a model's safety argument can never depend on
a message arriving, arriving once, arriving in order, or arriving before a
reconnect. See tasks/agent-doc/plan-network-adversarial-correctness.md.

THE CHANNEL
-----------
`net` is a BAG (multiset) of in-flight messages: a function from message to
its number of copies (>= 1). A bag has no order, so any in-flight message may
be delivered next: delay and reordering are free. `gen` is the connection
generation, bumped by every reconnect.

Adversary steps (taken whenever enabled, with no fairness at all):

  Drop       -- any in-flight copy vanishes (loss; also a silent half-open stall)
  Duplicate  -- any in-flight message gains a copy
  Reconnect  -- `gen` increments and ANY subset of distinct in-flight messages
                is discarded; the rest survive and arrive later, stale.

Protocol steps (the importing model conjoins them with its own handler):

  Send(m), Deliver(m), DeliverAndSend(m, r)

BOUNDS
------
`MaxCopies` caps the copies of one message. A send or duplicate past the cap
coalesces: the extra copy is dropped, which the adversary may do anyway, so the
cap removes no adversarial behaviour that matters for safety and keeps at least
one copy in flight for liveness. `MaxGen` caps reconnects so the state space is
finite. Nothing here bounds Drop: loss may happen forever.

FAIRNESS
--------
`FairLossy(Msgs)` is the fair-lossy assumption: a message that is put in
flight infinitely often is eventually received. Weak fairness is NOT
enough to state that. Drop disables delivery between resends, so delivery is
never CONTINUOUSLY enabled and WF never fires; delivery is enabled infinitely
often, which is exactly strong fairness. So FairLossy is SF on `Delivered(m, Msgs)`
for every message. (It takes no receive-action parameter because TLC 1.7.4
cannot check a temporal formula built from an operator argument; the
`delivered` variable is what lets the channel name the receive step itself.)

It says nothing about messages sent finitely often: a message sent once may be
dropped and never delivered, which is why a fire-and-forget protocol fails
liveness under this module.

A model that wants progress must therefore also make its own (re)send action
fair (usually WF), so that a message keeps being put in flight until acked.
***************************************************************************)

CONSTANTS MaxCopies, MaxGen

VARIABLES net, gen, delivered

chanVars == <<net, gen, delivered>>

\* `delivered` is {m} when the last channel step handed `m` to a receiver and
\* {} after a send or an adversary step (a set, so it compares with any message
\* shape). It exists only so fairness can tell a delivery from a Drop: both
\* remove one copy.

EmptyBag == [m \in {} |-> 0]

InFlight == DOMAIN net

Copies(m) == IF m \in DOMAIN net THEN net[m] ELSE 0

ChannelTypeOK(Msgs) ==
    /\ DOMAIN net \subseteq Msgs
    /\ \A m \in DOMAIN net : net[m] \in 1..MaxCopies
    /\ gen \in 0..MaxGen
    /\ delivered \in {{}} \cup {{m} : m \in Msgs}

\* Pure bag operators, so a step can take and put in one transition.
Put(b, m) ==
    IF m \in DOMAIN b
        THEN IF b[m] < MaxCopies THEN [b EXCEPT ![m] = @ + 1] ELSE b
        ELSE b @@ (m :> 1)

Take(b, m) ==
    IF b[m] = 1
        THEN [x \in DOMAIN b \ {m} |-> b[x]]
        ELSE [b EXCEPT ![m] = @ - 1]

ChannelInit ==
    /\ net = EmptyBag
    /\ gen = 0
    /\ delivered = {}

---------------------------------------------------------------------------
\* Protocol-facing actions. Each leaves `gen` alone; the caller states what
\* happens to its own variables.

\* Put `m` in flight. At the copy cap this is a no-op on `net` (coalesced).
Send(m) ==
    /\ net' = Put(net, m)
    /\ delivered' = {}
    /\ UNCHANGED gen

\* Remove one copy of an in-flight `m`: the receiver now holds it.
Deliver(m) ==
    /\ m \in DOMAIN net
    /\ net' = Take(net, m)
    /\ delivered' = {m}
    /\ UNCHANGED gen

\* Receive `m` and send reply `r` in the same step (e.g. data -> ack).
DeliverAndSend(m, r) ==
    /\ m \in DOMAIN net
    /\ net' = Put(Take(net, m), r)
    /\ delivered' = {m}
    /\ UNCHANGED gen

---------------------------------------------------------------------------
\* Adversary actions. A model allows them with
\*     \/ (C!Adversary /\ UNCHANGED protocolVars)
\* or, to react to a reconnect (resync from durable state),
\*     \/ (C!Reconnect /\ OnReconnect)

Drop ==
    /\ \E m \in DOMAIN net : net' = Take(net, m)
    /\ delivered' = {}
    /\ UNCHANGED gen

Duplicate ==
    /\ \E m \in DOMAIN net :
        /\ net[m] < MaxCopies
        /\ net' = [net EXCEPT ![m] = @ + 1]
    /\ delivered' = {}
    /\ UNCHANGED gen

Reconnect ==
    /\ gen < MaxGen
    /\ gen' = gen + 1
    /\ delivered' = {}
    /\ \E lost \in SUBSET DOMAIN net :
        net' = [m \in DOMAIN net \ lost |-> net[m]]

Adversary == Drop \/ Duplicate \/ Reconnect

---------------------------------------------------------------------------
\* A step that hands `m` to a receiver, with or without a reply in `Msgs`:
\* exactly the channel half of Deliver(m) or DeliverAndSend(m, r). Drop and
\* every other channel step set `delivered' = {}`, so a loss never counts as
\* a delivery. (Every channel variable is pinned because TLC evaluates
\* ENABLED by generating successor states of this action.)
Delivered(m, Msgs) ==
    /\ m \in DOMAIN net
    /\ delivered' = {m}
    /\ net' \in {Take(net, m)} \cup {Put(Take(net, m), r) : r \in Msgs}
    /\ UNCHANGED gen

\* Fair-lossy channel: every message in `Msgs` that is in flight infinitely
\* often is eventually delivered -- strong fairness on `Delivered(m)` per
\* message (see FAIRNESS in the header for why weak fairness cannot say it).
\* Only a step that conjoins C!Deliver(m) or C!DeliverAndSend(m, _) satisfies
\* it, so the importing model's receive action for `m` must be enabled
\* whenever `m` is in flight: a receiver that can refuse to take a message
\* off the wire defeats the assumption, and the model would then prove
\* liveness only for behaviours that do not exist.
FairLossy(Msgs) == \A m \in Msgs : SF_chanVars(Delivered(m, Msgs))

=============================================================================
