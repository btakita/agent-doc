// Distributed plane — WebRTC DataChannel IPC transport + the CRDT anti-entropy
// runtime (port of lazily-rs `webrtc_transport.rs` + `crdt_plane.rs`).
//
// This module is koffi-FREE: it is pure ESM and imports only the portable wire
// types (./index.js) and the HLC clock (./seq-crdt.js). "Real" WebRTC is reached
// through a platform adapter (RtcPeerChannel / RtcPeerConnector) that wraps the
// browser `RTCDataChannel` / `RTCPeerConnection` globals — no npm dependency and
// no import-time reference to those globals, so importing this module in Node
// never throws. Everything is testable over an in-memory loopback DataChannel.
//
// - `DataChannel` (documented interface) — one frame is one serialized
//   IpcMessage; `sendFrame`/`tryRecvFrame`/`isOpen`/`close`. Ordering and
//   reliability are the backend's job (a WebRTC channel opened `ordered: true`).
// - `WebRtcSink` — outbound IpcSink: permission-filters Snapshot/Delta/CrdtSync
//   via `filterReadable(permissions, peer)`, then encodes and sends the frame.
// - `WebRtcSource` — inbound IpcSource: pops a frame and decodes it verbatim.
//   Inbound write-permission enforcement is the graph-apply layer's job, NOT
//   done here (mirrors the Rust note).
// - `CrdtPlaneRuntime` — the live anti-entropy runtime: register cells,
//   localUpdate→CrdtOp, ingest→count (op-log dedup by (node, stamp)), and the
//   sync-frame pull protocol.

import { CrdtOp, CrdtSync, IpcMessage, IpcValue, WireStamp } from "./index.js";
import { Hlc } from "./seq-crdt.js";

// Lexicographic (wall_time, logical, peer) order over two WireStamps.
function compareWireStamp(a, b) {
  if (a.wallTime !== b.wallTime) return a.wallTime < b.wallTime ? -1 : 1;
  if (a.logical !== b.logical) return a.logical < b.logical ? -1 : 1;
  if (a.peer !== b.peer) return a.peer < b.peer ? -1 : 1;
  return 0;
}

function ipcValueEqual(a, b) {
  if (a === b) return true;
  if (a == null || b == null) return false;
  return JSON.stringify(a.toWire()) === JSON.stringify(b.toWire());
}

function assertPeerId(value, name = "peer") {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative safe integer`);
  }
  return value;
}

// ---------------------------------------------------------------------------
// WebRTC DataChannel transport
// ---------------------------------------------------------------------------

/**
 * Error raised by a WebRtcSink or WebRtcSource. `kind` is one of the frozen
 * taxonomy values: "Closed" (channel closed), "Encode" (frame serialization),
 * "Decode" (frame deserialization), "Channel" (underlying backend error).
 */
export const WebRtcTransportErrorKind = Object.freeze({
  Closed: "Closed",
  Encode: "Encode",
  Decode: "Decode",
  Channel: "Channel",
});

export class WebRtcTransportError extends Error {
  constructor(kind, cause = null) {
    const detail = cause && cause.message ? `: ${cause.message}` : "";
    super(`WebRTC transport error [${kind}]${detail}`);
    this.name = "WebRtcTransportError";
    this.kind = kind;
    if (cause !== null) this.cause = cause;
  }

  static closed() {
    return new WebRtcTransportError(WebRtcTransportErrorKind.Closed);
  }

  static encode(cause) {
    return new WebRtcTransportError(WebRtcTransportErrorKind.Encode, cause);
  }

  static decode(cause) {
    return new WebRtcTransportError(WebRtcTransportErrorKind.Decode, cause);
  }

  static channel(cause) {
    return new WebRtcTransportError(WebRtcTransportErrorKind.Channel, cause);
  }
}

/**
 * In-process loopback DataChannel for deterministic tests (no network, no WebRTC
 * stack). `InMemoryDataChannel.pair()` returns two cross-wired endpoints: a frame
 * sent on one is received (in order) on the other. Frames are Uint8Array.
 */
export class InMemoryDataChannel {
  #tx;
  #rx;
  #state;

  constructor(tx, rx, state) {
    this.#tx = tx;
    this.#rx = rx;
    this.#state = state;
  }

  static pair() {
    const aToB = [];
    const bToA = [];
    const state = { open: true };
    return [
      new InMemoryDataChannel(aToB, bToA, state),
      new InMemoryDataChannel(bToA, aToB, state),
    ];
  }

  sendFrame(frame) {
    this.#tx.push(frame);
  }

  tryRecvFrame() {
    return this.#rx.length > 0 ? this.#rx.shift() : null;
  }

  isOpen() {
    return this.#state.open;
  }

  close() {
    this.#state.open = false;
  }
}

/**
 * Permission-filtering IpcSink over a DataChannel. Every outbound
 * Snapshot/Delta/CrdtSync is filtered to what `peer` may read via
 * `filterReadable(permissions, peer)` before it is encoded and sent, so a peer
 * never receives graph state it is not entitled to.
 */
export class WebRtcSink {
  #channel;
  #permissions;
  #peer;

  constructor(channel, permissions, peer) {
    this.#channel = channel;
    this.#permissions = permissions;
    this.#peer = assertPeerId(peer);
  }

  get channel() {
    return this.#channel;
  }

  send(message) {
    if (!this.#channel.isOpen()) {
      throw WebRtcTransportError.closed();
    }
    let filtered;
    if (message.isSnapshot) {
      filtered = IpcMessage.snapshot(
        message.snapshot.filterReadable(this.#permissions, this.#peer),
      );
    } else if (message.isDelta) {
      filtered = IpcMessage.delta(
        message.delta.filterReadable(this.#permissions, this.#peer),
      );
    } else if (message.isCrdtSync) {
      filtered = IpcMessage.crdtSync(
        message.crdtSync.filterReadable(this.#permissions, this.#peer),
      );
    } else {
      // Reliable-sync control frames carry no node content; filtering is identity.
      filtered = message;
    }
    let frame;
    try {
      frame = filtered.encodeJson();
    } catch (error) {
      throw WebRtcTransportError.encode(error);
    }
    try {
      this.#channel.sendFrame(frame);
    } catch (error) {
      throw WebRtcTransportError.channel(error);
    }
  }
}

/**
 * IpcSource over a DataChannel. Delivers each decoded IpcMessage verbatim;
 * `recv()` returns null when no frame is pending and the channel is still open,
 * and throws a Closed WebRtcTransportError once the channel is closed and drained.
 */
export class WebRtcSource {
  #channel;

  constructor(channel) {
    this.#channel = channel;
  }

  get channel() {
    return this.#channel;
  }

  recv() {
    let frame;
    try {
      frame = this.#channel.tryRecvFrame();
    } catch (error) {
      throw WebRtcTransportError.channel(error);
    }
    if (frame !== null && frame !== undefined) {
      try {
        return IpcMessage.decodeJson(frame);
      } catch (error) {
        throw WebRtcTransportError.decode(error);
      }
    }
    if (this.#channel.isOpen()) {
      return null;
    }
    throw WebRtcTransportError.closed();
  }
}

// ---------------------------------------------------------------------------
// CRDT anti-entropy runtime
// ---------------------------------------------------------------------------

// Default state-based LWW register: the whole cell state is replaced by the
// op carrying the greatest WireStamp (lexicographic order). This is the model
// the distributed/anti_entropy_converge fixture exercises.
class LwwStateCell {
  constructor() {
    this.op = null;
  }

  // Merge a CrdtOp. Returns true iff the converged VALUE changed (a newer stamp
  // carrying identical bytes, or an older/equal stamp, is not a value change).
  merge(op) {
    if (this.op === null) {
      this.op = op;
      return true;
    }
    if (compareWireStamp(op.stamp, this.op.stamp) > 0) {
      const changed = !ipcValueEqual(op.state, this.op.state);
      this.op = op;
      return changed;
    }
    return false;
  }

  get value() {
    return this.op ? this.op.state : undefined;
  }
}

function dedupKey(node, stamp) {
  return `${node}|${stamp.wallTime}|${stamp.logical}|${stamp.peer}`;
}

// Base node id for entries a family materializes on first remote observation
// (#lzfamilysync). Family entry nodes are locally-private — keyed ops resolve by
// key string, never by raw node id — so this only needs to avoid colliding with
// application-assigned node ids; the runtime skips any id already in use.
const FAMILY_NODE_BASE = 2 ** 48;

/**
 * The live runtime that folds distributed CRDT anti-entropy frames into a set of
 * replicated root cells (port of lazily-rs `CrdtPlaneRuntime`). One runtime per
 * shared session per replica.
 *
 * - `register(node, key?, cell?)` — register a root cell under `node`, optionally
 *   projecting a wire-stable `key`. `cell` defaults to a state-based LWW register;
 *   a custom cell must implement `merge(op) -> boolean` and a `value` getter.
 * - `localUpdate(node, nowMicros, state)` — apply a local edit; ticks the plane
 *   clock, stamps a fresh WireStamp, records the op, and returns the CrdtOp to
 *   broadcast (or null when the value did not change / node is unknown).
 * - `ingest(sync, nowMicros)` — fold every not-yet-seen CrdtOp exactly once
 *   (op-log dedup by (node, stamp)) and advance the frontier/membership. Returns
 *   the count of newly-applied ops; re-ingesting a frame applies 0 (idempotent).
 * - `syncFrame()` / `syncFrameSince(frontier)` / `syncReply(request)` — the
 *   frontier-advertising pull protocol.
 */
export class CrdtPlaneRuntime {
  #peer;
  #hlc;
  #cells = new Map();
  #log = new Set();
  #ops = [];
  #keyToNode = new Map();
  #nodeToKey = new Map();
  #frontier = new Map();
  #membership = new Set();

  // -- Family sync (#lzfamilysync) -----------------------------------------
  #families = new Set();
  #familyMembers = new Map();
  #familyEpoch = 0;
  #nextFamilyNode = FAMILY_NODE_BASE;

  constructor(peer) {
    this.#peer = assertPeerId(peer);
    this.#hlc = new Hlc(this.#peer);
  }

  get peer() {
    return this.#peer;
  }

  // Number of registered / auto-created cells.
  get size() {
    return this.#cells.size;
  }

  isEmpty() {
    return this.#cells.size === 0;
  }

  register(node, key = null, cell = null) {
    assertPeerId(node, "node");
    if (key !== null && key !== undefined) {
      this.#keyToNode.set(key, node);
      this.#nodeToKey.set(node, key);
    }
    this.#cells.set(node, cell ?? new LwwStateCell());
    return this;
  }

  #cellFor(node) {
    let cell = this.#cells.get(node);
    if (cell === undefined) {
      cell = new LwwStateCell();
      this.#cells.set(node, cell);
    }
    return cell;
  }

  #resolveNode(op) {
    if (op.key !== null && this.#keyToNode.has(op.key)) {
      return this.#keyToNode.get(op.key);
    }
    if (op.key !== null) {
      this.#keyToNode.set(op.key, op.node);
      this.#nodeToKey.set(op.node, op.key);
    }
    return op.node;
  }

  #observeStamp(stamp) {
    if (stamp.peer !== this.#peer) {
      this.#membership.add(stamp.peer);
    } else {
      this.#membership.add(this.#peer);
    }
    const current = this.#frontier.get(stamp.peer);
    if (current === undefined || compareWireStamp(stamp, current) > 0) {
      this.#frontier.set(stamp.peer, stamp);
    }
  }

  // The converged IpcValue at `node`, or undefined.
  value(node) {
    return this.#cells.get(node)?.value;
  }

  // -- Family sync (#lzfamilysync) -----------------------------------------

  // Register a last-writer-wins family under `namespace`. An inbound keyed op whose
  // first key segment matches materializes a fresh entry on `ingest` (instead of
  // being dropped/mis-addressed), so membership propagates and a derived aggregate
  // over the family converges.
  registerFamilyLww(namespace) {
    this.#families.add(namespace);
    if (!this.#familyMembers.has(namespace)) this.#familyMembers.set(namespace, []);
    return this;
  }

  // The membership signal (#lzfamilysync), bumped whenever a family entry
  // materializes — a derived aggregate over the family reads it so a remote-added
  // key forces a recompute. A monotonically-increasing counter.
  membershipEpoch() {
    return this.#familyEpoch;
  }

  // The materialized keys of family `namespace`, in first-materialization order.
  familyKeys(namespace) {
    return [...(this.#familyMembers.get(namespace) ?? [])];
  }

  // The current converged boolean value of family entry `namespace/keySuffix`.
  familyValueLww(namespace, keySuffix) {
    const node = this.#keyToNode.get(`${namespace}/${keySuffix}`);
    if (node === undefined) return undefined;
    const iv = this.value(node);
    if (iv === undefined || iv.bytes === undefined) return undefined;
    return iv.bytes.length > 0 && iv.bytes[0] !== 0;
  }

  // Insert or update local LWW family entry `namespace/keySuffix` to boolean
  // `value`, returning the CrdtOp to broadcast (or null for a value-preserving
  // update). Materializes the entry (and bumps membership) on first insert.
  familySetLww(namespace, keySuffix, value, nowMicros) {
    const key = `${namespace}/${keySuffix}`;
    let node = this.#keyToNode.get(key);
    if (node === undefined) {
      node = this.#mintFamilyNode();
      this.register(node, key);
      this.#recordFamilyMember(namespace, key);
      this.#bumpFamilyEpoch();
    }
    return this.localUpdate(node, nowMicros, IpcValue.inline(Uint8Array.of(value ? 1 : 0)));
  }

  #mintFamilyNode() {
    for (;;) {
      const candidate = this.#nextFamilyNode;
      this.#nextFamilyNode += 1;
      if (!this.#cells.has(candidate)) return candidate;
    }
  }

  #recordFamilyMember(namespace, key) {
    const members = this.#familyMembers.get(namespace) ?? [];
    if (!members.includes(key)) members.push(key);
    this.#familyMembers.set(namespace, members);
  }

  #bumpFamilyEpoch() {
    this.#familyEpoch += 1;
  }

  // If `op` is a keyed op for a registered family whose entry is not yet known,
  // materialize it under a fresh locally-private node (indexed by the wire key) and
  // return that node; otherwise fall back to the normal node resolution.
  #resolveNodeForFamily(op) {
    if (op.key !== null && op.key !== undefined && !this.#keyToNode.has(op.key)) {
      const namespace = String(op.key).split("/")[0];
      if (this.#families.has(namespace)) {
        const node = this.#mintFamilyNode();
        this.#keyToNode.set(op.key, node);
        this.#nodeToKey.set(node, op.key);
        this.#recordFamilyMember(namespace, op.key);
        this.#bumpFamilyEpoch();
        return node;
      }
    }
    return this.#resolveNode(op);
  }

  // The winning CrdtOp at `node`, or undefined.
  winningOp(node) {
    const cell = this.#cells.get(node);
    return cell instanceof LwwStateCell ? (cell.op ?? undefined) : undefined;
  }

  // Registered node ids, ascending.
  nodes() {
    return [...this.#cells.keys()].sort((a, b) => a - b);
  }

  // Converged {node, key, state} projection (state as wire JSON), ascending by
  // node — the shape the anti_entropy_converge fixture asserts.
  converged() {
    const out = [];
    for (const node of this.nodes()) {
      const op = this.winningOp(node);
      if (op === undefined) continue;
      const entry = { node, state: op.state.toWire() };
      if (op.key !== null) entry.key = op.key;
      out.push(entry);
    }
    return out;
  }

  localUpdate(node, nowMicros, state) {
    if (!this.#cells.has(node)) {
      return null;
    }
    const hlc = this.#hlc.send(nowMicros);
    const wire = new WireStamp({
      wallTime: hlc.wallTime,
      logical: hlc.logical,
      peer: hlc.peer,
    });
    const key = this.#nodeToKey.get(node) ?? null;
    const op =
      key !== null
        ? CrdtOp.keyed(node, key, wire, state)
        : new CrdtOp(node, wire, state);
    const cell = this.#cellFor(node);
    const changed = cell.merge(op);
    if (!changed) {
      return null;
    }
    this.#log.add(dedupKey(node, wire));
    this.#ops.push(op);
    this.#observeStamp(wire);
    return op;
  }

  ingest(sync, nowMicros) {
    for (const entry of sync.frontier) {
      const stamp = entry.stamp;
      if (stamp.peer !== this.#peer) {
        this.#hlc.recv(stamp, nowMicros);
      }
      this.#observeStamp(stamp);
    }
    let applied = 0;
    for (const op of sync.ops) {
      const key = dedupKey(op.node, op.stamp);
      if (this.#log.has(key)) {
        continue;
      }
      this.#log.add(key);
      this.#ops.push(op);
      applied += 1;
      if (op.stamp.peer !== this.#peer) {
        this.#hlc.recv(op.stamp, nowMicros);
      }
      this.#observeStamp(op.stamp);
      // Materialize-on-ingest (#lzfamilysync): a keyed op for a registered family
      // whose entry is not yet known materializes it under a fresh local node
      // instead of being mis-addressed to a colliding local family node.
      const node = this.#resolveNodeForFamily(op);
      this.#cellFor(node).merge(op);
    }
    return applied;
  }

  // Per-peer highest observed WireStamp, ascending by peer, as {peer, stamp}.
  frontierEntries() {
    return [...this.#frontier.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([peer, stamp]) => ({ peer, stamp }));
  }

  // Wire form of the frontier: [[peer, WireStamp.toWire()], ...].
  wireFrontier() {
    return this.frontierEntries().map((entry) => [
      entry.peer,
      entry.stamp.toWire(),
    ]);
  }

  // Peers observed in this session (self included once local edits exist).
  membership() {
    return [...this.#membership].sort((a, b) => a - b);
  }

  membershipCount() {
    return this.#membership.size;
  }

  // A frame shipping the entire op log plus this replica's frontier. Safe to
  // resend (the receiver dedups).
  syncFrame() {
    return new CrdtSync({ frontier: this.frontierEntries(), ops: [...this.#ops] });
  }

  // A frame shipping only the ops a peer described by `since` has not observed.
  // `since` is an iterable of {peer, stamp} or [peer, WireStamp] frontier entries.
  syncFrameSince(since) {
    const watermark = this.#watermarkOf(since);
    const ops = this.#ops.filter((op) => {
      const seen = watermark.get(op.stamp.peer);
      return seen === undefined || compareWireStamp(op.stamp, seen) > 0;
    });
    return new CrdtSync({ frontier: this.frontierEntries(), ops });
  }

  // Reply to a peer's anti-entropy `request` (a CrdtSync): ship exactly the ops
  // the requester (described by `request.frontier`) is missing.
  syncReply(request) {
    return this.syncFrameSince(request.frontier);
  }

  #watermarkOf(since) {
    const watermark = new Map();
    for (const entry of since ?? []) {
      let peer;
      let stamp;
      if (Array.isArray(entry)) {
        peer = entry[0];
        stamp =
          entry[1] instanceof WireStamp ? entry[1] : WireStamp.fromWire(entry[1]);
      } else {
        peer = entry.peer;
        stamp =
          entry.stamp instanceof WireStamp
            ? entry.stamp
            : WireStamp.fromWire(entry.stamp);
      }
      watermark.set(peer, stamp);
    }
    return watermark;
  }
}

// ---------------------------------------------------------------------------
// Browser WebRTC platform adapter (constructed lazily; importing never throws)
// ---------------------------------------------------------------------------

/** Whether the browser WebRTC globals are present in this environment. */
export function isWebRtcAvailable() {
  return (
    typeof RTCPeerConnection !== "undefined" &&
    typeof RTCDataChannel !== "undefined"
  );
}

const textEncoder = new TextEncoder();

/**
 * Adapter that wraps a browser `RTCDataChannel` as a `DataChannel`. Inbound
 * messages are buffered so `tryRecvFrame` stays non-blocking. Frames are
 * Uint8Array; the channel `binaryType` is forced to "arraybuffer".
 */
export class RtcPeerChannel {
  #dc;
  #inbox = [];
  #open;

  constructor(dataChannel) {
    this.#dc = dataChannel;
    this.#open = dataChannel.readyState === "open";
    dataChannel.binaryType = "arraybuffer";
    dataChannel.addEventListener("open", () => {
      this.#open = true;
    });
    dataChannel.addEventListener("close", () => {
      this.#open = false;
    });
    dataChannel.addEventListener("message", (event) => {
      const data = event.data;
      if (data instanceof ArrayBuffer) {
        this.#inbox.push(new Uint8Array(data));
      } else if (typeof data === "string") {
        this.#inbox.push(textEncoder.encode(data));
      } else if (ArrayBuffer.isView(data)) {
        this.#inbox.push(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
      } else {
        this.#inbox.push(new Uint8Array(data));
      }
    });
  }

  sendFrame(frame) {
    this.#dc.send(frame);
  }

  tryRecvFrame() {
    return this.#inbox.length > 0 ? this.#inbox.shift() : null;
  }

  isOpen() {
    return this.#open;
  }

  close() {
    this.#dc.close();
  }
}

/**
 * Thin helper that drives an `RTCPeerConnection` offer/answer/ICE handshake via
 * a `SignalingClient` (from ./signaling.js). Constructed lazily: the constructor
 * throws if the WebRTC globals are absent, so a Node import of this module never
 * evaluates them.
 *
 * Typical use: the initiator calls `createDataChannel(label)` then
 * `createOffer(to)`; the responder feeds forwarded `offer`/`answer`/`ice`
 * server frames into `acceptOffer` / `acceptAnswer` / `addIceCandidate`.
 */
export class RtcPeerConnector {
  #client;
  #remote;
  #pc;

  constructor(signalingClient, { rtcConfig, remote = null } = {}) {
    if (typeof RTCPeerConnection === "undefined") {
      throw new Error("RTCPeerConnection is not available in this environment");
    }
    this.#client = signalingClient;
    this.#remote = remote;
    this.#pc = new RTCPeerConnection(rtcConfig);
    this.#pc.addEventListener("icecandidate", (event) => {
      if (event.candidate && this.#remote !== null) {
        this.#client.ice(this.#remote, JSON.stringify(event.candidate));
      }
    });
  }

  get connection() {
    return this.#pc;
  }

  set remote(peer) {
    this.#remote = peer;
  }

  // Create a `DataChannel`-shaped adapter over a fresh ordered/reliable channel.
  createDataChannel(label = "lazily", options = {}) {
    const dc = this.#pc.createDataChannel(label, { ordered: true, ...options });
    return new RtcPeerChannel(dc);
  }

  // Await inbound remote channels (returns a Promise<RtcPeerChannel>).
  onDataChannel() {
    return new Promise((resolve) => {
      this.#pc.addEventListener(
        "datachannel",
        (event) => resolve(new RtcPeerChannel(event.channel)),
        { once: true },
      );
    });
  }

  async createOffer(to) {
    this.#remote = to;
    const offer = await this.#pc.createOffer();
    await this.#pc.setLocalDescription(offer);
    this.#client.offer(to, offer.sdp);
    return offer;
  }

  async acceptOffer(from, sdp) {
    this.#remote = from;
    await this.#pc.setRemoteDescription({ type: "offer", sdp });
    const answer = await this.#pc.createAnswer();
    await this.#pc.setLocalDescription(answer);
    this.#client.answer(from, answer.sdp);
    return answer;
  }

  async acceptAnswer(sdp) {
    await this.#pc.setRemoteDescription({ type: "answer", sdp });
  }

  async addIceCandidate(candidate) {
    const init =
      typeof candidate === "string" ? JSON.parse(candidate) : candidate;
    await this.#pc.addIceCandidate(init);
  }

  close() {
    this.#pc.close();
  }
}
