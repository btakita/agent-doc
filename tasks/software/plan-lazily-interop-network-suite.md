# Plan: one Lazily cross-binding network conformance suite

Status: Phase 0-1 implemented; production peer rollout pending
Owner: `src/lazily-interop`
Contract: `src/lazily-interop/docs/peer-contract.md`
Companion plans:

- `tasks/software/plan-lazily-interop-peer-rollout.md`
- `tasks/software/plan-lazily-interop-channel-matrix.md`

## Goal

Build one suite that launches every Lazily implementation as a real peer and
proves two independent claims:

1. different implementations interpret the canonical state plane identically;
2. every supported data channel carries that state plane without changing,
   truncating, merging, reordering outside its contract, or losing it across a
   reconnect.

The participating bindings are Rust, Python, Kotlin, JavaScript, Dart, Zig, Go,
C++, and C#. The suite is one orchestrator and one peer contract, not nine
binding-specific test harnesses.

## Authority and boundaries

Authority, in order:

1. `src/lazily-spec/schemas/*.json` and the canonical conformance fixtures;
2. `src/lazily-spec/protocol.md`, especially Cross-language Channel
   Compatibility, Frame codecs, FFI, Shared-memory IPC, Distributed CRDT, and
   Reliable Sync;
3. `src/lazily-interop/docs/peer-contract.md`;
4. binding APIs only as adapters to the contract.

The orchestrator MUST NOT depend on lazily-rs wire structs or use Rust output as
the expected value. It validates emitted frames against the spec schemas,
compares independently produced snapshots, and treats the fixture corpus as the
oracle. Rust may be the first peer, but it is not the semantic authority.

Compute closures remain language-local. The network suite tests the shared state
plane: `Snapshot`, `Delta`, `CrdtSync`, `ResyncRequest`, `OutboxAck`, causal
receipts, permission-filtered frames, and signaling where the channel requires
it.

## Architecture

### One process protocol

Each binding ships one `interop peer` executable. The orchestrator controls it
over newline-delimited JSON on stdin/stdout. That control link is never under
test. A peer advertises:

- binding and version;
- peer-contract version;
- production-backed feature capabilities;
- supported frame codecs (`json`, `msgpack`, optional `postcard`);
- supported channels and channel variants;
- platform carve-outs such as `ffi = none`;
- optional observability counters (copies, reconnects, buffered bytes).

The peer contract stays deliberately small. It exposes local writes, frame
delivery, canonical snapshots, direct-link lifecycle, send/receive, statistics,
and shutdown. It does not expose binding internals.

Feature scenarios build their peer group from bindings that advertise the
required production feature. Unsupported implementations stay visible as
staged, are excluded from that feature's run, and receive no pass credit. Once a
peer is eligible, required codec/channel gaps fail closed. Fewer than two
eligible peers is red, preventing capability filtering from producing a vacuous
green.

### Two planes, two failure domains

The semantic plane relays frames through the orchestrator. Delivery schedules,
partitions, duplication, and clock values come from a deterministic scenario
seed. This plane proves interpretation and convergence without a real network
flake.

The transport plane opens a direct link between two peer processes. The sender
encodes with its own codec, the receiver decodes with its own codec, and the
orchestrator compares the received canonical frame with the sent frame. This
plane proves framing and channel behavior without mixing its timing failures
into convergence assertions.

Every failure record names:

- plane;
- scenario and seed;
- emitting and receiving bindings;
- channel and codec;
- first divergent frame or assertion;
- peer stderr and process exit status;
- a replay command.

### Orchestrator implementation

Create `src/lazily-interop` as a standalone Rust workspace containing:

- `lazily-interop` CLI;
- peer registry loader (`peers.toml`);
- NDJSON process supervisor with per-command deadlines;
- schema/fixture loader reading the sibling `lazily-spec` checkout;
- deterministic scenario engine and fault scheduler;
- direct-link endpoint allocator;
- JSON, JUnit, and concise console reporters;
- transcript minimizer that writes a replayable failing scenario.

Rust is chosen for a single static supervisor binary and reliable process/socket
control. The crate MUST parse control messages into its own harness DTOs and
validate state-plane frames from the JSON schemas; it MUST NOT import
`lazily-rs`.

## Scenario model

Scenario files live in `src/lazily-interop/scenarios/` and contain no
language-specific commands. Each declares:

- peers and assigned `PeerId`s;
- logical times;
- local operations;
- delivery graph and ordering;
- partitions, duplication, truncation, reconnect, and recovery actions;
- required codecs and channels;
- final and intermediate assertions.

The minimum semantic corpus is:

1. keyed and keyless LWW writes, including required `CrdtOp.key: null`;
2. equal-wall-time/equal-logical peer tie-break;
3. canonical tuple-shaped stamp frontiers;
4. duplicate delivery (`applied = 0`);
5. reverse and mixed delivery order;
6. partition, divergent writes, heal, and anti-entropy convergence;
7. frontier-based incremental catch-up;
8. gap detection, `ResyncRequest`, snapshot recovery, and outbox replay;
9. permission filtering before serialization;
10. causal receipt progression and terminal-outcome agreement;
11. inline and shared-blob payloads;
12. all externally tagged state-plane variants.

Every scenario runs with at least three peers once the reference pair is green.
Pairwise runs identify a culprit; three-way runs prevent one implementation from
becoming the de facto oracle.

## Execution phases

### Phase 0 — freeze the contract and result schema

Implemented in `src/lazily-interop`:

- Draft 2020-12 control, scenario, and result schemas;
- codec, feature, channel, variant, platform-profile, and carve-out negotiation;
- pinned failure categories and shell-safe replay commands.

- Review `docs/peer-contract.md` against `protocol.md`.
- Add codec negotiation to `hello` and `link_open`.
- Define platform profiles and honest capability/carve-out reporting.
- Add JSON Schemas for the control protocol, scenario files, and result files.
- Pin deterministic error categories and replay-command format.

Gate: malformed peer output, unknown capability values, missing required
channels, and unsupported contract versions all fail closed.

### Phase 1 — orchestrator plus an intentionally independent reference peer

Implemented in `src/lazily-interop`:

- standalone Rust CLI with no `lazily-rs` dependency;
- deadline-enforced NDJSON process supervision;
- sibling `lazily-spec` schema validation;
- deterministic semantic relay and canonical expected snapshots;
- JSON, JUnit, and console reporting;
- real Unix IPC length-prefix link used by the framing mutation;
- an independent reference peer and eight-of-eight mutation gate.

- Implement process supervision, schema validation, semantic relay, reports, and
  replay.
- Implement a minimal reference peer that exists only to test the harness.
- Mutation-check the harness: wrong frontier object, omitted null key, reversed
  stamp comparison, false `applied` count, truncated frame, merged frames,
  swallowed unsupported channel, and non-flushed control output must each fail
  with the expected culprit.

Gate: the suite proves it can go red for each defect class before any production
binding is credited.

### Phase 2 — first cross-language pair

- Add Rust and Zig peers.
- Run canonical JSON and msgpack semantic scenarios in both directions.
- Add one direct IPC-stream link and the loopback WebRTC data-channel contract.
- Replace any binding-local mirror assertion with the shared scenario where the
  suite now owns the behavior.

Gate: `rs → zig` and `zig → rs` both pass, and mutations to either encoder or
decoder are attributed to the mutated peer.

### Phase 3 — all implemented bindings

Add Python, Kotlin, JavaScript, Dart, Go, and C++ peers using the rollout plan.
Each joins first against both Rust and Zig, then the scheduled all-pairs matrix.

Gate: every one of the eight bindings that currently ships the state plane
passes all semantic scenarios and every applicable required channel profile.

### Phase 4 — C# joins without a paper adapter

C# joins only after `#lzcsipc` and `#lzcssync` ship the real wire and runtime
surfaces. The peer MUST wrap Lazily library code; it may not reimplement the
protocol inside the test executable.

Until then, the registry reports C# as staged and excludes it from CRDT feature
groups. That is neither a failure nor a pass for C#; the full nine-binding
completion criterion remains open.

Gate: C# passes the same mutation probes and network scenarios with no
binding-specific fixture relaxation.

### Phase 5 — full channel matrix and CI

Implement the channel plan, then install three CI tiers:

- PR smoke: schema checks, harness mutations, a deterministic spanning set of
  cross-language pairs, and all cheap loopback channels;
- merge gate: every binding at least once as sender and receiver for each
  applicable required channel and codec;
- nightly: all 36 binding pairs, both directions, all channel profiles,
  randomized deterministic seeds, reconnect/backpressure cases, and real WebRTC
  stacks where available.

The nightly result publishes a coverage matrix. An unsupported required cell is
red, not skipped green.

## Completion criteria

- One command launches the full suite from a clean checkout.
- All nine peers implement one versioned control contract.
- Canonical JSON and required msgpack run cross-language, not self-round-trip.
- Every spec channel has a direct-link scenario and a declared platform profile.
- Every binding is exercised as both encoder and decoder.
- Every red identifies the plane, pair, channel, codec, seed, and first
  divergence and provides a local replay command.
- No scenario passes when zero frames, zero peers, or zero required channels ran.
- The full matrix is enforced in CI with machine-readable historical results.

## Explicit non-goals

- Testing language-local reactive compute closures over the wire.
- Making the orchestrator a production relay.
- Requiring every runtime to bundle a full ICE/DTLS stack; the portable WebRTC
  contract is mandatory, while real-stack smoke follows declared platform
  adapters.
- Replacing the cheap per-binding canonical byte/fixture gate. The network suite
  supplements it with cross-implementation interpretation and transport proof.
