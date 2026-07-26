# The interop peer contract

A binding participates in the interop network by shipping ONE executable — its
**interop peer**. The peer speaks line-delimited JSON on stdin/stdout. That stdio
link is the *control plane*: it is how the harness drives the peer, and it is
never the thing under test.

What IS under test splits in two, and keeping them apart is the main design
decision in this repo:

- **The semantic plane** — do two different implementations, given the same ops
  in different orders, reach the same state? Frames are relayed by the
  orchestrator, so delivery order, partitions and reordering are *script*, not
  scheduling. Fully deterministic.
- **The transport plane** — does a frame survive a real channel intact, and does
  that channel honour its own contract (framing, backpressure, reconnect,
  zero-copy)? Frames cross a real FFI boundary / arena / socket / WebSocket /
  data channel between two peers.

A WebRTC flake must never be able to fail a convergence assertion. If it can,
people stop trusting the convergence result, and a gate people learn to ignore is
worse than no gate. So a red always names a plane and a channel.

## Control-plane wire

One JSON object per line, request on stdin, response on stdout. Anything a peer
writes to stderr is captured and reported on failure; it is never parsed.

A peer MUST NOT buffer: it writes its response line and flushes before reading
the next request. A peer that exits before `bye` is a failure, and its stderr is
the diagnostic.

---

# Part 1 — semantic plane

### `hello`

```json
{"cmd": "hello", "peer": 1, "protocol_version": 1}
```

```json
{"ok": true, "binding": "lazily-rs", "version": "0.51.0", "protocol_version": 1, "features": ["distributed_crdt", "reliable_sync"], "codecs": ["json", "msgpack"], "channels": ["in_process_ffi", "shared_memory", "ipc_socket"], "channel_variants": {"ipc_socket": ["unix_stream_length_prefix"]}, "platform_profile": "unix", "carve_outs": []}
```

`peer` is this peer's `PeerId` for the whole run — the identity its plane stamps
ops with. The orchestrator assigns it; a peer that minted its own would make the
transcript non-reproducible.

`features` declares production surfaces the peer actually wraps:
`distributed_crdt`, `reliable_sync`, `causal_receipts`, `signaling`, and
`transport_links`. A feature scenario forms its peer group only from bindings
that advertise the feature. A binding that does not implement the feature yet
is reported as **staged** and receives no green credit for that scenario.

`codecs` and `channels` declare which wire codecs and transports the peer
implements. `channel_variants` names concrete implementations of each advertised
channel. **Declaring fewer is always safe; declaring one you do not implement is
a hard failure.** `platform_profile` is one of `portable`, `unix`, `windows`, or
`wasm`; `carve_outs` records honest platform limitations. Unknown values fail
closed.

The harness never silently skips a channel an eligible peer group was required
to run — see "No vacuous channels" below.

Reject a `protocol_version` you do not implement rather than guessing:
`{"ok": false, "error": "unsupported protocol_version 2"}`.

### `local_set`

```json
{"cmd": "local_set", "node": 1, "key": "doc/title", "state": {"Inline": [65]}, "at": 10}
```

Apply a local write and return the frame you would broadcast. `at` is the logical
wall-time to stamp with — the peer MUST use it and MUST NOT read a real clock.
`key` may be `null`.

```json
{"ok": true, "frame": {"CrdtSync": {"frontier": [[1, {"wall_time": 10, "logical": 0, "peer": 1}]], "ops": [...]}}}
```

The orchestrator validates the frame against `schemas/distributed.json` *before*
delivering it anywhere, so a malformed frame fails at the emitting peer. That is
what makes a red name a culprit.

### `deliver`

```json
{"cmd": "deliver", "frame": {"CrdtSync": {...}}, "at": 11}
```

```json
{"ok": true, "applied": 2}
```

Re-delivering a frame already seen MUST report `"applied": 0`. This is the
state-based CvRDT idempotence claim, and it is the one thing a peer cannot fake
by converging correctly.

### `snapshot`

```json
{"cmd": "snapshot"}
```

The converged view, entries sorted by `node` then by `key` (`null` first), so two
peers that agree produce identical responses. MUST NOT mutate anything — it is
called repeatedly, including between deliveries.

```json
{"ok": true, "cells": [{"node": 1, "key": "doc/title", "state": {"Inline": [66]}}]}
```

### `bye`

Respond `{"ok": true}`, then exit 0.

---

# Part 2 — transport plane

A **link** is one channel instance connecting exactly two peers. The orchestrator
opens it, tells each side its role, then asks one side to send and the other to
receive. The orchestrator never carries the payload — that is the point.

### `link_open`

```json
{"cmd": "link_open", "link": 1, "channel": "ipc_socket", "codec": "msgpack", "role": "listen", "endpoint": "/tmp/lazily-interop-xyz/link1.sock", "options": {}}
```

`codec` MUST be one advertised by both peers. `role` is `"listen"` or
`"connect"`; the orchestrator opens the listener first and waits for its `ok`
before opening the connector. `endpoint` is channel-specific (see below). A peer
that does not implement `channel` MUST answer
`{"ok": false, "error": "unsupported channel", "unsupported": true}` — the
`unsupported` flag is what lets the harness report an honest gap instead of a
crash or a pass.

### `link_send`

```json
{"cmd": "link_send", "link": 1, "frame": {"CrdtSync": {...}}}
```

Encode the frame with this binding's own codec and put the bytes on the channel.
Response `{"ok": true, "bytes": 214}` — the on-wire byte count, which is how the
harness detects a channel that silently truncated or re-framed.

### `link_recv`

```json
{"cmd": "link_recv", "link": 1, "timeout_ms": 2000, "expect": 1}
```

Block for up to `timeout_ms` for `expect` whole frames, decode each with this
binding's codec, and return them:

```json
{"ok": true, "frames": [{"CrdtSync": {...}}], "bytes": 214}
```

Returning fewer than `expect` frames is a failure, not a partial success — a
channel that loses or merges messages is exactly what this plane exists to catch.

### `link_close`

```json
{"cmd": "link_close", "link": 1}
```

### `link_stats` (optional, channel-dependent)

```json
{"ok": true, "buffered_bytes": 0, "copies": 0, "reconnects": 1}
```

Used by backpressure and zero-copy assertions. A peer that cannot report a
statistic omits the key; the harness treats a *missing* key as "not asserted" and
a *wrong* value as a failure, so omitting is honest and lying is not.

---

## The channels

| `channel` | `endpoint` | What crosses | What ONLY this channel can break |
|---|---|---|---|
| `in_process_ffi` | ignored | C ABI structs (`schemas/ffi.json`) | struct layout, ownership/lifetime across the boundary, `LazilyFfiBytes` free discipline, double-free and leak on the callee side |
| `shared_memory` | arena path | `IpcValue.SharedBlob` refs into a mapped `ShmBlobArena` | offset/len agreement, checksum, generation/epoch staleness, alignment, and whether the read was actually zero-copy |
| `ipc_socket` | unix socket path | length-prefixed `IpcMessage` frames | framing, partial reads, message-boundary preservation under coalescing |
| `websocket` | `ws://host:port/path` | signaling frames and relayed payloads | text vs binary framing, fragmentation, close/reconnect and the resync that follows |
| `webrtc_datachannel` | signaling endpoint | relayed `IpcMessage` over a `DataChannel` | ordered vs unordered delivery, message-size limit and chunking, `bufferedAmount` backpressure |

Two notes on scope. `in_process_ffi` is only meaningful for a peer that can host
*two* planes in one process and pass between them across the C ABI; a peer
declares it only if it can. And for `webrtc_datachannel`, what is portable across
nine bindings is the `DataChannel` *contract* — every binding already models it
(zig has `DataChannel` and `InMemoryDataChannel`), while the ICE/DTLS stack is a
platform adapter. The matrix therefore runs the loopback data channel everywhere
and a real-stack smoke set only where a real stack exists.

## No vacuous channels

A scenario first names the feature whose peer group it exercises. Unsupported
bindings remain visible as staged and are excluded from that group. C# therefore
does not join CRDT scenarios until its real IPC and CRDT surfaces ship.

The scenario then names the channels its eligible peer group must run on. If a
peer in that group declares it does not implement one, the run records
`unsupported` **and says so in the result**. Under `--require-channels` — which
is what CI uses — that is a failure.

This is deliberate and it is the same rule as the conformance-coverage guard in
the bindings: a skip nobody asserts against reports green while testing nothing.
The harness will happily tell you that four of five channels ran. It will not
call that a pass. Likewise, a staged binding receives no pass for a scenario it
did not enter, and fewer than two eligible peers is an
`insufficient_eligible_peers` failure rather than a skipped green.

## Failure and replay contract

Every result pins one category from the result schema, including
`schema_violation`, `convergence`, `duplicate_applied`, `frame_count`,
`frame_truncated`, `unsupported_swallowed`, and `peer_timeout`. It also names the
plane and culprit and carries a shell-safe replay command with the registry,
scenario, spec checkout, seed, and strict-channel mode.

## What the orchestrator asserts

**Semantic plane**

1. Every emitted frame validates against `schemas/distributed.json`.
2. All peers converge: after the transcript drains, every peer's `snapshot` is
   equal. This is the assertion a static fixture cannot make, because it is a
   property of two *different* implementations reaching the same state.
3. A re-delivered frame applies 0 ops.
4. A scenario marked `order_independent` is replayed with per-peer delivery
   orders reversed and must reach the same converged snapshot.

**Transport plane**

5. A frame sent over a link arrives structurally equal to the frame sent —
   compared after decoding, so an encoder/decoder pair that is symmetrically
   wrong is caught by the *peer on the other side*, not by itself.
6. `n` frames sent arrive as `n` frames, never merged or split.
7. Channel-specific: `shared_memory` asserts the reader saw the writer's bytes
   without a copy and rejects a stale generation; `websocket` and
   `webrtc_datachannel` assert a reconnect resyncs rather than silently losing
   ops; `webrtc_datachannel` asserts a frame above the size limit is chunked and
   reassembled rather than dropped.

## Why the byte-level layer still comes first

The canonical fixture corpus already pins the bytes, and a binding that
round-trips against the canonical bytes interoperates *by construction*. That
layer is cheaper, deterministic, and catches more per unit of effort.

The reason to believe that is a real defect found in lazily-zig, which encoded
*and* decoded the stamp frontier as `{peer, stamp}` where the schema pins a
2-element `[peer, stamp]` tuple. It round-tripped against itself perfectly. This
harness catches that only when zig happens to be paired with another binding on
that exact type; comparing the encoder's output to the canonical bytes catches it
every time, at a fraction of the cost.

What the byte layer cannot express is *interpretation* and *transport*: two peers
can both encode a frame correctly and still disagree about which write wins, and
a perfectly-encoded frame can still be truncated by a channel that got its
framing wrong. That is this repo's job.

## Adding a binding

1. Implement Part 1 as an executable. Declare `"channels": []` at first.
   Advertise only production-backed `features`; an unfinished binding stays
   staged in the registry.
2. Register it in `peers.toml`:
   ```toml
   [peers.lazily-rs]
   command = ["cargo", "run", "--quiet", "--example", "interop_peer"]
   cwd = "../lazily-rs"
   ```
3. Green against `reference` on the semantic scenarios, then add channels one at
   a time.

Start **pairwise**. A nine-way red says nothing about which binding is wrong; a
pair names one of two.
