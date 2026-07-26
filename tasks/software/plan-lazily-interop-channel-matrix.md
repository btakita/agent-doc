# Plan: exercise every Lazily data channel in the interop suite

Parent: `tasks/software/plan-lazily-interop-network-suite.md`
Contract: `src/lazily-interop/docs/peer-contract.md`
Spec: `src/lazily-spec/protocol.md`

## Dimensions

Keep three dimensions explicit:

- **frame**: Snapshot, Delta, CrdtSync, ResyncRequest, OutboxAck, receipts,
  signaling;
- **codec**: canonical JSON, required cross-language msgpack, optional postcard
  or binding-local positional formats;
- **channel**: in-process FFI, shared memory, IPC stream, WebSocket, WebRTC data
  channel.

A codec is not a channel. A shared-memory payload reference still travels inside
an `IpcMessage` over a control channel. Reports therefore identify all three
dimensions and never collapse "msgpack over WebSocket" into a generic
"WebSocket passed".

## Required channel profiles

| Channel id | Variants | Required proof | Faults injected |
|---|---|---|---|
| `in_process_ffi` | host loads guest; both directions when both bindings expose a C ABI | ABI discriminants, owned buffer lifetime, decode/re-encode, free exactly once, panic/exception containment | invalid bytes, callee error, repeated free guard, large frame |
| `shared_memory` | POSIX shm/mmap; platform-native equivalent | writer stores bytes, receiver resolves `ShmBlobRef`, checksum/generation/epoch validated, zero-copy counter when exposed | stale generation, stale epoch, checksum mismatch, truncated arena |
| `ipc_socket` | Unix socket, named pipe, local TCP stream | length-prefix agreement, partial-read loop, frame-boundary preservation, reconnect | split prefix/body, coalesced frames, early close, oversized frame |
| `websocket` | Node/VM loopback everywhere; browser/runtime real stack where supported | binary/text policy, one message per frame, signaling, close/reconnect, resync | fragmentation, ping/control traffic, close mid-frame, reconnect |
| `webrtc_datachannel` | deterministic loopback contract everywhere; real ICE/DTLS smoke where a platform adapter exists | reliable ordered delivery, negotiated size, chunk/reassembly, buffered-amount backpressure, reconnect/resync | out-of-order loopback, dropped chunk, duplicate chunk, buffer saturation |

Platform profiles make carve-outs explicit:

- `native`: FFI, shared memory, IPC stream, WebSocket, loopback WebRTC required;
- `browser`: WebSocket and loopback/real WebRTC required; FFI/shared memory
  follow the spec capability declaration;
- `real-webrtc`: a smoke supplement, not a substitute for deterministic
  loopback;
- `sanitizer`: native plus ownership/leak instrumentation;
- `race`: native plus runtime race detector.

`--require-profile` fails if any required channel is unsupported. An
`unsupported` result is visible in exploratory reports but never becomes a
passing required cell.

## Common frame corpus

Every channel must carry the same minimum corpus in both directions:

1. empty Snapshot and non-empty Snapshot with keys, roots, and edges;
2. every DeltaOp variant;
3. keyless and keyed CrdtOp;
4. non-empty frontier with `[peer, WireStamp]` entries;
5. ResyncRequest and OutboxAck;
6. causal receipt sequence;
7. inline payload at 0, 1, boundary, and large sizes;
8. shared-blob reference where applicable;
9. unknown externally tagged variant (must reject);
10. malformed required/nullable distinction (`key` absent vs explicit null)
    according to the schema.

The sender's structured frame is schema-validated before send. The receiver
returns a canonical structured frame after decoding. Equality is structural
against the schema-normalized frame, never raw byte equality across different
codecs.

## Channel-specific scenarios

### In-process FFI

- Host A calls guest B and then B calls A.
- Cover all FFI message-kind discriminants.
- Assert returned allocation ownership and exactly one free.
- Repeat under ASan/Valgrind for C/C++/Rust/Zig hosts where available.
- A guest panic/exception returns an error status and never crosses the ABI.

### Shared memory

- Writer and reader are different processes.
- `SharedBlob` payload contents equal inline reference bytes.
- Reader rejects stale generation/epoch and bad checksum.
- Writer rotation does not let an old reference read new bytes.
- If a peer exposes `copies`, require zero payload copies after mapping; absence
  is reported as unverified, not green zero-copy.

### IPC streams

- Run Unix socket on Unix, named pipe on Windows, local TCP everywhere.
- Deliberately split a length prefix and body over several writes.
- Coalesce multiple complete frames into one write.
- Close after prefix, after partial body, and between frames.
- Reconnect and resume from the durable outbox cursor.

### WebSocket

- Exercise both sender directions and concurrent bidirectional traffic.
- Pin whether each codec uses binary or text frames.
- Fragment a large payload at the WebSocket layer.
- Interleave ping/pong and signaling frames without corrupting state frames.
- Close after an acknowledged epoch and before acknowledgement; both recovery
  paths must produce the same final snapshot.

### WebRTC data channel

- Deterministic loopback implementation is mandatory for every binding.
- Reliable/ordered is the conformance mode; unordered mode is a negative/fault
  source, not an alternate success contract.
- Send below, exactly at, and above the negotiated message-size limit.
- Above-limit payloads chunk and reassemble with integrity and frame identity.
- Saturate `bufferedAmount`; sender applies backpressure instead of dropping.
- Real ICE/DTLS runs use a local signaling server and no public network
  dependency in the required gate.

## Pair selection

For each matrix cell, run both `A → B` and `B → A`. The PR gate uses a
deterministic edge-cover so every binding appears as sender and receiver for
every required channel. The merge gate rotates the edge-cover by commit hash.
Nightly runs every one of the 36 binding pairs.

Do not run only `binding → Rust`. Schema validation catches malformed emission,
but pair rotation is required to expose decoder-specific incompatibilities and
reference-binding bugs.

## Timing and flake policy

- Semantic scenarios use logical clocks only.
- Transport deadlines are per operation and recorded.
- A timeout is a transport failure, never retried into a pass inside one test.
- CI may rerun the exact seed once only to classify reproducibility; the first
  failure remains visible.
- Real WebRTC smoke may be quarantined from the deterministic merge gate, but
  its capability cell remains non-green until a run passes.

## Result matrix

The JSON result schema records:

```text
binding_a, binding_b, direction, profile, plane, channel, variant,
codec, frame_family, scenario, seed, status, unsupported_reason,
bytes_sent, bytes_received, first_divergence, replay_command
```

The human report renders:

- binding-pair summary;
- channel/codec coverage;
- unsupported required cells;
- first failure per culprit;
- flaky/non-reproducible transport failures;
- historical trend keyed by binding commit SHAs.

## Completion criteria

- Every applicable required channel has at least one pairwise pass for every
  binding as sender and receiver in the merge gate.
- The nightly full matrix has no unsupported required cells.
- Every fault above has a mutation or simulation test that proves the harness
  goes red.
- Zero-copy, ownership, framing, reconnect, chunking, and backpressure claims
  are asserted only on the channel that can prove them.
- A transport red cannot change a semantic convergence result, and a semantic
  red cannot be hidden by retrying a transport.
