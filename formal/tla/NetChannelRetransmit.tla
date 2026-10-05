------------------------- MODULE NetChannelRetransmit -------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Reference protocol over the adversarial `NetChannel`: resend-until-ack with an
idempotent receiver keyed by (sequence, generation). It is the template for
re-checking the hot-path models (netadv3) over a lossy network, and the
target of the wedges that prove `NetChannel` has teeth.

THE PROTOCOL
------------
The sender delivers items 1..N in order, stop-and-wait. It keeps putting
`Data(sNext, gen)` in flight until it receives `Ack(sNext, gen)` for the
CURRENT generation, then moves on. The receiver keeps `rNext` and `applied`
as durable state (state.db): those survive a reconnect, in-flight messages
do not. On `Data(s, g)`:

  g # gen          -- stale generation: discard, apply nothing
  s = rNext        -- apply once, advance, ack
  s < rNext        -- duplicate (sender lost the ack): re-ack, apply nothing

The receiver always takes a message off the wire, so the fair-lossy
assumption applies to every message.

THE PROPERTIES
--------------
Safety holds under the FULLY adversarial channel (no fairness at all):
`ExactlyOnce`, `NoStaleApply`, `AckedImpliesApplied`. Liveness, `Completes`,
holds under `NetChannel!FairLossy` plus weak fairness on the resend.

THE KNOBS (one wedge per knob; scripts/run_tla.sh asserts each MUST violate)
-----------------------------------------------------------------------
  Retransmit = FALSE -- fire-and-forget: each item is sent once. One Drop
                        stalls it forever; `Completes` MUST be violated
                        (NetChannelRetransmitFireAndForgetWedge.cfg).
  Idempotent = FALSE -- the receiver applies a duplicate again; `ExactlyOnce`
                        MUST be violated (NetChannelRetransmitDuplicateWedge.cfg).
  GenCheck   = FALSE -- the receiver ignores generation; a message that
                        survived a Reconnect is applied in the new generation
                        and `NoStaleApply` MUST be violated
                        (NetChannelRetransmitStaleGenWedge.cfg).
  ChannelFair = FALSE -- drop the fair-lossy assumption, keep the resend fair:
                        the adversary drops every copy forever and `Completes`
                        MUST be violated (NetChannelRetransmitUnfairWedge.cfg).
                        Proves the liveness proof rests on FairLossy, not on
                        the resend alone.

`NetChannelRetransmitReach.cfg` asserts that the protocol never completes after
having discarded both a duplicate and a stale message. It must be violated:
proof that the adversary actually duplicates and reorders across a reconnect
in the positive run, so the green result is not vacuous.
***************************************************************************)

CONSTANTS N, MaxCopies, MaxGen, Retransmit, Idempotent, GenCheck, ChannelFair

VARIABLES
    net, gen, delivered, \* the channel (NetChannel)
    sNext,              \* sender: next item awaiting its ack
    sSent,              \* sender: current item put in flight at least once
    rNext,              \* receiver (durable): next item to apply
    applied,            \* receiver (durable): apply count per item, capped at 2
    staleApplied,       \* history: some apply used a message from an old gen
    sawDupDiscard,      \* history (Reach only): a duplicate was not re-applied
    sawStaleDiscard     \* history (Reach only): a stale message was discarded

C == INSTANCE NetChannel

protoVars == <<sNext, sSent, rNext, applied, staleApplied, sawDupDiscard, sawStaleDiscard>>
vars == <<net, gen, delivered, protoVars>>

Items == 1..N
Data(s, g) == [type |-> "data", seq |-> s, gen |-> g]
Ack(s, g) == [type |-> "ack", seq |-> s, gen |-> g]
Msgs == {Data(s, g) : s \in Items, g \in 0..MaxGen}
     \cup {Ack(s, g) : s \in Items, g \in 0..MaxGen}

Bump(x) == IF x >= 2 THEN 2 ELSE x + 1

TypeOK ==
    /\ C!ChannelTypeOK(Msgs)
    /\ sNext \in 1..N + 1
    /\ sSent \in BOOLEAN
    /\ rNext \in 1..N + 1
    /\ applied \in [Items -> 0..2]
    /\ staleApplied \in BOOLEAN
    /\ sawDupDiscard \in BOOLEAN
    /\ sawStaleDiscard \in BOOLEAN

Init ==
    /\ C!ChannelInit
    /\ sNext = 1
    /\ sSent = FALSE
    /\ rNext = 1
    /\ applied = [s \in Items |-> 0]
    /\ staleApplied = FALSE
    /\ sawDupDiscard = FALSE
    /\ sawStaleDiscard = FALSE

---------------------------------------------------------------------------
\* Sender: (re)send the current item, stamped with the current generation.
SenderSend ==
    /\ sNext <= N
    /\ Retransmit \/ ~sSent
    /\ C!Send(Data(sNext, gen))
    /\ sSent' = TRUE
    /\ UNCHANGED <<sNext, rNext, applied, staleApplied, sawDupDiscard, sawStaleDiscard>>

\* Sender: an ack for the current item in the current generation advances it;
\* any other ack (old item, old generation) is consumed and ignored.
SenderRecvAck(m) ==
    /\ m.type = "ack"
    /\ C!Deliver(m)
    /\ IF m.seq = sNext /\ m.gen = gen
          THEN /\ sNext' = sNext + 1
               /\ sSent' = FALSE
          ELSE UNCHANGED <<sNext, sSent>>
    /\ UNCHANGED <<rNext, applied, staleApplied, sawDupDiscard, sawStaleDiscard>>

\* Receiver: always takes the message off the wire.
ReceiverRecvData(m) ==
    /\ m.type = "data"
    /\ IF GenCheck /\ m.gen # gen
          THEN \* Stale generation: discard.
               /\ C!Deliver(m)
               /\ sawStaleDiscard' = TRUE
               /\ UNCHANGED <<rNext, applied, staleApplied, sawDupDiscard>>
       ELSE IF m.seq = rNext
          THEN \* Fresh: apply once, advance, ack.
               /\ C!DeliverAndSend(m, Ack(m.seq, gen))
               /\ applied' = [applied EXCEPT ![m.seq] = Bump(@)]
               /\ rNext' = rNext + 1
               /\ staleApplied' = (staleApplied \/ m.gen # gen)
               /\ UNCHANGED <<sawDupDiscard, sawStaleDiscard>>
       ELSE IF m.seq < rNext
          THEN \* Duplicate: re-ack; apply again only if not idempotent.
               /\ C!DeliverAndSend(m, Ack(m.seq, gen))
               /\ IF Idempotent
                     THEN /\ sawDupDiscard' = TRUE
                          /\ UNCHANGED <<applied, staleApplied>>
                     ELSE /\ applied' = [applied EXCEPT ![m.seq] = Bump(@)]
                          /\ staleApplied' = (staleApplied \/ m.gen # gen)
                          /\ UNCHANGED sawDupDiscard
               /\ UNCHANGED <<rNext, sawStaleDiscard>>
          ELSE \* Ahead of rNext: unreachable under stop-and-wait; discard.
               /\ C!Deliver(m)
               /\ UNCHANGED <<rNext, applied, staleApplied, sawDupDiscard, sawStaleDiscard>>
    /\ UNCHANGED <<sNext, sSent>>

Recv(m) == SenderRecvAck(m) \/ ReceiverRecvData(m)

\* The network misbehaves. Durable state survives a reconnect; the sender
\* re-stamps its next resend with the new generation.
AdversaryStep ==
    /\ C!Adversary
    /\ UNCHANGED protoVars

Next ==
    \/ SenderSend
    \/ \E m \in C!InFlight : Recv(m)
    \/ AdversaryStep

\* Fair-lossy channel + a sender that keeps resending. Nothing constrains the
\* adversary: Drop, Duplicate and Reconnect may be taken at any point.
Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(SenderSend)
    /\ IF ChannelFair THEN C!FairLossy(Msgs) ELSE TRUE

---------------------------------------------------------------------------
\* Safety (holds with no fairness at all).
ExactlyOnce == \A s \in Items : applied[s] <= 1
NoStaleApply == ~staleApplied
AckedImpliesApplied == \A s \in Items : s < sNext => applied[s] >= 1

\* Liveness (needs FairLossy + WF on the resend).
Completes == <>(sNext = N + 1 /\ \A s \in Items : applied[s] = 1)

\* Reach: MUST be violated (see header).
NeverCompletesUnderAdversary ==
    ~(sNext = N + 1 /\ sawDupDiscard /\ sawStaleDiscard)

=============================================================================
