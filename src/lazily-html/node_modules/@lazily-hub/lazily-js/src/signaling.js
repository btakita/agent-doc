// Signaling plane — the WebRTC discovery/relay wire protocol + room routing +
// a client over a WebSocket-shaped seam (port of lazily-rs `signaling_client.rs`
// and `signaling/src/{protocol,room-core,client}.ts`).
//
// koffi-FREE, pure ESM. The signaling server brokers peer discovery for the
// distributed CRDT plane: peers join a session, learn the roster, and exchange
// the WebRTC SDP/ICE handshake (or relay opaque payloads). It never interprets
// CRDT state.
//
// Wire contract (matches lazily-spec/conformance/signaling/frames.json byte for
// byte): `type` tags are kebab-case; `peer` ids are bare JSON numbers
// <= 2^53-1; client-directed frames carry `to`; server-forwarded frames carry a
// server-stamped `from` (never client-supplied). A `join` frame OMITS
// `capabilities` from its JSON when absent.

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

function assertPeer(value, name = "peer") {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative safe integer <= 2^53-1`);
  }
  return value;
}

function assertString(value, name) {
  if (typeof value !== "string") {
    throw new TypeError(`${name} must be a string`);
  }
  return value;
}

function assertObject(value, name) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object`);
  }
  return value;
}

/** Frozen set of signaling error codes (schemas/signaling.json § ErrorCode). */
export const SignalingErrorCode = Object.freeze({
  BadMessage: "bad_message",
  NotJoined: "not_joined",
  AlreadyJoined: "already_joined",
  DuplicatePeer: "duplicate_peer",
  UnknownTarget: "unknown_target",
  PermissionDenied: "permission_denied",
});

/**
 * Error surfaced by the signaling client/room. `kind` is "protocol" (a frame
 * failed to decode) or "closed" (the transport closed).
 */
export class SignalingError extends Error {
  constructor(kind, message, cause = null) {
    super(message);
    this.name = "SignalingError";
    this.kind = kind;
    if (cause !== null) this.cause = cause;
  }

  static protocol(message, cause = null) {
    return new SignalingError("protocol", message, cause);
  }

  static closed(message = "signaling connection closed") {
    return new SignalingError("closed", message);
  }
}

// ---------------------------------------------------------------------------
// ClientMessage tagged union
// ---------------------------------------------------------------------------

export class ClientJoin {
  constructor(peer, capabilities = null) {
    this.type = "join";
    this.peer = assertPeer(peer);
    this.capabilities =
      capabilities === null || capabilities === undefined
        ? null
        : Object.freeze([...capabilities].map((c) => assertString(c, "capability")));
    Object.freeze(this);
  }

  toWire() {
    const wire = { type: "join", peer: this.peer };
    if (this.capabilities !== null) {
      wire.capabilities = [...this.capabilities];
    }
    return wire;
  }
}

export class ClientOffer {
  constructor(to, sdp) {
    this.type = "offer";
    this.to = assertPeer(to, "to");
    this.sdp = assertString(sdp, "sdp");
    Object.freeze(this);
  }

  toWire() {
    return { type: "offer", to: this.to, sdp: this.sdp };
  }
}

export class ClientAnswer {
  constructor(to, sdp) {
    this.type = "answer";
    this.to = assertPeer(to, "to");
    this.sdp = assertString(sdp, "sdp");
    Object.freeze(this);
  }

  toWire() {
    return { type: "answer", to: this.to, sdp: this.sdp };
  }
}

export class ClientIce {
  constructor(to, candidate) {
    this.type = "ice";
    this.to = assertPeer(to, "to");
    this.candidate = assertString(candidate, "candidate");
    Object.freeze(this);
  }

  toWire() {
    return { type: "ice", to: this.to, candidate: this.candidate };
  }
}

export class ClientRelay {
  constructor(to, payload) {
    this.type = "relay";
    this.to = assertPeer(to, "to");
    this.payload = payload;
    Object.freeze(this);
  }

  toWire() {
    return { type: "relay", to: this.to, payload: this.payload };
  }
}

export class ClientLeave {
  constructor() {
    this.type = "leave";
    Object.freeze(this);
  }

  toWire() {
    return { type: "leave" };
  }
}

export const ClientMessage = Object.freeze({
  join(peer, capabilities) {
    return new ClientJoin(peer, capabilities ?? null);
  },
  offer(to, sdp) {
    return new ClientOffer(to, sdp);
  },
  answer(to, sdp) {
    return new ClientAnswer(to, sdp);
  },
  ice(to, candidate) {
    return new ClientIce(to, candidate);
  },
  relay(to, payload) {
    return new ClientRelay(to, payload);
  },
  leave() {
    return new ClientLeave();
  },
  fromWire(value) {
    const object = assertObject(value, "ClientMessage");
    switch (object.type) {
      case "join":
        return new ClientJoin(object.peer, object.capabilities ?? null);
      case "offer":
        return new ClientOffer(object.to, object.sdp);
      case "answer":
        return new ClientAnswer(object.to, object.sdp);
      case "ice":
        return new ClientIce(object.to, object.candidate);
      case "relay":
        return new ClientRelay(object.to, object.payload);
      case "leave":
        return new ClientLeave();
      default:
        throw new TypeError(`unknown ClientMessage type: ${object.type}`);
    }
  },
});

// ---------------------------------------------------------------------------
// ServerMessage tagged union
// ---------------------------------------------------------------------------

export class ServerWelcome {
  constructor(peer, peers = []) {
    this.type = "welcome";
    this.peer = assertPeer(peer);
    this.peers = Object.freeze([...peers].map((p) => assertPeer(p, "roster peer")));
    Object.freeze(this);
  }

  toWire() {
    return { type: "welcome", peer: this.peer, peers: [...this.peers] };
  }
}

export class ServerPeerJoined {
  constructor(peer) {
    this.type = "peer-joined";
    this.peer = assertPeer(peer);
    Object.freeze(this);
  }

  toWire() {
    return { type: "peer-joined", peer: this.peer };
  }
}

export class ServerPeerLeft {
  constructor(peer) {
    this.type = "peer-left";
    this.peer = assertPeer(peer);
    Object.freeze(this);
  }

  toWire() {
    return { type: "peer-left", peer: this.peer };
  }
}

export class ServerOffer {
  constructor(from, sdp) {
    this.type = "offer";
    this.from = assertPeer(from, "from");
    this.sdp = assertString(sdp, "sdp");
    Object.freeze(this);
  }

  toWire() {
    return { type: "offer", from: this.from, sdp: this.sdp };
  }
}

export class ServerAnswer {
  constructor(from, sdp) {
    this.type = "answer";
    this.from = assertPeer(from, "from");
    this.sdp = assertString(sdp, "sdp");
    Object.freeze(this);
  }

  toWire() {
    return { type: "answer", from: this.from, sdp: this.sdp };
  }
}

export class ServerIce {
  constructor(from, candidate) {
    this.type = "ice";
    this.from = assertPeer(from, "from");
    this.candidate = assertString(candidate, "candidate");
    Object.freeze(this);
  }

  toWire() {
    return { type: "ice", from: this.from, candidate: this.candidate };
  }
}

export class ServerRelay {
  constructor(from, payload) {
    this.type = "relay";
    this.from = assertPeer(from, "from");
    this.payload = payload;
    Object.freeze(this);
  }

  toWire() {
    return { type: "relay", from: this.from, payload: this.payload };
  }
}

export class ServerErrorMessage {
  constructor(code, message) {
    this.type = "error";
    this.code = assertString(code, "code");
    this.message = assertString(message, "message");
    Object.freeze(this);
  }

  toWire() {
    return { type: "error", code: this.code, message: this.message };
  }
}

export const ServerMessage = Object.freeze({
  welcome(peer, peers) {
    return new ServerWelcome(peer, peers ?? []);
  },
  peerJoined(peer) {
    return new ServerPeerJoined(peer);
  },
  peerLeft(peer) {
    return new ServerPeerLeft(peer);
  },
  offer(from, sdp) {
    return new ServerOffer(from, sdp);
  },
  answer(from, sdp) {
    return new ServerAnswer(from, sdp);
  },
  ice(from, candidate) {
    return new ServerIce(from, candidate);
  },
  relay(from, payload) {
    return new ServerRelay(from, payload);
  },
  error(code, message) {
    return new ServerErrorMessage(code, message);
  },
  fromWire(value) {
    const object = assertObject(value, "ServerMessage");
    switch (object.type) {
      case "welcome":
        return new ServerWelcome(object.peer, object.peers ?? []);
      case "peer-joined":
        return new ServerPeerJoined(object.peer);
      case "peer-left":
        return new ServerPeerLeft(object.peer);
      case "offer":
        return new ServerOffer(object.from, object.sdp);
      case "answer":
        return new ServerAnswer(object.from, object.sdp);
      case "ice":
        return new ServerIce(object.from, object.candidate);
      case "relay":
        return new ServerRelay(object.from, object.payload);
      case "error":
        return new ServerErrorMessage(object.code, object.message);
      default:
        throw new TypeError(`unknown ServerMessage type: ${object.type}`);
    }
  },
});

// ---------------------------------------------------------------------------
// Signaling permissions (open vs allowlist)
// ---------------------------------------------------------------------------

/**
 * Per-peer signaling admission gate. In "allowlist" mode this is default-deny;
 * in "open" mode every check passes.
 */
export class SignalingPermissions {
  #mode;
  #peers = new Map();

  constructor(mode = "open") {
    if (mode !== "open" && mode !== "allowlist") {
      throw new TypeError(`unknown signaling mode: ${mode}`);
    }
    this.#mode = mode;
  }

  get mode() {
    return this.#mode;
  }

  #grants(peer) {
    let grants = this.#peers.get(peer);
    if (grants === undefined) {
      grants = { join: false, targets: new Set() };
      this.#peers.set(peer, grants);
    }
    return grants;
  }

  allowJoin(peer) {
    this.#grants(assertPeer(peer)).join = true;
    return this;
  }

  allowSignal(peer, target) {
    this.#grants(assertPeer(peer)).targets.add(assertPeer(target, "target"));
    return this;
  }

  allowMany(peer, targets) {
    const grants = this.#grants(assertPeer(peer));
    grants.join = true;
    for (const target of targets) grants.targets.add(assertPeer(target, "target"));
    return this;
  }

  revokePeer(peer) {
    return this.#peers.delete(peer);
  }

  // op is { kind: "join" } or { kind: "offer"|"answer"|"ice"|"relay", to }.
  isAllowed(peer, op) {
    if (this.#mode === "open") return true;
    const grants = this.#peers.get(peer);
    if (grants === undefined) return false;
    return op.kind === "join" ? grants.join : grants.targets.has(op.to);
  }
}

// ---------------------------------------------------------------------------
// SignalingRoom — transport-agnostic room routing
// ---------------------------------------------------------------------------

const DIRECTED = new Set(["offer", "answer", "ice", "relay"]);

/**
 * Transport-agnostic signaling room routing (port of `room-core.ts`). Feed a
 * decoded ClientMessage from a connection id and it returns the server frames to
 * deliver: `receive(connId, message) -> Array<{to: connId, message: ServerMessage}>`.
 *
 * Anti-spoof invariant: a forwarded frame's `from` is the SENDER's server-
 * registered peer id, never a client-supplied value. The `welcome` roster
 * excludes the joining peer and is sorted ascending.
 */
export class SignalingRoom {
  #byPeer = new Map();
  #byConn = new Map();
  #permissions;

  constructor({ mode = "open", permissions } = {}) {
    this.#permissions = permissions ?? new SignalingPermissions(mode);
  }

  roster() {
    return [...this.#byPeer.keys()].sort((a, b) => a - b);
  }

  size() {
    return this.#byPeer.size;
  }

  receive(connId, message) {
    const out = [];
    const emit = (to, msg) => out.push({ to, message: msg });
    switch (message.type) {
      case "join":
        this.#join(connId, message.peer, emit);
        break;
      case "leave":
        this.#leave(connId, emit);
        break;
      case "offer":
        this.#forward(connId, message.to, "offer", emit, (from) =>
          ServerMessage.offer(from, message.sdp),
        );
        break;
      case "answer":
        this.#forward(connId, message.to, "answer", emit, (from) =>
          ServerMessage.answer(from, message.sdp),
        );
        break;
      case "ice":
        this.#forward(connId, message.to, "ice", emit, (from) =>
          ServerMessage.ice(from, message.candidate),
        );
        break;
      case "relay":
        this.#forward(connId, message.to, "relay", emit, (from) =>
          ServerMessage.relay(from, message.payload),
        );
        break;
      default:
        emit(connId, ServerMessage.error(SignalingErrorCode.BadMessage, "unknown message"));
    }
    return out;
  }

  // Drop a connection that closed (socket close/error).
  disconnect(connId) {
    const out = [];
    this.#leave(connId, (to, msg) => out.push({ to, message: msg }));
    return out;
  }

  #join(connId, peer, emit) {
    if (this.#byConn.has(connId)) {
      emit(connId, ServerMessage.error(SignalingErrorCode.AlreadyJoined, "connection already joined"));
      return;
    }
    if (!this.#permissions.isAllowed(peer, { kind: "join" })) {
      emit(connId, ServerMessage.error(SignalingErrorCode.PermissionDenied, `peer ${peer} is not allowed to join`));
      return;
    }
    if (this.#byPeer.has(peer)) {
      emit(connId, ServerMessage.error(SignalingErrorCode.DuplicatePeer, `peer ${peer} already present`));
      return;
    }

    this.#byPeer.set(peer, connId);
    this.#byConn.set(connId, peer);

    const peers = this.roster().filter((p) => p !== peer);
    emit(connId, ServerMessage.welcome(peer, peers));
    for (const [otherPeer, otherConn] of this.#byPeer) {
      if (otherPeer !== peer) emit(otherConn, ServerMessage.peerJoined(peer));
    }
  }

  #leave(connId, emit) {
    const peer = this.#byConn.get(connId);
    if (peer === undefined) return;
    this.#byConn.delete(connId);
    this.#byPeer.delete(peer);
    for (const [otherPeer, otherConn] of this.#byPeer) {
      if (otherPeer !== peer) emit(otherConn, ServerMessage.peerLeft(peer));
    }
  }

  #forward(connId, to, kind, emit, build) {
    const from = this.#byConn.get(connId);
    if (from === undefined) {
      emit(connId, ServerMessage.error(SignalingErrorCode.NotJoined, "join before signaling"));
      return;
    }
    if (DIRECTED.has(kind) && !this.#permissions.isAllowed(from, { kind, to })) {
      emit(
        connId,
        ServerMessage.error(
          SignalingErrorCode.PermissionDenied,
          `peer ${from} is not allowed to signal peer ${to}`,
        ),
      );
      return;
    }
    const targetConn = this.#byPeer.get(to);
    if (targetConn === undefined) {
      emit(
        connId,
        ServerMessage.error(
          SignalingErrorCode.UnknownTarget,
          `peer ${to} is not in this session`,
        ),
      );
      return;
    }
    emit(targetConn, build(from));
  }
}

// ---------------------------------------------------------------------------
// SignalingSocket seam + in-memory loopback pair
// ---------------------------------------------------------------------------

// Async single-consumer frame queue with close semantics.
class FrameQueue {
  #items = [];
  #waiters = [];
  #closed = false;

  push(item) {
    if (this.#waiters.length > 0) {
      this.#waiters.shift()(item);
    } else {
      this.#items.push(item);
    }
  }

  take() {
    if (this.#items.length > 0) {
      return Promise.resolve(this.#items.shift());
    }
    if (this.#closed) {
      return Promise.resolve(null);
    }
    return new Promise((resolve) => this.#waiters.push(resolve));
  }

  close() {
    this.#closed = true;
    while (this.#waiters.length > 0) {
      this.#waiters.shift()(null);
    }
  }
}

/**
 * In-memory SignalingSocket implementing the seam
 * `{ send(text): void; recv(): Promise<string|null>; close(): void }`.
 * `MemorySignalingSocket.pair()` returns two cross-wired endpoints: text sent on
 * one is received on the other.
 */
export class MemorySignalingSocket {
  #tx;
  #rx;

  constructor(tx, rx) {
    this.#tx = tx;
    this.#rx = rx;
  }

  static pair() {
    const aToB = new FrameQueue();
    const bToA = new FrameQueue();
    return [
      new MemorySignalingSocket(aToB, bToA),
      new MemorySignalingSocket(bToA, aToB),
    ];
  }

  send(text) {
    this.#tx.push(text);
  }

  recv() {
    return this.#rx.take();
  }

  close() {
    this.#tx.close();
    this.#rx.close();
  }
}

// ---------------------------------------------------------------------------
// SignalingClient over the SignalingSocket seam
// ---------------------------------------------------------------------------

/**
 * A signaling-session client over a `SignalingSocket` seam
 * (`{ send(text): Promise|void; recv(): Promise<string|null>; close() }`).
 * `SignalingClient.connect(socket, peer, {capabilities})` auto-sends the join.
 */
export class SignalingClient {
  #socket;
  #peer;

  constructor(socket, peer) {
    this.#socket = socket;
    this.#peer = assertPeer(peer);
  }

  static connect(socket, peer, { capabilities } = {}) {
    const client = new SignalingClient(socket, peer);
    client.send(ClientMessage.join(peer, capabilities ?? null));
    return client;
  }

  get peer() {
    return this.#peer;
  }

  // Send a typed ClientMessage. Returns whatever the socket's send returns
  // (Promise or void).
  send(message) {
    return this.#socket.send(JSON.stringify(message.toWire()));
  }

  offer(to, sdp) {
    return this.send(ClientMessage.offer(to, sdp));
  }

  answer(to, sdp) {
    return this.send(ClientMessage.answer(to, sdp));
  }

  ice(to, candidate) {
    return this.send(ClientMessage.ice(to, candidate));
  }

  relay(to, payload) {
    return this.send(ClientMessage.relay(to, payload));
  }

  leave() {
    const result = this.send(ClientMessage.leave());
    this.#socket.close();
    return result;
  }

  close() {
    this.#socket.close();
  }

  // Receive the next server message, or null once the socket closes.
  async recv() {
    const text = await this.#socket.recv();
    if (text === null || text === undefined) {
      return null;
    }
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch (error) {
      throw SignalingError.protocol("failed to parse server frame", error);
    }
    return ServerMessage.fromWire(parsed);
  }

  // Register a callback for incoming server messages; returns an unsubscribe fn.
  onMessage(callback) {
    let active = true;
    (async () => {
      while (active) {
        let message;
        try {
          message = await this.recv();
        } catch {
          break;
        }
        if (message === null) break;
        if (active) callback(message);
      }
    })();
    return () => {
      active = false;
    };
  }
}
