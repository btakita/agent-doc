const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

function assertObject(value, name) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be an object`);
  }
  return value;
}

function assertTagged(value, name) {
  const object = assertObject(value, name);
  const entries = Object.entries(object);
  if (entries.length !== 1) {
    throw new TypeError(`${name} must be externally tagged`);
  }
  return entries[0];
}

function assertInteger(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative safe integer`);
  }
  return value;
}

function bytesFromWire(value) {
  if (!Array.isArray(value)) {
    throw new TypeError("byte payload must be an array");
  }
  return value.map((byte, index) => {
    if (!Number.isInteger(byte) || byte < 0 || byte > 255) {
      throw new TypeError(`byte payload[${index}] must be in 0..255`);
    }
    return byte;
  });
}

function bytesOf(value) {
  if (value instanceof Uint8Array) {
    return Array.from(value);
  }
  return bytesFromWire(value);
}

// NodeKey: optional wire-stable "/"-joined keyed address (protocol.md § NodeKey).
// JSON omits the field when absent; a missing field decodes to null.
const NODE_KEY_MAX_BYTES = 1024;
const NODE_KEY_MAX_SEGMENTS = 32;

function normalizeNodeKey(key) {
  if (key === undefined || key === null) {
    return null;
  }
  if (typeof key !== "string") {
    throw new TypeError("NodeKey must be a string");
  }
  if (key === "") {
    throw new TypeError("NodeKey must not be empty");
  }
  if (textEncoder.encode(key).length > NODE_KEY_MAX_BYTES) {
    throw new TypeError(`NodeKey must be <= ${NODE_KEY_MAX_BYTES} bytes`);
  }
  const segments = key.split("/");
  if (segments.length > NODE_KEY_MAX_SEGMENTS) {
    throw new TypeError(`NodeKey must have <= ${NODE_KEY_MAX_SEGMENTS} segments`);
  }
  if (segments.some((segment) => segment === "")) {
    throw new TypeError(
      "NodeKey must not contain empty segments (leading/trailing/double '/')",
    );
  }
  return key;
}

// The pluggable blob backends a descriptor may name (zero-copy transport,
// `#lzzcpy`). `shm` is the default and is omitted from the wire so legacy
// descriptors round-trip byte-for-byte. See src/transport.js and
// lazily-spec/docs/zero-copy-transport.md.
export const BlobBackendKind = Object.freeze({
  Shm: "shm",
  Arrow: "arrow",
  InProcess: "in_process",
});

const BLOB_BACKEND_KINDS = new Set(Object.values(BlobBackendKind));

export class ShmBlobRef {
  constructor({ offset, len, generation, epoch, checksum, backend }) {
    this.offset = assertInteger(offset, "offset");
    this.len = assertInteger(len, "len");
    this.generation = assertInteger(generation, "generation");
    this.epoch = assertInteger(epoch, "epoch");
    this.checksum = assertInteger(checksum, "checksum");
    // Optional pluggable-backend discriminator. Defaults to `shm` so every
    // pre-transport descriptor keeps meaning "POSIX shared memory".
    const kind = backend ?? BlobBackendKind.Shm;
    if (!BLOB_BACKEND_KINDS.has(kind)) {
      throw new TypeError(`unknown blob backend: ${kind}`);
    }
    this.backend = kind;
    Object.freeze(this);
  }

  // Return a copy of this descriptor tagged for `backend`. Backends stamp their
  // own kind onto an arena-minted descriptor via this helper.
  withBackend(backend) {
    return new ShmBlobRef({
      offset: this.offset,
      len: this.len,
      generation: this.generation,
      epoch: this.epoch,
      checksum: this.checksum,
      backend,
    });
  }

  toWire() {
    const wire = {
      offset: this.offset,
      len: this.len,
      generation: this.generation,
      epoch: this.epoch,
      checksum: this.checksum,
    };
    // Omit the default `shm` so the descriptor stays byte-compatible with the
    // pre-`#lzzcpy` wire form (the `backend`-absent conformance fixture).
    if (this.backend !== BlobBackendKind.Shm) {
      wire.backend = this.backend;
    }
    return wire;
  }

  static fromWire(value) {
    const object = assertObject(value, "ShmBlobRef");
    return new ShmBlobRef({
      offset: object.offset,
      len: object.len,
      generation: object.generation,
      epoch: object.epoch,
      checksum: object.checksum,
      backend: object.backend ?? BlobBackendKind.Shm,
    });
  }
}

export class NodeStatePayload {
  constructor(bytes) {
    this.bytes = Object.freeze(bytesOf(bytes));
    Object.freeze(this);
  }

  toWire() {
    return { Payload: [...this.bytes] };
  }
}

export class NodeStateSharedBlob {
  constructor(blob) {
    this.blob = blob instanceof ShmBlobRef ? blob : ShmBlobRef.fromWire(blob);
    Object.freeze(this);
  }

  toWire() {
    return { SharedBlob: this.blob.toWire() };
  }
}

export class NodeStateOpaque {
  toWire() {
    return "Opaque";
  }
}

export const NodeState = Object.freeze({
  payload(bytes) {
    return new NodeStatePayload(bytes);
  },
  sharedBlob(blob) {
    return new NodeStateSharedBlob(blob);
  },
  opaque() {
    return new NodeStateOpaque();
  },
  fromWire(value) {
    if (typeof value === "string") {
      if (value !== "Opaque") {
        throw new TypeError(`unknown NodeState unit variant: ${value}`);
      }
      return new NodeStateOpaque();
    }

    const [tag, body] = assertTagged(value, "NodeState");
    switch (tag) {
      case "Payload":
        return new NodeStatePayload(body);
      case "SharedBlob":
        return new NodeStateSharedBlob(ShmBlobRef.fromWire(body));
      case "Opaque":
        return new NodeStateOpaque();
      default:
        throw new TypeError(`unknown NodeState variant: ${tag}`);
    }
  },
});

export class IpcValueInline {
  constructor(bytes) {
    this.bytes = Object.freeze(bytesOf(bytes));
    Object.freeze(this);
  }

  toWire() {
    return { Inline: [...this.bytes] };
  }
}

export class IpcValueSharedBlob {
  constructor(blob) {
    this.blob = blob instanceof ShmBlobRef ? blob : ShmBlobRef.fromWire(blob);
    Object.freeze(this);
  }

  toWire() {
    return { SharedBlob: this.blob.toWire() };
  }
}

export const IpcValue = Object.freeze({
  inline(bytes) {
    return new IpcValueInline(bytes);
  },
  sharedBlob(blob) {
    return new IpcValueSharedBlob(blob);
  },
  of(value) {
    if (value instanceof IpcValueInline || value instanceof IpcValueSharedBlob) {
      return value;
    }
    if (value instanceof ShmBlobRef) {
      return new IpcValueSharedBlob(value);
    }
    return new IpcValueInline(value);
  },
  fromWire(value) {
    const [tag, body] = assertTagged(value, "IpcValue");
    switch (tag) {
      case "Inline":
        return new IpcValueInline(body);
      case "SharedBlob":
        return new IpcValueSharedBlob(ShmBlobRef.fromWire(body));
      default:
        throw new TypeError(`unknown IpcValue variant: ${tag}`);
    }
  },
});

export class NodeSnapshot {
  constructor(node, typeTag, state, key = null) {
    this.node = assertInteger(node, "node");
    this.typeTag = String(typeTag);
    this.state = state;
    this.key = normalizeNodeKey(key);
    Object.freeze(this);
  }

  toWire() {
    const wire = {
      node: this.node,
      type_tag: this.typeTag,
      state: this.state.toWire(),
    };
    if (this.key !== null) {
      wire.key = this.key;
    }
    return wire;
  }

  static payload(node, typeTag, bytes, key) {
    return new NodeSnapshot(node, typeTag, NodeState.payload(bytes), key);
  }

  static sharedBlob(node, typeTag, blob, key) {
    return new NodeSnapshot(node, typeTag, NodeState.sharedBlob(blob), key);
  }

  static opaque(node, typeTag, key) {
    return new NodeSnapshot(node, typeTag, NodeState.opaque(), key);
  }

  static fromWire(value) {
    const object = assertObject(value, "NodeSnapshot");
    return new NodeSnapshot(
      object.node,
      object.type_tag,
      NodeState.fromWire(object.state),
      object.key ?? null,
    );
  }
}

export class EdgeSnapshot {
  constructor(dependent, dependency) {
    this.dependent = assertInteger(dependent, "dependent");
    this.dependency = assertInteger(dependency, "dependency");
    Object.freeze(this);
  }

  toWire() {
    return {
      dependent: this.dependent,
      dependency: this.dependency,
    };
  }

  isReadableBy(permissions, peer) {
    return (
      permissions.canRead(peer, this.dependent) &&
      permissions.canRead(peer, this.dependency)
    );
  }

  static fromWire(value) {
    const object = assertObject(value, "EdgeSnapshot");
    return new EdgeSnapshot(object.dependent, object.dependency);
  }
}

export class Snapshot {
  constructor({ epoch, nodes = [], edges = [], roots = [] }) {
    this.epoch = assertInteger(epoch, "epoch");
    this.nodes = Object.freeze([...nodes]);
    this.edges = Object.freeze([...edges]);
    this.roots = Object.freeze(roots.map((node) => assertInteger(node, "root")));
    Object.freeze(this);
  }

  toWire() {
    return {
      epoch: this.epoch,
      nodes: this.nodes.map((node) => node.toWire()),
      edges: this.edges.map((edge) => edge.toWire()),
      roots: [...this.roots],
    };
  }

  filterReadable(permissions, peer) {
    return new Snapshot({
      epoch: this.epoch,
      nodes: this.nodes.filter((node) => permissions.canRead(peer, node.node)),
      edges: this.edges.filter((edge) => edge.isReadableBy(permissions, peer)),
      roots: permissions.filterReadable(peer, this.roots),
    });
  }

  static fromWire(value) {
    const object = assertObject(value, "Snapshot");
    return new Snapshot({
      epoch: object.epoch,
      nodes: (object.nodes ?? []).map((node) => NodeSnapshot.fromWire(node)),
      edges: (object.edges ?? []).map((edge) => EdgeSnapshot.fromWire(edge)),
      roots: object.roots ?? [],
    });
  }
}

class DeltaOpBase {
  targetReadable() {
    throw new Error("DeltaOp.targetReadable must be implemented by subclasses");
  }
}

export class DeltaOpCellSet extends DeltaOpBase {
  constructor(node, payload) {
    super();
    this.node = assertInteger(node, "node");
    this.payload = IpcValue.of(payload);
    Object.freeze(this);
  }

  toWire() {
    return { CellSet: { node: this.node, payload: this.payload.toWire() } };
  }

  targetReadable(permissions, peer) {
    return permissions.canRead(peer, this.node);
  }
}

export class DeltaOpSlotValue extends DeltaOpBase {
  constructor(node, payload) {
    super();
    this.node = assertInteger(node, "node");
    this.payload = IpcValue.of(payload);
    Object.freeze(this);
  }

  toWire() {
    return { SlotValue: { node: this.node, payload: this.payload.toWire() } };
  }

  targetReadable(permissions, peer) {
    return permissions.canRead(peer, this.node);
  }
}

export class DeltaOpInvalidate extends DeltaOpBase {
  constructor(node) {
    super();
    this.node = assertInteger(node, "node");
    Object.freeze(this);
  }

  toWire() {
    return { Invalidate: { node: this.node } };
  }

  targetReadable(permissions, peer) {
    return permissions.canRead(peer, this.node);
  }
}

export class DeltaOpNodeAdd extends DeltaOpBase {
  constructor(node, typeTag, state, key = null) {
    super();
    this.node = assertInteger(node, "node");
    this.typeTag = String(typeTag);
    this.state = state;
    this.key = normalizeNodeKey(key);
    Object.freeze(this);
  }

  toWire() {
    const wire = {
      node: this.node,
      type_tag: this.typeTag,
      state: this.state.toWire(),
    };
    if (this.key !== null) {
      wire.key = this.key;
    }
    return { NodeAdd: wire };
  }

  targetReadable(permissions, peer) {
    return permissions.canRead(peer, this.node);
  }
}

export class DeltaOpNodeRemove extends DeltaOpBase {
  constructor(node) {
    super();
    this.node = assertInteger(node, "node");
    Object.freeze(this);
  }

  toWire() {
    return { NodeRemove: { node: this.node } };
  }

  targetReadable(permissions, peer) {
    return permissions.canRead(peer, this.node);
  }
}

export class DeltaOpEdgeAdd extends DeltaOpBase {
  constructor(dependent, dependency) {
    super();
    this.dependent = assertInteger(dependent, "dependent");
    this.dependency = assertInteger(dependency, "dependency");
    Object.freeze(this);
  }

  toWire() {
    return { EdgeAdd: { dependent: this.dependent, dependency: this.dependency } };
  }

  targetReadable(permissions, peer) {
    return (
      permissions.canRead(peer, this.dependent) &&
      permissions.canRead(peer, this.dependency)
    );
  }
}

export class DeltaOpEdgeRemove extends DeltaOpBase {
  constructor(dependent, dependency) {
    super();
    this.dependent = assertInteger(dependent, "dependent");
    this.dependency = assertInteger(dependency, "dependency");
    Object.freeze(this);
  }

  toWire() {
    return {
      EdgeRemove: { dependent: this.dependent, dependency: this.dependency },
    };
  }

  targetReadable(permissions, peer) {
    return (
      permissions.canRead(peer, this.dependent) &&
      permissions.canRead(peer, this.dependency)
    );
  }
}

export const DeltaOp = Object.freeze({
  cellSet(node, payload) {
    return new DeltaOpCellSet(node, payload);
  },
  slotValue(node, payload) {
    return new DeltaOpSlotValue(node, payload);
  },
  invalidate(node) {
    return new DeltaOpInvalidate(node);
  },
  nodeAdd(node, typeTag, state, key) {
    return new DeltaOpNodeAdd(node, typeTag, state, key);
  },
  nodeRemove(node) {
    return new DeltaOpNodeRemove(node);
  },
  edgeAdd(dependent, dependency) {
    return new DeltaOpEdgeAdd(dependent, dependency);
  },
  edgeRemove(dependent, dependency) {
    return new DeltaOpEdgeRemove(dependent, dependency);
  },
  fromWire(value) {
    const [tag, body] = assertTagged(value, "DeltaOp");
    const object = assertObject(body, tag);
    switch (tag) {
      case "CellSet":
        return new DeltaOpCellSet(object.node, IpcValue.fromWire(object.payload));
      case "SlotValue":
        return new DeltaOpSlotValue(object.node, IpcValue.fromWire(object.payload));
      case "Invalidate":
        return new DeltaOpInvalidate(object.node);
      case "NodeAdd":
        return new DeltaOpNodeAdd(
          object.node,
          object.type_tag,
          NodeState.fromWire(object.state),
          object.key ?? null,
        );
      case "NodeRemove":
        return new DeltaOpNodeRemove(object.node);
      case "EdgeAdd":
        return new DeltaOpEdgeAdd(object.dependent, object.dependency);
      case "EdgeRemove":
        return new DeltaOpEdgeRemove(object.dependent, object.dependency);
      default:
        throw new TypeError(`unknown DeltaOp variant: ${tag}`);
    }
  },
});

export const DeltaApplyStatusKind = Object.freeze({
  Apply: "apply",
  ResyncRequired: "resync_required",
});

export class DeltaApplyStatus {
  constructor(kind, fields = {}) {
    this.kind = kind;
    Object.assign(this, fields);
    Object.freeze(this);
  }

  get isApply() {
    return this.kind === DeltaApplyStatusKind.Apply;
  }

  get isResyncRequired() {
    return this.kind === DeltaApplyStatusKind.ResyncRequired;
  }

  static apply() {
    return new DeltaApplyStatus(DeltaApplyStatusKind.Apply);
  }

  static resyncRequired(lastEpoch, baseEpoch, epoch) {
    return new DeltaApplyStatus(DeltaApplyStatusKind.ResyncRequired, {
      lastEpoch,
      baseEpoch,
      epoch,
    });
  }
}

export class Delta {
  constructor({ baseEpoch, epoch, ops = [] }) {
    this.baseEpoch = assertInteger(baseEpoch, "baseEpoch");
    this.epoch = assertInteger(epoch, "epoch");
    this.ops = Object.freeze([...ops]);
    Object.freeze(this);
  }

  isNextAfter(lastEpoch) {
    return this.baseEpoch === lastEpoch && this.epoch === this.baseEpoch + 1;
  }

  // The accepted-event span this delta advances: epoch - baseEpoch (usually 1,
  // > 1 for a coalesced multi-epoch-span delta; #lzsync, spec § Multi-epoch-span
  // delta). Coerced to >= 0 for a malformed backward delta.
  span() {
    return Math.max(0, this.epoch - this.baseEpoch);
  }

  applyStatus(lastEpoch) {
    if (this.isNextAfter(lastEpoch)) {
      return DeltaApplyStatus.apply();
    }
    return DeltaApplyStatus.resyncRequired(lastEpoch, this.baseEpoch, this.epoch);
  }

  filterReadable(permissions, peer) {
    return new Delta({
      baseEpoch: this.baseEpoch,
      epoch: this.epoch,
      ops: this.ops.filter((op) => op.targetReadable(permissions, peer)),
    });
  }

  toWire() {
    return {
      base_epoch: this.baseEpoch,
      epoch: this.epoch,
      ops: this.ops.map((op) => op.toWire()),
    };
  }

  static next(baseEpoch, ops = []) {
    return new Delta({ baseEpoch, epoch: baseEpoch + 1, ops });
  }

  static fromWire(value) {
    const object = assertObject(value, "Delta");
    return new Delta({
      baseEpoch: object.base_epoch,
      epoch: object.epoch,
      ops: (object.ops ?? []).map((op) => DeltaOp.fromWire(op)),
    });
  }
}

// A frontier entry is the (peer, WireStamp) tuple lazily-rs carries as
// Vec<(u64, WireStamp)>; serde emits it as a 2-element JSON array [peer, stamp].
function frontierEntryFromWire(value) {
  if (!Array.isArray(value) || value.length !== 2) {
    throw new TypeError("frontier entry must be a [peer, WireStamp] pair");
  }
  return Object.freeze({
    peer: assertInteger(value[0], "frontier peer"),
    stamp: WireStamp.fromWire(value[1]),
  });
}

function frontierEntryOf(entry) {
  if (entry && typeof entry === "object" && !Array.isArray(entry)) {
    if ("peer" in entry) {
      return Object.freeze({
        peer: assertInteger(entry.peer, "frontier peer"),
        stamp:
          entry.stamp instanceof WireStamp
            ? entry.stamp
            : WireStamp.fromWire(entry.stamp),
      });
    }
  }
  return frontierEntryFromWire(entry);
}

export class WireStamp {
  constructor({ wallTime, logical, peer }) {
    this.wallTime = assertInteger(wallTime, "wallTime");
    this.logical = assertInteger(logical, "logical");
    this.peer = assertInteger(peer, "peer");
    Object.freeze(this);
  }

  toWire() {
    return {
      wall_time: this.wallTime,
      logical: this.logical,
      peer: this.peer,
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "WireStamp");
    return new WireStamp({
      wallTime: object.wall_time,
      logical: object.logical,
      peer: object.peer,
    });
  }
}

export class CrdtOp {
  constructor(node, stamp, state, key = null) {
    this.node = assertInteger(node, "node");
    this.stamp = stamp instanceof WireStamp ? stamp : WireStamp.fromWire(stamp);
    this.state = IpcValue.of(state);
    // CrdtOp mirrors lazily-rs's derived serde (no skip_serializing_if), so a
    // keyless op serializes `key: null` — unlike NodeSnapshot/NodeAdd, which
    // omit the field. normalizeNodeKey still enforces bounds when a key is set.
    this.key = normalizeNodeKey(key);
    Object.freeze(this);
  }

  toWire() {
    return {
      node: this.node,
      key: this.key,
      stamp: this.stamp.toWire(),
      state: this.state.toWire(),
    };
  }

  targetReadable(permissions, peer) {
    return permissions.canRead(peer, this.node);
  }

  static keyed(node, key, stamp, state) {
    return new CrdtOp(node, stamp, state, key);
  }

  static fromWire(value) {
    const object = assertObject(value, "CrdtOp");
    return new CrdtOp(
      object.node,
      WireStamp.fromWire(object.stamp),
      IpcValue.fromWire(object.state),
      object.key ?? null,
    );
  }
}

export class CrdtSync {
  constructor({ frontier = [], ops = [] }) {
    this.frontier = Object.freeze(frontier.map(frontierEntryOf));
    this.ops = Object.freeze([...ops]);
    Object.freeze(this);
  }

  filterReadable(permissions, peer) {
    // The frontier advertisement names peers and stamps, not node content, and
    // the receiver needs the whole frontier to compute a sound watermark — so
    // it is retained in full while ops are omitted (not redacted), like Delta.
    return new CrdtSync({
      frontier: this.frontier,
      ops: this.ops.filter((op) => op.targetReadable(permissions, peer)),
    });
  }

  toWire() {
    return {
      frontier: this.frontier.map((entry) => [
        entry.peer,
        entry.stamp.toWire(),
      ]),
      ops: this.ops.map((op) => op.toWire()),
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "CrdtSync");
    return new CrdtSync({
      frontier: (object.frontier ?? []).map(frontierEntryFromWire),
      ops: (object.ops ?? []).map((op) => CrdtOp.fromWire(op)),
    });
  }
}

export const ReceiptOutcome = Object.freeze({
  Observed: "observed",
  Accepted: "accepted",
  Applied: "applied",
  Rejected: "rejected",
});

export function isTerminalReceiptOutcome(outcome) {
  return outcome === ReceiptOutcome.Applied || outcome === ReceiptOutcome.Rejected;
}

function normalizeReceiptOutcome(outcome) {
  if (Object.values(ReceiptOutcome).includes(outcome)) {
    return outcome;
  }
  throw new TypeError(`unknown receipt outcome: ${outcome}`);
}

export class CausalReceipt {
  constructor({
    receiptId,
    causationId,
    observer,
    generation,
    outcome,
    reason = null,
    payloadHash = null,
  }) {
    this.receiptId = assertString(receiptId, "receiptId");
    this.causationId = assertString(causationId, "causationId");
    this.observer = assertString(observer, "observer");
    this.generation = assertInteger(generation, "generation");
    this.outcome = normalizeReceiptOutcome(outcome);
    this.reason = reason === null ? null : assertString(reason, "reason");
    this.payloadHash = payloadHash === null ? null : assertString(payloadHash, "payloadHash");
    Object.freeze(this);
  }

  get isTerminal() {
    return isTerminalReceiptOutcome(this.outcome);
  }

  toWire() {
    return {
      receipt_id: this.receiptId,
      causation_id: this.causationId,
      observer: this.observer,
      generation: this.generation,
      outcome: this.outcome,
      reason: this.reason,
      payload_hash: this.payloadHash,
    };
  }

  static observed(receiptId, causationId, observer, generation) {
    return new CausalReceipt({
      receiptId,
      causationId,
      observer,
      generation,
      outcome: ReceiptOutcome.Observed,
    });
  }

  static accepted(receiptId, causationId, observer, generation) {
    return new CausalReceipt({
      receiptId,
      causationId,
      observer,
      generation,
      outcome: ReceiptOutcome.Accepted,
    });
  }

  static applied(receiptId, causationId, observer, generation, payloadHash = null) {
    return new CausalReceipt({
      receiptId,
      causationId,
      observer,
      generation,
      outcome: ReceiptOutcome.Applied,
      payloadHash,
    });
  }

  static rejected(receiptId, causationId, observer, generation, reason = null) {
    return new CausalReceipt({
      receiptId,
      causationId,
      observer,
      generation,
      outcome: ReceiptOutcome.Rejected,
      reason,
    });
  }

  static fromWire(value) {
    const object = assertObject(value, "CausalReceipt");
    return new CausalReceipt({
      receiptId: object.receipt_id,
      causationId: object.causation_id,
      observer: object.observer,
      generation: object.generation,
      outcome: object.outcome,
      reason: object.reason ?? null,
      payloadHash: object.payload_hash ?? null,
    });
  }
}

export class CausalReceipts {
  constructor(receipts = []) {
    this.receipts = Object.freeze(receipts.map((receipt) =>
      receipt instanceof CausalReceipt ? receipt : CausalReceipt.fromWire(receipt),
    ));
    Object.freeze(this);
  }

  toWire() {
    return {
      receipts: this.receipts.map((receipt) => receipt.toWire()),
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "CausalReceipts");
    return new CausalReceipts(object.receipts ?? []);
  }
}

export class ReceiptMessage {
  constructor(kind, value) {
    this.kind = kind;
    this.causalReceipts = undefined;
    if (kind === "CausalReceipts") {
      this.causalReceipts =
        value instanceof CausalReceipts ? value : CausalReceipts.fromWire(value);
    } else {
      throw new TypeError(`unknown ReceiptMessage kind: ${kind}`);
    }
    Object.freeze(this);
  }

  toWire() {
    return { CausalReceipts: this.causalReceipts.toWire() };
  }

  encodeJson() {
    return textEncoder.encode(JSON.stringify(this.toWire()));
  }

  static causalReceipts(causalReceipts) {
    return new ReceiptMessage("CausalReceipts", causalReceipts);
  }

  static fromWire(value) {
    const [tag, body] = assertTagged(value, "ReceiptMessage");
    switch (tag) {
      case "CausalReceipts":
        return ReceiptMessage.causalReceipts(CausalReceipts.fromWire(body));
      default:
        throw new TypeError(`unknown ReceiptMessage variant: ${tag}`);
    }
  }

  static decodeJson(data) {
    const text =
      data instanceof Uint8Array ? textDecoder.decode(data) : String(data);
    return ReceiptMessage.fromWire(JSON.parse(text));
  }
}

export const ReceiptApplyStatusKind = Object.freeze({
  Recorded: "recorded",
  Duplicate: "duplicate",
  StaleGeneration: "stale_generation",
  TerminalConflict: "terminal_conflict",
});

export class ReceiptProjection {
  #receiptsById = new Map();
  #latestByCausation = new Map();
  #terminalByCausation = new Map();
  #staleReceiptIds = new Set();

  observe(currentGeneration, receipt) {
    const next = receipt instanceof CausalReceipt ? receipt : CausalReceipt.fromWire(receipt);
    if (this.#receiptsById.has(next.receiptId) || this.#staleReceiptIds.has(next.receiptId)) {
      return { kind: ReceiptApplyStatusKind.Duplicate };
    }

    if (currentGeneration !== null && currentGeneration !== undefined) {
      const expected = assertInteger(currentGeneration, "currentGeneration");
      if (next.generation !== expected) {
        this.#staleReceiptIds.add(next.receiptId);
        return {
          kind: ReceiptApplyStatusKind.StaleGeneration,
          expected,
          actual: next.generation,
        };
      }
    }

    if (next.isTerminal) {
      const existing = this.#terminalByCausation.get(next.causationId);
      if (existing && existing.outcome !== next.outcome) {
        return {
          kind: ReceiptApplyStatusKind.TerminalConflict,
          causationId: next.causationId,
          existing: existing.outcome,
          incoming: next.outcome,
        };
      }
      if (!existing) {
        this.#terminalByCausation.set(next.causationId, next);
      }
    }

    this.#latestByCausation.set(next.causationId, next);
    this.#receiptsById.set(next.receiptId, next);
    return { kind: ReceiptApplyStatusKind.Recorded };
  }

  latestFor(causationId) {
    return this.#latestByCausation.get(causationId) ?? null;
  }

  terminalFor(causationId) {
    return this.#terminalByCausation.get(causationId) ?? null;
  }

  containsReceipt(receiptId) {
    return this.#receiptsById.has(receiptId) || this.#staleReceiptIds.has(receiptId);
  }

  staleReceiptIds() {
    return [...this.#staleReceiptIds];
  }
}

// Reliable-sync reverse-channel control frame: request a covering Snapshot on a
// detected gap (#lzsync, spec § ResyncCoordinator). Carries no node content.
export class ResyncRequest {
  constructor({ fromEpoch }) {
    this.fromEpoch = assertInteger(fromEpoch, "fromEpoch");
    Object.freeze(this);
  }

  toWire() {
    return { from_epoch: this.fromEpoch };
  }

  static fromWire(value) {
    const object = assertObject(value, "ResyncRequest");
    return new ResyncRequest({ fromEpoch: object.from_epoch });
  }
}

// Reliable-sync reverse-channel control frame: prove receipt through throughEpoch
// (#lzsync, spec § DurableOutbox). Advances the sender's outbox retention cursor
// and doubles as the reconnect resume cursor. Carries no node content.
export class OutboxAck {
  constructor({ throughEpoch }) {
    this.throughEpoch = assertInteger(throughEpoch, "throughEpoch");
    Object.freeze(this);
  }

  toWire() {
    return { through_epoch: this.throughEpoch };
  }

  static fromWire(value) {
    const object = assertObject(value, "OutboxAck");
    return new OutboxAck({ throughEpoch: object.through_epoch });
  }
}

export class IpcMessage {
  constructor(kind, value) {
    this.kind = kind;
    this.snapshot = undefined;
    this.delta = undefined;
    this.crdtSync = undefined;
    this.resyncRequest = undefined;
    this.outboxAck = undefined;
    if (kind === "Snapshot") {
      this.snapshot = value;
    } else if (kind === "Delta") {
      this.delta = value;
    } else if (kind === "CrdtSync") {
      this.crdtSync = value;
    } else if (kind === "ResyncRequest") {
      this.resyncRequest = value;
    } else if (kind === "OutboxAck") {
      this.outboxAck = value;
    } else {
      throw new TypeError(`unknown IpcMessage kind: ${kind}`);
    }
    Object.freeze(this);
  }

  get isSnapshot() {
    return this.kind === "Snapshot";
  }

  get isDelta() {
    return this.kind === "Delta";
  }

  get isCrdtSync() {
    return this.kind === "CrdtSync";
  }

  get isResyncRequest() {
    return this.kind === "ResyncRequest";
  }

  get isOutboxAck() {
    return this.kind === "OutboxAck";
  }

  // Reliable-sync reverse-channel control frame (no node content).
  get isControl() {
    return this.kind === "ResyncRequest" || this.kind === "OutboxAck";
  }

  toWire() {
    if (this.kind === "Snapshot") {
      return { Snapshot: this.snapshot.toWire() };
    }
    if (this.kind === "Delta") {
      return { Delta: this.delta.toWire() };
    }
    if (this.kind === "CrdtSync") {
      return { CrdtSync: this.crdtSync.toWire() };
    }
    if (this.kind === "ResyncRequest") {
      return { ResyncRequest: this.resyncRequest.toWire() };
    }
    return { OutboxAck: this.outboxAck.toWire() };
  }

  encodeJson() {
    return textEncoder.encode(JSON.stringify(this.toWire()));
  }

  static snapshot(snapshot) {
    return new IpcMessage("Snapshot", snapshot);
  }

  static delta(delta) {
    return new IpcMessage("Delta", delta);
  }

  static crdtSync(crdtSync) {
    return new IpcMessage("CrdtSync", crdtSync);
  }

  static resyncRequestMessage(request) {
    return new IpcMessage("ResyncRequest", request);
  }

  static outboxAckMessage(ack) {
    return new IpcMessage("OutboxAck", ack);
  }

  static fromWire(value) {
    const [tag, body] = assertTagged(value, "IpcMessage");
    switch (tag) {
      case "Snapshot":
        return IpcMessage.snapshot(Snapshot.fromWire(body));
      case "Delta":
        return IpcMessage.delta(Delta.fromWire(body));
      case "CrdtSync":
        return IpcMessage.crdtSync(CrdtSync.fromWire(body));
      case "ResyncRequest":
        return IpcMessage.resyncRequestMessage(ResyncRequest.fromWire(body));
      case "OutboxAck":
        return IpcMessage.outboxAckMessage(OutboxAck.fromWire(body));
      default:
        throw new TypeError(`unknown IpcMessage variant: ${tag}`);
    }
  }

  static decodeJson(data) {
    const text =
      data instanceof Uint8Array ? textDecoder.decode(data) : String(data);
    return IpcMessage.fromWire(JSON.parse(text));
  }
}

// Capability negotiation (protocol.md § Capability Negotiation). Every
// non-local session starts with this compatibility handshake, exchanged before
// any `Snapshot` or `Delta` flows. If peers disagree on `protocol_major_version`,
// `codec`, `ordered_reliable`, or required features, they fail closed.
export const PROTOCOL_ID = "lazily-ipc";
export const PROTOCOL_MAJOR_VERSION = 1;

export const Codec = Object.freeze({
  Json: "json",
  Bincode: "bincode",
  Postcard: "postcard",
});

// FFI message-kind discriminant (schemas/ffi.json § LazilyFfiMessageKind). The
// JSON representation is normative; CrdtSync = 3 is required of the discriminant.
export const LazilyFfiMessageKind = Object.freeze({
  Unknown: 0,
  Snapshot: 1,
  Delta: 2,
  CrdtSync: 3,
  ResyncRequest: 4,
  OutboxAck: 5,
});

// The FFI status codes mirror schemas/ffi.json § LazilyFfiStatus.
export const LazilyFfiStatus = Object.freeze({
  Ok: 0,
  Empty: 1,
  NullPointer: 2,
  InvalidMessage: 3,
  EncodeFailed: 4,
  Panic: 5,
});

// ---------------------------------------------------------------------------
// Reliable sync protocol (#lzsync).
//
// Delivery-reliability over the Snapshot/Delta/CrdtSync planes (lazily-spec
// § Reliable Sync): gap recovery, at-least-once outbox, and OR-set / LWW liveness
// cells. Correctness backstop: lazily-formal ReliableSync.lean; cross-language
// pins: lazily-spec/conformance/reliable-sync/.
// ---------------------------------------------------------------------------

// Receiver decision for an inbound frame (spec § ResyncCoordinator).
export const ResyncAction = Object.freeze({
  Apply: "Apply",
  RequestSnapshot: "RequestSnapshot",
  Ignore: "Ignore",
});

// Receiver-side reliable-sync coordinator. Holds lastEpoch (highest epoch fully
// applied) and a resyncing flag (a RequestSnapshot is outstanding until a covering
// Snapshot lands). ingest advances lastEpoch on Apply; the caller MUST fold the
// frame's ops on Apply. Mirrors ReliableSync.step.
export class ResyncCoordinator {
  constructor(lastEpoch = 0) {
    this.lastEpoch = assertInteger(lastEpoch, "lastEpoch");
    this.resyncing = false;
  }

  // Classify + fold an inbound Delta; advances to delta.epoch on Apply (multi-epoch aware).
  // Returns { action, fromEpoch? }.
  ingestDelta(delta) {
    if (delta.baseEpoch === this.lastEpoch) {
      if (delta.epoch >= delta.baseEpoch + 1) {
        this.lastEpoch = delta.epoch;
        this.resyncing = false;
        return { action: ResyncAction.Apply };
      }
      return { action: ResyncAction.Ignore }; // empty/backward epoch
    }
    if (delta.baseEpoch < this.lastEpoch) {
      return { action: ResyncAction.Ignore }; // already applied — re-delivery
    }
    // gap: baseEpoch > lastEpoch
    if (this.resyncing) {
      return { action: ResyncAction.Ignore }; // suppress duplicate request
    }
    this.resyncing = true;
    return { action: ResyncAction.RequestSnapshot, fromEpoch: this.lastEpoch };
  }

  // Adopt a Snapshot — a full-state frame always applies.
  ingestSnapshot(snapshotEpoch) {
    this.lastEpoch = snapshotEpoch;
    this.resyncing = false;
    return { action: ResyncAction.Apply };
  }

  // Classify an inbound IpcMessage. CrdtSync rides the CRDT plane and the
  // reverse-channel control frames are for the sender's driver, so both are
  // ignored by this data receiver.
  ingest(msg) {
    if (msg.isSnapshot) return this.ingestSnapshot(msg.snapshot.epoch);
    if (msg.isDelta) return this.ingestDelta(msg.delta);
    return { action: ResyncAction.Ignore };
  }

  // The IpcMessage(OutboxAck) advertising this receiver's resume cursor.
  ack() {
    return IpcMessage.outboxAckMessage(new OutboxAck({ throughEpoch: this.lastEpoch }));
  }
}

// A byte-oriented OutboxStore is any object with
// put/deleteThrough/scanAfter/loadCursor/saveCursor. Stores are deliberately
// protocol-dumb; Outbox is the one append/ack/replay implementation.
export class Outbox {
  constructor(store) {
    this.store = store;
    this.ackedThrough = store.loadCursor();
  }

  append(epoch, msg) {
    return this.store.put(epoch, msg.encodeJson());
  }

  ackThrough(epoch) {
    if (epoch <= this.ackedThrough) return undefined;
    const target = () => Math.max(this.ackedThrough, epoch, this.store.loadCursor());
    const finish = () => {
      this.ackedThrough = target();
    };
    const remove = () => this.store.deleteThrough(target());
    const saved = this.store.saveCursor(epoch);
    if (saved && typeof saved.then === "function") {
      return saved.then(remove).then(finish);
    }
    const removed = remove();
    if (removed && typeof removed.then === "function") {
      return removed.then(finish);
    }
    finish();
    return undefined;
  }

  replayFrom(cursor) {
    return this.store
      .scanAfter(Math.max(cursor, this.ackedThrough))
      .sort((a, b) => a[0] - b[0])
      .map(([epoch, frame]) => [epoch, IpcMessage.decodeJson(frame)]);
  }

  retainedEpochs() {
    return this.store.scanAfter(this.ackedThrough).map(([epoch]) => epoch).sort((a, b) => a - b);
  }
}

export class InMemoryStore {
  constructor() {
    this.entries = new Map();
    this.cursor = 0;
  }

  put(epoch, frame) {
    this.entries.set(epoch, Uint8Array.from(frame));
  }

  deleteThrough(epoch) {
    for (const key of this.entries.keys()) {
      if (key <= epoch) this.entries.delete(key);
    }
  }

  scanAfter(epoch) {
    return [...this.entries]
      .filter(([key]) => key > epoch)
      .sort((a, b) => a[0] - b[0])
      .map(([key, frame]) => [key, Uint8Array.from(frame)]);
  }

  loadCursor() {
    return this.cursor;
  }

  saveCursor(epoch) {
    this.cursor = Math.max(this.cursor, epoch);
  }
}

// Process-local default, routed through the same protocol as persistent stores.
export class InMemoryOutbox extends Outbox {
  constructor() {
    super(new InMemoryStore());
  }
}

// An observed-remove set (OR-set) liveness cell. A (doc, pid) is present iff some
// add-tag is not shadowed by a remove that observed it (add-wins over a stale
// remove). Join is the union of both tag sets — a semilattice, so out-of-order and
// duplicate delivery converge (ReliableSync.joinOR_*).
export class OrSet {
  constructor() {
    this.adds = new Set();
    this.removes = new Set();
  }

  add(tag) {
    this.adds.add(tag);
  }

  removeObserved(tags) {
    for (const t of tags) this.removes.add(t);
  }

  present() {
    for (const t of this.adds) if (!this.removes.has(t)) return true;
    return false;
  }

  join(other) {
    for (const t of other.adds) this.adds.add(t);
    for (const t of other.removes) this.removes.add(t);
  }
}

// Total order (wallTime, logical, peer) — the wire mirror of the HLC stamp.
export function wireStampGreater(a, b) {
  if (a.wallTime !== b.wallTime) return a.wallTime > b.wallTime;
  if (a.logical !== b.logical) return a.logical > b.logical;
  return a.peer > b.peer;
}

// A last-writer-wins register liveness cell (per-pid alive, owner lease), keyed by
// WireStamp: the highest stamp wins. Join is the stamp-max, a semilattice
// (ReliableSync.joinReg_*).
export class WireLwwRegister {
  constructor(stamp, value) {
    this.stamp = stamp;
    this.value = value;
  }

  // Write value at stamp iff it dominates the current stamp.
  set(stamp, value) {
    if (wireStampGreater(stamp, this.stamp)) {
      this.stamp = stamp;
      this.value = value;
    }
  }

  // Join another replica's register (keep the higher stamp).
  join(other) {
    this.set(other.stamp, other.value);
  }
}

// -- SyncDriver: full-duplex reliable-sync loop (#sync-driver) --------------
//
// The transport seam is two host-supplied duck-typed objects (spec §
// "Transport seam (IpcSink / IpcSource, #lzsync-transport-seam)"); the seams
// carry no wire form of their own — what crosses the wire is the codec-encoded
// IpcMessage frame. A binding names them idiomatically; the semantics are
// normative:
//
//   IpcSink   — { send(msg): boolean }  outbound: deliver one already-framed
//               IpcMessage. Returning `false` (or throwing) means the frame was
//               NOT durably handed to the peer; at-least-once is a DRIVER
//               property (retain-in-outbox + replay-on-reconnect), not a sink
//               property, so a sink is free to be plain best-effort.
//   IpcSource — { recv(): IpcMessage | null }  inbound: poll for the next frame
//               without blocking. `null`/`undefined` = currently exhausted or
//               closed (no inbound progress this tick). A THROW is the reconnect
//               signal: it surfaces from tick() as a DriverError.Source and the
//               host re-establishes the carrier and calls onReconnect().
//   Clock     — { nowMillis(): number }  monotonic millis; the driver owns no
//               clock/runtime — the host ticks on its own cadence and the clock
//               only timestamps the stall signal.
//   SnapshotProvider — { snapshot(fromEpoch): IpcMessage }  the sender-side
//               answer to a peer's ResyncRequest (a covering Snapshot at
//               epoch >= fromEpoch).

// A monotonic wall-clock Clock seam (Date.now millis). Hosts that need a
// deterministic or steady clock inject their own { nowMillis() } instead.
export class SystemClock {
  nowMillis() {
    return Date.now();
  }
}

// What one SyncDriver.tick() accomplished (spec § SyncDriver). `applied` are the
// inbound Snapshot/Delta/CrdtSync frames the host MUST fold into its projection
// this tick — the driver has already advanced the receiver cursor for them.
export class Progress {
  constructor() {
    this.sent = 0; // data frames pushed to the sink this tick
    this.applied = []; // inbound frames the host must fold (Apply)
    this.resyncRequested = false; // a gap was detected → ResyncRequest emitted
    this.snapshotsServed = 0; // inbound ResyncRequests answered with a snapshot
    this.peerAckedThrough = 0; // peer ack cursor after this tick (retention/resume)
    this.retained = 0; // outbox frames still unacked (reconnect replay depth)
  }
}

// A transport error surfaced by SyncDriver.tick(). A SINK failure is not fatal
// (retain-and-stall, reported via Progress/stall signals); only an inbound
// SOURCE read failure is thrown, telling the host to reconnect + onReconnect().
export class DriverError extends Error {
  constructor(cause) {
    super(`reliable-sync source read failed: ${cause}`);
    this.name = "DriverError";
    this.kind = "Source";
    this.cause = cause;
  }
}

// Full-duplex reliable-sync loop driver (spec § SyncDriver). One driver drives
// one peer connection over a host-supplied sink/source pair. It composes the
// three pure-protocol pieces (ResyncCoordinator, DurableOutbox, SnapshotProvider)
// into the loop shape the spec pins: drain (append-before-send) → retain-on-fail
// → receive (route control frames + coordinator) → resync-on-reconnect. The
// driver owns no threads, no clock source, and no storage engine — the host
// injects all three and decides the tick cadence. Mirrors lazily-rs SyncDriver.
export class SyncDriver {
  // options: { sink, source, outbox?, clock?, provider, lastEpoch? }.
  constructor(options) {
    const opts = assertObject(options, "SyncDriver");
    this.sink = opts.sink;
    this.source = opts.source;
    this.outbox = opts.outbox ?? new InMemoryOutbox();
    this.clock = opts.clock ?? new SystemClock();
    this.provider = opts.provider;
    this.coordinator = new ResyncCoordinator(opts.lastEpoch ?? 0);
    this.pending = []; // host-enqueued outbound [epoch, IpcMessage] staged for drain
    this.peerAckedThrough = 0; // highest epoch the peer has acked (retention/resume)
    this.ackOwed = false; // we applied a frame and owe the peer an OutboxAck
    this.replayPending = false; // a reconnect happened; next tick replays the suffix
    this.stalledSince = null; // millis of the last sink send failure (null = healthy)
  }

  // Stage an outbound data frame at `epoch` for the next tick's drain. `epoch`
  // is the frame's accepted-event count (Delta.epoch / Snapshot.epoch); it
  // becomes the outbox retention key.
  enqueue(epoch, msg) {
    this.pending.push([epoch, msg]);
  }

  // Signal that the transport was re-established; the next tick() replays the
  // unacked outbox suffix and re-advertises our receiver cursor.
  onReconnect() {
    this.replayPending = true;
    this.ackOwed = true;
    this.stalledSince = null;
  }

  // The receiver's current applied epoch.
  lastEpoch() {
    return this.coordinator.lastEpoch;
  }

  // Whether the sink is currently stalled (last send failed, awaiting reconnect).
  isStalled() {
    return this.stalledSince !== null;
  }

  // Millis the sink has been stalled as of `now`, or 0 when healthy — a backoff
  // signal for the host scheduler (which owns cadence/backoff policy).
  stalledFor(now) {
    return this.stalledSince === null ? 0 : Math.max(0, now - this.stalledSince);
  }

  // Deliver one frame; a `false` return OR a throw is a send failure (the outbox
  // already holds the frame, so it is retained + replayed on reconnect).
  #trySend(msg) {
    try {
      return this.sink.send(msg) !== false;
    } catch {
      return false;
    }
  }

  // Run one loop pass. Sink failures retain-and-stall (not an error); only an
  // inbound source read failure throws a DriverError. Returns a Progress.
  tick() {
    const now = this.clock.nowMillis();
    const progress = new Progress();

    // 1. resync-on-reconnect: replay the unacked outbox suffix, oldest first.
    if (this.replayPending) {
      this.replayPending = false;
      for (const [, msg] of this.outbox.replayFrom(this.peerAckedThrough)) {
        if (this.#trySend(msg)) {
          progress.sent += 1;
        } else {
          this.stalledSince = now;
          this.replayPending = true; // finish the replay after the next reconnect
          break;
        }
      }
    }

    // 2. drain fresh enqueues: append-before-send, retain-and-stop on failure.
    //    A pre-existing stall skips the drain — never push into a sink already
    //    known to be down.
    while (this.stalledSince === null && this.pending.length > 0) {
      const [epoch, msg] = this.pending[0];
      this.outbox.append(epoch, msg); // append BEFORE send: at-least-once durability
      this.pending.shift();
      if (this.#trySend(msg)) {
        progress.sent += 1;
      } else {
        this.stalledSince = now; // retained in the outbox → replayed on reconnect
        break;
      }
    }

    // 3. receive: route control frames + feed data frames through the coordinator.
    for (;;) {
      let msg;
      try {
        msg = this.source.recv();
      } catch (error) {
        throw new DriverError(error);
      }
      if (msg == null) break; // exhausted/closed → no inbound progress this tick
      if (msg.isOutboxAck) {
        const through = msg.outboxAck.throughEpoch;
        if (through > this.peerAckedThrough) this.peerAckedThrough = through;
        this.outbox.ackThrough(through);
      } else if (msg.isResyncRequest) {
        const snap = this.provider.snapshot(msg.resyncRequest.fromEpoch);
        if (this.#trySend(snap)) progress.snapshotsServed += 1;
        else this.stalledSince = now;
      } else if (msg.isCrdtSync) {
        progress.applied.push(msg); // idempotent anti-entropy plane — host folds it
      } else if (msg.isSnapshot || msg.isDelta) {
        const res = this.coordinator.ingest(msg);
        if (res.action === ResyncAction.Apply) {
          this.ackOwed = true;
          progress.applied.push(msg);
        } else if (res.action === ResyncAction.RequestSnapshot) {
          const req = IpcMessage.resyncRequestMessage(new ResyncRequest({ fromEpoch: res.fromEpoch }));
          if (this.#trySend(req)) progress.resyncRequested = true;
          else this.stalledSince = now;
        }
        // Ignore → drop
      }
    }

    // 4. advertise our receiver cursor if we applied anything (retry until sent).
    if (this.ackOwed && this.#trySend(this.coordinator.ack())) {
      this.ackOwed = false;
    }

    progress.peerAckedThrough = this.peerAckedThrough;
    progress.retained = this.outbox.retainedEpochs().length;
    return progress;
  }
}

function assertString(value, name) {
  if (typeof value !== "string") {
    throw new TypeError(`${name} must be a string`);
  }
  return value;
}

function assertBoolean(value, name) {
  if (typeof value !== "boolean") {
    throw new TypeError(`${name} must be a boolean`);
  }
  return value;
}

function assertStringArray(value, name) {
  if (!Array.isArray(value)) {
    throw new TypeError(`${name} must be an array of strings`);
  }
  return value.map((item, index) => {
    if (typeof item !== "string") {
      throw new TypeError(`${name}[${index}] must be a string`);
    }
    return item;
  });
}

export class SessionHandshake {
  constructor(fields) {
    const obj = assertObject(fields, "SessionHandshake");
    this.protocolId = assertString(obj.protocol_id, "protocol_id");
    this.protocolMajorVersion = assertInteger(
      obj.protocol_major_version,
      "protocol_major_version",
    );
    this.codec = assertString(obj.codec, "codec");
    this.maxFrameSize = assertInteger(obj.max_frame_size, "max_frame_size");
    this.fragmentationSupported = assertBoolean(
      obj.fragmentation_supported,
      "fragmentation_supported",
    );
    this.orderedReliable = assertBoolean(obj.ordered_reliable, "ordered_reliable");
    this.peerId = assertInteger(obj.peer_id, "peer_id");
    this.sessionId = assertString(obj.session_id, "session_id");
    this.features = Object.freeze(assertStringArray(obj.features ?? [], "features"));
    Object.freeze(this);
  }

  toWire() {
    return {
      protocol_id: this.protocolId,
      protocol_major_version: this.protocolMajorVersion,
      codec: this.codec,
      max_frame_size: this.maxFrameSize,
      fragmentation_supported: this.fragmentationSupported,
      ordered_reliable: this.orderedReliable,
      peer_id: this.peerId,
      session_id: this.sessionId,
      features: [...this.features],
    };
  }

  encodeJson() {
    return textEncoder.encode(JSON.stringify(this.toWire()));
  }

  // Fail-closed compatibility check (protocol.md § Capability Negotiation).
  // Peers are compatible only when they agree on `protocol_id`,
  // `protocol_major_version`, `codec`, and `ordered_reliable`, and `other`
  // offers every feature this side requires.
  checkCompatible(other, requiredFeatures = []) {
    if (this.protocolId !== PROTOCOL_ID) {
      return { ok: false, field: "protocol_id", reason: `expected "${PROTOCOL_ID}"` };
    }
    if (other.protocolId !== PROTOCOL_ID) {
      return { ok: false, field: "protocol_id", reason: `peer is not ${PROTOCOL_ID}` };
    }
    if (this.protocolMajorVersion !== other.protocolMajorVersion) {
      return {
        ok: false,
        field: "protocol_major_version",
        reason: `${this.protocolMajorVersion} != ${other.protocolMajorVersion}`,
      };
    }
    if (this.codec !== other.codec) {
      return { ok: false, field: "codec", reason: `${this.codec} != ${other.codec}` };
    }
    if (this.orderedReliable !== other.orderedReliable) {
      return {
        ok: false,
        field: "ordered_reliable",
        reason: `${this.orderedReliable} != ${other.orderedReliable}`,
      };
    }
    const offered = new Set(other.features);
    for (const required of requiredFeatures) {
      if (!offered.has(required)) {
        return {
          ok: false,
          field: "features",
          reason: `peer does not offer required feature "${required}"`,
        };
      }
    }
    return { ok: true };
  }

  static fromWire(value) {
    return new SessionHandshake(assertObject(value, "SessionHandshake"));
  }

  static decodeJson(data) {
    const text =
      data instanceof Uint8Array ? textDecoder.decode(data) : String(data);
    return SessionHandshake.fromWire(JSON.parse(text));
  }
}

// lazily-js binding-level conformance declaration. The conformance matrix lists
// each layer as MUST; a binding that structurally cannot host a layer MUST
// advertise the omission (capability negotiation / fail-closed) rather than stay
// silent. lazily-js runs on browser/Worker JS — a platform with no shared
// in-process address space — so it declares the C-ABI FFI carve-out
// (`ffi = none`) per protocol.md § "C-ABI FFI is required". Every other MUST
// layer is shipped: the reactive core, keyed collections, the semantic tree,
// the sequence + text CRDTs, IPC, state machine, state charts, permissions, and
// capability negotiation.
export const FfiCapability = Object.freeze({
  Host: "host",
  None: "none",
});

export const BINDING_CAPABILITIES = Object.freeze({
  binding: "lazily-js",
  // C-ABI FFI: carve-out — browser/Worker JS cannot host a native in-process ABI.
  // Must NOT be advertised as embeddable; still exposes the full state plane
  // (incl. CrdtSync) over IPC/WebSocket/WebRTC.
  ffi: FfiCapability.None,
  // Reactive core (Cell/Slot/Effect/Signal): shipped.
  reactive_core: true,
  // Async reactive context: shipped (./reactive-async — Promise-driven derivations
  // with revision-guarded stale-completion discard, in-flight dedup, cancellation).
  async_context: true,
  // Shipped MUST surfaces:
  ipc: true,
  crdt: true,
  collections: { cellmap: true, celltree: true, reconcile: true },
  sem_tree: true,
  seq_crdt: true,
  text_crdt: true,
  stable_id: true,
  state_machine: true,
  state_charts: true,
  permissions: true,
  capability_negotiation: true,
  causal_receipts: true,
  // Optional (MAY) transports — bridged by this binding (./signaling,
  // ./distributed): the kebab-tagged signaling wire protocol + room routing and
  // the WebRTC DataChannel IPC transport + CRDT anti-entropy runtime. Real
  // WebRTC is reached through a browser platform adapter (no npm dependency).
  signaling: true,
  webrtc: true,
});

export const OpKind = Object.freeze({
  Read: "read",
  Write: "write",
  TriggerEffect: "trigger_effect",
});

export class RemoteOp {
  constructor(kind, node) {
    this.kind = kind;
    this.node = assertInteger(node, "node");
    Object.freeze(this);
  }

  static read(node) {
    return new RemoteOp(OpKind.Read, node);
  }

  static write(node) {
    return new RemoteOp(OpKind.Write, node);
  }

  static triggerEffect(node) {
    return new RemoteOp(OpKind.TriggerEffect, node);
  }
}

export class PermissionDenied extends Error {
  constructor(peer, op) {
    super(`peer ${peer} denied ${op.kind} on node ${op.node}`);
    this.name = "PermissionDenied";
    this.peer = peer;
    this.op = op;
  }
}

export class PeerPermissions {
  #peers = new Map();

  allow(peer, op) {
    const peerId = assertInteger(peer, "peer");
    let peerPerms = this.#peers.get(peerId);
    if (!peerPerms) {
      peerPerms = new Map();
      this.#peers.set(peerId, peerPerms);
    }
    let nodes = peerPerms.get(op.kind);
    if (!nodes) {
      nodes = new Set();
      peerPerms.set(op.kind, nodes);
    }
    const had = nodes.has(op.node);
    nodes.add(op.node);
    return !had;
  }

  allowMany(peer, kind, nodes) {
    for (const node of nodes) {
      this.allow(peer, new RemoteOp(kind, node));
    }
  }

  revoke(peer, op) {
    const peerPerms = this.#peers.get(peer);
    const nodes = peerPerms?.get(op.kind);
    if (!nodes?.delete(op.node)) {
      return false;
    }
    this.#prune(peer);
    return true;
  }

  revokePeer(peer) {
    return this.#peers.delete(peer);
  }

  isAllowed(peer, op) {
    return this.#peers.get(peer)?.get(op.kind)?.has(op.node) === true;
  }

  canRead(peer, node) {
    return this.isAllowed(peer, RemoteOp.read(node));
  }

  check(peer, op) {
    if (!this.isAllowed(peer, op)) {
      throw new PermissionDenied(peer, op);
    }
  }

  filterReadable(peer, nodes) {
    return [...nodes].filter((node) => this.canRead(peer, node));
  }

  peerCount() {
    return this.#peers.size;
  }

  #prune(peer) {
    const peerPerms = this.#peers.get(peer);
    if (!peerPerms) {
      return;
    }
    for (const [kind, nodes] of peerPerms.entries()) {
      if (nodes.size === 0) {
        peerPerms.delete(kind);
      }
    }
    if (peerPerms.size === 0) {
      this.#peers.delete(peer);
    }
  }
}

// ===========================================================================
// Command / RPC message plane (command-plane-v1)
//
// An evented command message family that is an additive sibling to
// Snapshot / Delta / CrdtSync. The one hard rule: terminal authority is the
// causal receipt, not the event or the transport. observed/accepted/started
// events are non-terminal progress; a command becomes terminal only when a
// terminal CausalReceipt folds in. CommandRpcClient is derived behavior over
// the CommandProjection reducer — a unary call resolves only on a terminal
// projection.
// ===========================================================================

export const DedupePolicy = Object.freeze({
  None: "none",
  SameIdempotencyKey: "same_idempotency_key",
  SameCommandId: "same_command_id",
});

function normalizeDedupePolicy(value) {
  if (Object.values(DedupePolicy).includes(value)) {
    return value;
  }
  throw new TypeError(`unknown dedupe policy: ${value}`);
}

export class CommandPolicy {
  constructor({ dedupe, supersede, cancelOnPreempt }) {
    this.dedupe = normalizeDedupePolicy(dedupe);
    if (typeof supersede !== "boolean") {
      throw new TypeError("supersede must be a boolean");
    }
    if (typeof cancelOnPreempt !== "boolean") {
      throw new TypeError("cancelOnPreempt must be a boolean");
    }
    this.supersede = supersede;
    this.cancelOnPreempt = cancelOnPreempt;
    Object.freeze(this);
  }

  toWire() {
    return {
      dedupe: this.dedupe,
      supersede: this.supersede,
      cancel_on_preempt: this.cancelOnPreempt,
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "CommandPolicy");
    return new CommandPolicy({
      dedupe: object.dedupe,
      supersede: object.supersede,
      cancelOnPreempt: object.cancel_on_preempt,
    });
  }
}

export class CommandSubmit {
  constructor({
    commandId,
    causationId,
    source,
    target,
    namespace,
    name,
    authorityGeneration,
    idempotencyKey,
    deadlineMs,
    policy,
    payloadType,
    payloadHash,
    payload,
    requiredFeatures = [],
  }) {
    this.commandId = assertString(commandId, "commandId");
    this.causationId = assertString(causationId, "causationId");
    this.source = assertString(source, "source");
    this.target = assertString(target, "target");
    this.namespace = assertString(namespace, "namespace");
    this.name = assertString(name, "name");
    this.authorityGeneration = assertInteger(authorityGeneration, "authorityGeneration");
    this.idempotencyKey = assertString(idempotencyKey, "idempotencyKey");
    this.deadlineMs = assertInteger(deadlineMs, "deadlineMs");
    this.policy = policy instanceof CommandPolicy ? policy : CommandPolicy.fromWire(policy);
    this.payloadType = assertString(payloadType, "payloadType");
    this.payloadHash = assertString(payloadHash, "payloadHash");
    this.payload = IpcValue.of(payload);
    this.requiredFeatures = Object.freeze(assertStringArray(requiredFeatures, "requiredFeatures"));
    Object.freeze(this);
  }

  toWire() {
    return {
      command_id: this.commandId,
      causation_id: this.causationId,
      source: this.source,
      target: this.target,
      namespace: this.namespace,
      name: this.name,
      authority_generation: this.authorityGeneration,
      idempotency_key: this.idempotencyKey,
      deadline_ms: this.deadlineMs,
      policy: this.policy.toWire(),
      payload_type: this.payloadType,
      payload_hash: this.payloadHash,
      payload: this.payload.toWire(),
      required_features: [...this.requiredFeatures],
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "CommandSubmit");
    return new CommandSubmit({
      commandId: object.command_id,
      causationId: object.causation_id,
      source: object.source,
      target: object.target,
      namespace: object.namespace,
      name: object.name,
      authorityGeneration: object.authority_generation,
      idempotencyKey: object.idempotency_key,
      deadlineMs: object.deadline_ms,
      policy: CommandPolicy.fromWire(object.policy),
      payloadType: object.payload_type,
      payloadHash: object.payload_hash,
      payload: IpcValue.fromWire(object.payload),
      requiredFeatures: object.required_features ?? [],
    });
  }
}

export class CommandCancel {
  constructor({ commandId, causationId, source, authorityGeneration, reason = null }) {
    this.commandId = assertString(commandId, "commandId");
    this.causationId = assertString(causationId, "causationId");
    this.source = assertString(source, "source");
    this.authorityGeneration = assertInteger(authorityGeneration, "authorityGeneration");
    this.reason = reason === null ? null : assertString(reason, "reason");
    Object.freeze(this);
  }

  toWire() {
    return {
      command_id: this.commandId,
      causation_id: this.causationId,
      source: this.source,
      authority_generation: this.authorityGeneration,
      reason: this.reason,
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "CommandCancel");
    return new CommandCancel({
      commandId: object.command_id,
      causationId: object.causation_id,
      source: object.source,
      authorityGeneration: object.authority_generation,
      reason: object.reason ?? null,
    });
  }
}

export const CommandEventKind = Object.freeze({
  Observed: "observed",
  Accepted: "accepted",
  Started: "started",
  Progress: "progress",
  Cancelled: "cancelled",
  Superseded: "superseded",
  TimedOut: "timed_out",
});

function normalizeCommandEventKind(value) {
  if (Object.values(CommandEventKind).includes(value)) {
    return value;
  }
  throw new TypeError(`unknown command event kind: ${value}`);
}

export class CommandEvent {
  constructor({ eventId, commandId, kind, generation, detail = null }) {
    this.eventId = assertString(eventId, "eventId");
    this.commandId = assertString(commandId, "commandId");
    this.kind = normalizeCommandEventKind(kind);
    this.generation = assertInteger(generation, "generation");
    this.detail = detail === null ? null : assertString(detail, "detail");
    Object.freeze(this);
  }

  toWire() {
    return {
      event_id: this.eventId,
      command_id: this.commandId,
      kind: this.kind,
      generation: this.generation,
      detail: this.detail,
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "CommandEvent");
    return new CommandEvent({
      eventId: object.event_id,
      commandId: object.command_id,
      kind: object.kind,
      generation: object.generation,
      detail: object.detail ?? null,
    });
  }
}

export class CommandEvents {
  constructor(events = []) {
    this.events = Object.freeze(
      events.map((event) => (event instanceof CommandEvent ? event : CommandEvent.fromWire(event))),
    );
    Object.freeze(this);
  }

  toWire() {
    return { events: this.events.map((event) => event.toWire()) };
  }

  static fromWire(value) {
    const object = assertObject(value, "CommandEvents");
    return new CommandEvents(object.events ?? []);
  }
}

export const CommandStatus = Object.freeze({
  Submitted: "submitted",
  Accepted: "accepted",
  Running: "running",
  Applied: "applied",
  Rejected: "rejected",
  Cancelled: "cancelled",
  Superseded: "superseded",
  TimedOut: "timed_out",
});

const TERMINAL_COMMAND_STATUSES = Object.freeze([
  CommandStatus.Applied,
  CommandStatus.Rejected,
  CommandStatus.Cancelled,
  CommandStatus.Superseded,
  CommandStatus.TimedOut,
]);

export function isTerminalCommandStatus(status) {
  return TERMINAL_COMMAND_STATUSES.includes(status);
}

function normalizeCommandStatus(value) {
  if (Object.values(CommandStatus).includes(value)) {
    return value;
  }
  throw new TypeError(`unknown command status: ${value}`);
}

export class CommandProjectionEntry {
  constructor({
    commandId,
    status,
    terminal,
    generation,
    reason = null,
    terminalReceiptId = null,
    lastEventId = null,
  }) {
    this.commandId = assertString(commandId, "commandId");
    this.status = normalizeCommandStatus(status);
    if (typeof terminal !== "boolean") {
      throw new TypeError("terminal must be a boolean");
    }
    this.terminal = terminal;
    this.generation = assertInteger(generation, "generation");
    this.reason = reason === null ? null : assertString(reason, "reason");
    this.terminalReceiptId =
      terminalReceiptId === null ? null : assertString(terminalReceiptId, "terminalReceiptId");
    this.lastEventId = lastEventId === null ? null : assertString(lastEventId, "lastEventId");
    Object.freeze(this);
  }

  toWire() {
    return {
      command_id: this.commandId,
      status: this.status,
      terminal: this.terminal,
      generation: this.generation,
      reason: this.reason,
      terminal_receipt_id: this.terminalReceiptId,
      last_event_id: this.lastEventId,
    };
  }

  with(patch) {
    return new CommandProjectionEntry({ ...this, ...patch });
  }

  static fromWire(value) {
    const object = assertObject(value, "CommandProjectionEntry");
    return new CommandProjectionEntry({
      commandId: object.command_id,
      status: object.status,
      terminal: object.terminal,
      generation: object.generation,
      reason: object.reason ?? null,
      terminalReceiptId: object.terminal_receipt_id ?? null,
      lastEventId: object.last_event_id ?? null,
    });
  }
}

export class CommandProjectionImage {
  constructor(generation, commands = []) {
    this.generation = assertInteger(generation, "generation");
    this.commands = Object.freeze(
      commands.map((entry) =>
        entry instanceof CommandProjectionEntry ? entry : CommandProjectionEntry.fromWire(entry),
      ),
    );
    Object.freeze(this);
  }

  toWire() {
    return {
      generation: this.generation,
      commands: this.commands.map((entry) => entry.toWire()),
    };
  }

  static fromWire(value) {
    const object = assertObject(value, "CommandProjectionImage");
    return new CommandProjectionImage(object.generation, object.commands ?? []);
  }
}

export class CommandMessage {
  constructor(kind, value) {
    this.kind = kind;
    this.submit = undefined;
    this.cancel = undefined;
    this.events = undefined;
    this.projection = undefined;
    switch (kind) {
      case "CommandSubmit":
        this.submit = value instanceof CommandSubmit ? value : CommandSubmit.fromWire(value);
        break;
      case "CommandCancel":
        this.cancel = value instanceof CommandCancel ? value : CommandCancel.fromWire(value);
        break;
      case "CommandEvents":
        this.events = value instanceof CommandEvents ? value : CommandEvents.fromWire(value);
        break;
      case "CommandProjection":
        this.projection =
          value instanceof CommandProjectionImage ? value : CommandProjectionImage.fromWire(value);
        break;
      default:
        throw new TypeError(`unknown CommandMessage kind: ${kind}`);
    }
    Object.freeze(this);
  }

  toWire() {
    switch (this.kind) {
      case "CommandSubmit":
        return { CommandSubmit: this.submit.toWire() };
      case "CommandCancel":
        return { CommandCancel: this.cancel.toWire() };
      case "CommandEvents":
        return { CommandEvents: this.events.toWire() };
      case "CommandProjection":
        return { CommandProjection: this.projection.toWire() };
      default:
        throw new TypeError(`unknown CommandMessage kind: ${this.kind}`);
    }
  }

  encodeJson() {
    return textEncoder.encode(JSON.stringify(this.toWire()));
  }

  static ofSubmit(submit) {
    return new CommandMessage("CommandSubmit", submit);
  }

  static ofCancel(cancel) {
    return new CommandMessage("CommandCancel", cancel);
  }

  static ofEvents(events) {
    return new CommandMessage("CommandEvents", events);
  }

  static ofProjection(image) {
    return new CommandMessage("CommandProjection", image);
  }

  static fromWire(value) {
    const [tag, body] = assertTagged(value, "CommandMessage");
    switch (tag) {
      case "CommandSubmit":
        return CommandMessage.ofSubmit(CommandSubmit.fromWire(body));
      case "CommandCancel":
        return CommandMessage.ofCancel(CommandCancel.fromWire(body));
      case "CommandEvents":
        return CommandMessage.ofEvents(CommandEvents.fromWire(body));
      case "CommandProjection":
        return CommandMessage.ofProjection(CommandProjectionImage.fromWire(body));
      default:
        throw new TypeError(`unknown CommandMessage variant: ${tag}`);
    }
  }

  static decodeJson(data) {
    const text = data instanceof Uint8Array ? textDecoder.decode(data) : String(data);
    return CommandMessage.fromWire(JSON.parse(text));
  }
}

export const CommandApplyStatusKind = Object.freeze({
  Recorded: "recorded",
  Duplicate: "duplicate",
  Unknown: "unknown",
  StaleGeneration: "stale_generation",
  TerminalConflict: "terminal_conflict",
});

function terminalStatusOf(outcome, reason) {
  if (outcome === ReceiptOutcome.Applied) {
    return CommandStatus.Applied;
  }
  if (outcome === ReceiptOutcome.Rejected) {
    switch (reason) {
      case "cancelled":
        return CommandStatus.Cancelled;
      case "superseded":
        return CommandStatus.Superseded;
      case "timed_out":
        return CommandStatus.TimedOut;
      default:
        return CommandStatus.Rejected;
    }
  }
  return CommandStatus.Accepted;
}

function progressStatusOf(kind) {
  switch (kind) {
    case CommandEventKind.Observed:
    case CommandEventKind.Accepted:
      return CommandStatus.Accepted;
    case CommandEventKind.Started:
    case CommandEventKind.Progress:
      return CommandStatus.Running;
    default:
      return null;
  }
}

function phaseRank(status) {
  switch (status) {
    case CommandStatus.Submitted:
      return 0;
    case CommandStatus.Accepted:
      return 1;
    case CommandStatus.Running:
      return 2;
    default:
      return 3;
  }
}

export class CommandProjection {
  #generation = 0;
  #entries = new Map();
  #seenEventIds = new Set();
  #seenReceiptIds = new Set();
  #seenCancelIds = new Set();
  #conflicts = new Set();

  get generation() {
    return this.#generation;
  }

  applyMessage(message) {
    const msg = message instanceof CommandMessage ? message : CommandMessage.fromWire(message);
    switch (msg.kind) {
      case "CommandSubmit":
        return this.submit(msg.submit);
      case "CommandCancel":
        return this.cancel(msg.cancel);
      case "CommandEvents": {
        let last = { kind: CommandApplyStatusKind.Unknown };
        for (const event of msg.events.events) {
          last = this.event(event);
        }
        return last;
      }
      case "CommandProjection":
        return this.applyProjection(msg.projection);
      default:
        throw new TypeError(`unknown CommandMessage kind: ${msg.kind}`);
    }
  }

  submit(submit) {
    const s = submit instanceof CommandSubmit ? submit : CommandSubmit.fromWire(submit);
    if (this.#entries.has(s.commandId)) {
      return { kind: CommandApplyStatusKind.Duplicate };
    }
    this.#generation = Math.max(this.#generation, s.authorityGeneration);
    this.#entries.set(
      s.commandId,
      new CommandProjectionEntry({
        commandId: s.commandId,
        status: CommandStatus.Submitted,
        terminal: false,
        generation: s.authorityGeneration,
      }),
    );
    return { kind: CommandApplyStatusKind.Recorded };
  }

  event(event) {
    const e = event instanceof CommandEvent ? event : CommandEvent.fromWire(event);
    if (this.#seenEventIds.has(e.eventId)) {
      return { kind: CommandApplyStatusKind.Duplicate };
    }
    const entry = this.#entries.get(e.commandId);
    if (!entry) {
      return { kind: CommandApplyStatusKind.Unknown };
    }
    if (e.generation !== entry.generation) {
      return {
        kind: CommandApplyStatusKind.StaleGeneration,
        expected: entry.generation,
        actual: e.generation,
      };
    }
    this.#seenEventIds.add(e.eventId);
    let updated = entry.with({ lastEventId: e.eventId });
    const next = progressStatusOf(e.kind);
    if (!updated.terminal && next !== null && phaseRank(next) >= phaseRank(updated.status)) {
      updated = updated.with({ status: next });
    }
    this.#entries.set(e.commandId, updated);
    return { kind: CommandApplyStatusKind.Recorded };
  }

  cancel(cancel) {
    const c = cancel instanceof CommandCancel ? cancel : CommandCancel.fromWire(cancel);
    if (this.#seenCancelIds.has(c.causationId)) {
      return { kind: CommandApplyStatusKind.Duplicate };
    }
    const entry = this.#entries.get(c.commandId);
    if (!entry) {
      return { kind: CommandApplyStatusKind.Unknown };
    }
    if (c.authorityGeneration !== entry.generation) {
      return {
        kind: CommandApplyStatusKind.StaleGeneration,
        expected: entry.generation,
        actual: c.authorityGeneration,
      };
    }
    this.#seenCancelIds.add(c.causationId);
    // A cancel is non-terminal by itself; the rejected receipt makes it terminal.
    return { kind: CommandApplyStatusKind.Recorded };
  }

  observeReceipt(receipt) {
    const r = receipt instanceof CausalReceipt ? receipt : CausalReceipt.fromWire(receipt);
    if (this.#seenReceiptIds.has(r.receiptId)) {
      return { kind: CommandApplyStatusKind.Duplicate };
    }
    const entry = this.#entries.get(r.causationId);
    if (!entry) {
      return { kind: CommandApplyStatusKind.Unknown };
    }
    if (r.generation !== entry.generation) {
      return {
        kind: CommandApplyStatusKind.StaleGeneration,
        expected: entry.generation,
        actual: r.generation,
      };
    }
    if (!r.isTerminal) {
      this.#seenReceiptIds.add(r.receiptId);
      if (!entry.terminal && phaseRank(CommandStatus.Accepted) >= phaseRank(entry.status)) {
        this.#entries.set(r.causationId, entry.with({ status: CommandStatus.Accepted }));
      }
      return { kind: CommandApplyStatusKind.Recorded };
    }
    const incoming = terminalStatusOf(r.outcome, r.reason);
    if (entry.terminal) {
      if (entry.status === incoming) {
        this.#seenReceiptIds.add(r.receiptId);
        return { kind: CommandApplyStatusKind.Recorded };
      }
      this.#conflicts.add(r.causationId);
      return {
        kind: CommandApplyStatusKind.TerminalConflict,
        commandId: r.causationId,
        existing: entry.status,
        incoming,
      };
    }
    this.#seenReceiptIds.add(r.receiptId);
    this.#entries.set(
      r.causationId,
      entry.with({
        terminal: true,
        status: incoming,
        reason: r.reason,
        terminalReceiptId: r.receiptId,
      }),
    );
    return { kind: CommandApplyStatusKind.Recorded };
  }

  applyProjection(image) {
    const img = image instanceof CommandProjectionImage ? image : CommandProjectionImage.fromWire(image);
    this.#generation = Math.max(this.#generation, img.generation);
    for (const entry of img.commands) {
      this.#entries.set(entry.commandId, entry);
      if (entry.lastEventId !== null) {
        this.#seenEventIds.add(entry.lastEventId);
      }
      if (entry.terminalReceiptId !== null) {
        this.#seenReceiptIds.add(entry.terminalReceiptId);
      }
    }
    return { kind: CommandApplyStatusKind.Recorded };
  }

  entry(commandId) {
    return this.#entries.get(commandId) ?? null;
  }

  terminalFor(commandId) {
    const entry = this.#entries.get(commandId);
    return entry && entry.terminal ? entry : null;
  }

  hasConflict(commandId) {
    return this.#conflicts.has(commandId);
  }

  toImage() {
    const commands = [...this.#entries.values()].sort((a, b) =>
      a.commandId < b.commandId ? -1 : a.commandId > b.commandId ? 1 : 0,
    );
    return new CommandProjectionImage(this.#generation, commands);
  }
}

export const CallStateKind = Object.freeze({
  Pending: "pending",
  Resolved: "resolved",
  Conflict: "conflict",
});

// Functional helper: build + send a CommandSubmit through a transport function
// and fold it into a projection. Returns the command id.
export function submitCommand(transport, projection, submit) {
  const message = CommandMessage.ofSubmit(submit);
  transport(message);
  projection.applyMessage(message);
  return submit.commandId;
}

// Functional helper: build + send a CommandCancel through a transport function.
export function cancelCommand(transport, projection, cancel) {
  const message = CommandMessage.ofCancel(cancel);
  transport(message);
  projection.applyMessage(message);
}

export class CommandRpcClient {
  #transport;
  projection = new CommandProjection();

  constructor(transport) {
    if (typeof transport !== "function") {
      throw new TypeError("transport must be a function (message) => void");
    }
    this.#transport = transport;
  }

  submit(submit) {
    return submitCommand(this.#transport, this.projection, submit);
  }

  cancel(cancel) {
    cancelCommand(this.#transport, this.projection, cancel);
  }

  ingestCommand(message) {
    return this.projection.applyMessage(message);
  }

  ingestReceipt(receipt) {
    return this.projection.observeReceipt(receipt);
  }

  pollCall(commandId) {
    if (this.projection.hasConflict(commandId)) {
      return { kind: CallStateKind.Conflict };
    }
    const entry = this.projection.terminalFor(commandId);
    return entry
      ? { kind: CallStateKind.Resolved, entry }
      : { kind: CallStateKind.Pending };
  }
}
