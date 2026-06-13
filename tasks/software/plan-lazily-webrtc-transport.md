# Plan: WebRTC DataChannel IPC transport for lazily-rs (#jpf1 / #webrtc1–3)

Design for a peer-to-peer WebRTC **DataChannel** transport that carries the
lazily IPC wire protocol (`Snapshot`/`Delta`) between agents, building on the
**already-implemented** signaling layer. `#webrtc1` = this design; `#webrtc2` =
implementation; `#webrtc3` = tests + bench.

## What already exists (don't rebuild)

- `src/signaling_client.rs` — async `SignalingClient` with `offer` / `answer` /
  `ice` over a relay (`ClientMessage`/`ServerMessage` carry SDP + ICE strings).
  This is the **rendezvous half**; it does NOT open a media/data path.
- `src/ipc.rs` — the wire protocol and the transport contract:
  - `trait IpcSink { fn send(&mut self, &IpcMessage) -> Result<(),Error>; … }`
    (+ `send_snapshot`/`send_delta` defaults). **Synchronous.**
  - `trait IpcSource { fn recv(&mut self) -> Result<Option<IpcMessage>,Error>; }`
    — `Ok(None)` = currently empty/closed. **Synchronous.**
  - `IpcMessage` (Snapshot/Delta/…) is already serde-serializable (JSON via
    `serde`, or binary via `postcard` under `ipc-binary`).
  - `Snapshot::filter_readable(&PeerPermissions, peer)` already exists.
- `src/distributed.rs` — `PeerId`, `PeerPermissions`, `RemoteOp` (per-peer
  read/write/trigger allowlist).

## The core design tension: sync trait ↔ async WebRTC

`IpcSink`/`IpcSource` are synchronous; `webrtc` + DataChannel are async/tokio.
**Bridge with channels, don't change the traits** (keeping the traits sync lets
the WebRTC transport be a drop-in alongside the in-process/socket transports):

```
IpcSink::send  --(mpsc::UnboundedSender, non-blocking)-->  async writer task --> DataChannel.send
DataChannel.on_message --> async reader task --(mpsc::UnboundedSender)--> inbound queue
IpcSource::recv --(try_recv, Ok(None) when empty)-------------------------/
```

- `WebRtcTransport` owns: an `RTCPeerConnection`, one ordered+reliable
  `RTCDataChannel`, an outbound `mpsc` (drained by a writer task), an inbound
  `mpsc` (filled by the channel's `on_message`), the local `PeerPermissions`, and
  the remote `PeerId`.
- `WebRtcSink` (impl `IpcSink`): `send` serializes the `IpcMessage` and
  `unbounded_send`s the bytes — non-blocking, never awaits, so it satisfies the
  sync trait. `send_snapshot` applies `filter_readable` for the peer first.
- `WebRtcSource` (impl `IpcSource`): `recv` does `try_recv` on the inbound queue →
  `Ok(Some(msg))` / `Ok(None)` (empty) / `Err` (channel closed = peer gone).
- A separate async constructor (`WebRtcTransport::connect`) runs the signaling
  handshake and resolves once the DataChannel is `open`; the sync sink/source
  handles are cheap `Clone`able split halves.

## Connection lifecycle

1. **Offerer**: create `RTCPeerConnection`, `create_data_channel("lazily-ipc",
   {ordered:true})`, `create_offer`, set local desc, send via
   `SignalingClient::offer`; trickle ICE via `SignalingClient::ice`.
2. **Answerer**: on `ServerMessage::Offer`, set remote desc, `create_answer`, send
   via `answer`; accept the peer's data channel through `on_data_channel`.
3. Both: feed inbound `ServerMessage::Ice` into `add_ice_candidate`.
4. `on_open` → resolve `connect()`; `on_message` → inbound queue;
   `on_close`/ICE `Failed`/`Disconnected` → mark transport closed (sink/source
   start returning `Err`).
5. **Reconnect** is caller-driven: drop the transport and call `connect` again
   (re-runs signaling). Document that the lazily graph re-syncs by sending a fresh
   `Snapshot` after reconnect (deltas are not replayed across a reconnect).

## Crate choice (decide in #webrtc2)

- **`webrtc` (webrtc-rs)** — full pure-Rust stack (ICE/DTLS/SCTP/DataChannel),
  tokio-native, batteries-included DataChannel API. **Recommended default** for a
  first transport: least wiring, async-native, matches the channel design above.
- **`str0m`** — sans-IO (you own sockets + timers). Lighter/more control, no forced
  async runtime, but significantly more wiring. Note as the alternative if binary
  size / runtime independence becomes a constraint. (Both compile to native only;
  this transport is host-side, not in-zome — consistent with the lazily WASM
  story.)

## Feature gating

- New `webrtc = ["dep:webrtc", "dep:tokio", "serde", "signaling-client"]` Cargo
  feature (reuses the existing async/serde/signaling surface). Native-only.
- `WebRtcTransport`/`WebRtcSink`/`WebRtcSource` live behind `#[cfg(feature =
  "webrtc")]` in a new `src/webrtc_transport.rs`.

## Security / permissions

- Outbound: `send_snapshot` MUST `filter_readable` for the remote peer; `send_delta`
  filters per-op via `PeerPermissions` before enqueue (drop ops the peer can't
  read). Inbound writes are checked against `PeerPermissions` (`RemoteOp::write`)
  before applying — the transport is a carrier, the permission gate is mandatory.
- DTLS (built into WebRTC) gives transport encryption; signaling only exchanges
  SDP/ICE, never graph data.

## #webrtc3 test + bench plan

- **Loopback integration test** (two `WebRtcTransport`s in one process over a
  local signaling stub or in-memory signaling): assert snapshot sync, delta
  ordering (DataChannel ordered=true), permission-filtered routing (peer denied a
  node never receives it), and reconnect-after-drop re-syncs via fresh snapshot.
- **Criterion bench** vs the in-process and socket transports: 4 KB / 64 KB / 1 MB
  payload latency + throughput, so the WebRTC overhead is quantified before it's
  recommended for any path.

## Out of scope (here)

- TURN/relay fallback config, multi-DataChannel QoS lanes, and the Cloudflare
  signaling server itself (#yxjw, already built) — note as follow-ups, not blockers.
