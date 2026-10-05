# Plan: agent-doc provably correct under varying network conditions

Operator requirement (2026-10-05): "agent-doc needs to be provably correct with varying
network conditions." The deployment design point is a Coder remote dev workspace
(JetBrains Remote Dev backend) with Zscaler in the path, which means high and variable
latency, jitter, and possible connection resets.

## Gap

The TLA+ corpus in `src/agent-doc/formal/tla/` (about 30 models, with Reach and Wedge
variants) treats the editor↔controller↔tmux IPC hop as an atomic step or a reliable
channel. `formal/tla/README.md` never mentions channel delay, reordering, loss,
duplication or reconnect. So the safety properties those models prove are proven only
for a perfect network.

## Target property set

For each cross-process protocol, safety must hold under an adversarial channel, and
liveness must hold under fair, eventually-delivering channels:

- **Safety under adversarial channels:** any delay, any reordering, message loss,
  duplication, and reconnects with a new connection or generation. No timeout
  value may be part of a safety argument.
- **Liveness under weak fairness:** a message re-sent infinitely often is eventually
  delivered. Progress may wait on delivery but never on a wall-clock guess.
- **Latency:** the number of serial round trips on hot paths (focus/layout
  projection, response delivery, closeout ACK) is bounded and stated per path. Any
  remaining timeout is configurable or derived from measured RTT, never a fixed
  localhost-tuned constant.

## Message loss (operator question, 2026-10-05)

Yes, messages may be lost, and losing a message is the harder case. Delay and
reordering only postpone delivery, but a lost message never arrives unless something
sends it again. The NetChannel model's Drop action covers loss. Loss requires every
hot-path protocol to have these properties:

- **Retransmit until acked, or re-derive by pull.** Each state change crossing the
  network is either resent until acknowledged, or carried as state that the receiver
  can pull or resync from (a level-triggered snapshot or generation, not an
  edge-triggered event). A one-shot fire-and-forget notification on a correctness
  path is a defect.
- **Idempotent receivers.** The sender can't tell a lost request from a lost ack,
  so it resends, and the receiver must apply each message exactly once by key
  (idempotency key, sequence number or generation), even under duplication.
- **Half-open connections.** Zscaler and other proxies can silently stall a TCP
  stream with no reset. Liveness needs a heartbeat/lease that turns a silent stall
  into a reconnect, and safety must not depend on how fast that heartbeat fires.
- **Reconnect resync.** After a reconnect the two sides reconcile from durable state
  (state.db, generation counters), never from assumed in-flight messages.
- **Liveness statement:** under fairness, a message resent infinitely often is
  eventually delivered. Without resending, the model checker must find the stall.
  The Wedge configs include a fire-and-forget variant for that reason.

## Phases

1. **netadv1 — channel-assumption audit.** For every model in `formal/tla/` and every
   IPC protocol in `agent-doc-ipc-protocol` and `agent-doc-reliable-sync-io`, record
   the channel assumption (atomic, FIFO, reliable, or lossy), the timeouts on its
   correctness path, and the serial round-trip count. Output: a table in
   `formal/tla/README.md` plus a ranked list of protocols whose safety depends on
   timing.
2. **netadv2 — shared adversarial channel module.** Write `formal/tla/NetChannel.tla`:
   a bag-based channel with Deliver, Drop, Duplicate and Reconnect actions, plus a
   fair variant. Add Wedge configs that prove the module actually finds a known
   timing bug, so a green run isn't vacuous.
3. **netadv3 — re-check the hot-path models over NetChannel.** Start with the
   protocols ranked highest in netadv1, likely VisibleDeliveryReceipt,
   EditorReplicaStrand, PassiveTmuxSync, RecycleSettleDispatch, AgentDocCloseout and
   the GH #136 layout/recycle model. Fix every counterexample in code with
   SimWorld coverage that replays the trace.
4. **netadv4 — SimWorld network fault injection.** Give the SimWorld harness a
   seeded network-conditions layer (latency distribution, jitter, reorder, drop,
   duplicate, reconnect) with a Coder+Zscaler profile, and run the conformance
   suites under it in `make check`.
6. **netadv6 — deterministic simulation fuzzing.** Builds on netadv4. A seeded
   explorer drives SimWorld through random schedules: process interleavings, network
   faults, editor edits, recycles/installs, crashes and reconnects. After every step
   it checks invariant oracles, the same safety properties the TLA+ models state
   (unique owner, no lost operator text, exactly-once commit, recycle request
   eventually consumed under fairness). A failing seed is shrunk to a minimal trace
   and committed as a permanent regression seed. `make check` runs a short fixed
   budget plus every regression seed; a nightly CI job runs a long budget on fresh
   seeds and opens an issue with the shrunk trace on failure.
7. **netadv7 — coverage-guided fuzzing of untrusted input.** `cargo-fuzz` targets
   for the bytes that cross a process or network boundary: IPC wire decode, the
   markdown component/patch parsers, frontmatter, and CRDT op decode. Oracles: no
   panics, and lossless round-trips where the parser claims them. Corpus seeded from
   real `.agent-doc/logs` traffic (redacted), with crashers promoted to unit tests.
   proptest is already used in 3 places; extend it to the pure state-transition
   functions that the TLA+ models describe (model-based testing: the same transition
   table drives both).
5. **netadv5 — latency budget enforcement.** For each hot path, a test asserts the
   serial round-trip count. Fixed sleeps or settle windows on correctness paths are
   replaced with ack-driven transitions.

## Related

- GH #136 (stale stash supervisor pane admitted as a layout column) is the first
  concrete instance; its fix carries a formal state-machine model for the
  admission → recycle lifecycle.
