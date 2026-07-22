export type NodeId = number;
export type PeerId = number;
export type WireBytes = readonly number[] | Uint8Array;

export type BlobBackendKindValue = "shm" | "arrow" | "in_process";

export const BlobBackendKind: {
  readonly Shm: "shm";
  readonly Arrow: "arrow";
  readonly InProcess: "in_process";
};

export class ShmBlobRef {
  constructor(fields: {
    offset: number;
    len: number;
    generation: number;
    epoch: number;
    checksum: number;
    backend?: BlobBackendKindValue;
  });
  readonly offset: number;
  readonly len: number;
  readonly generation: number;
  readonly epoch: number;
  readonly checksum: number;
  readonly backend: BlobBackendKindValue;
  withBackend(backend: BlobBackendKindValue): ShmBlobRef;
  toWire(): ShmBlobRefWire;
  static fromWire(value: ShmBlobRefWire): ShmBlobRef;
}

export type ShmBlobRefWire = {
  offset: number;
  len: number;
  generation: number;
  epoch: number;
  checksum: number;
  backend?: BlobBackendKindValue;
};

export class NodeStatePayload {
  constructor(bytes: WireBytes);
  readonly bytes: readonly number[];
  toWire(): { Payload: number[] };
}

export class NodeStateSharedBlob {
  constructor(blob: ShmBlobRef | ShmBlobRefWire);
  readonly blob: ShmBlobRef;
  toWire(): { SharedBlob: ShmBlobRefWire };
}

export class NodeStateOpaque {
  toWire(): "Opaque";
}

export type NodeStateValue =
  | NodeStatePayload
  | NodeStateSharedBlob
  | NodeStateOpaque;

export const NodeState: {
  payload(bytes: WireBytes): NodeStatePayload;
  sharedBlob(blob: ShmBlobRef | ShmBlobRefWire): NodeStateSharedBlob;
  opaque(): NodeStateOpaque;
  fromWire(value: unknown): NodeStateValue;
};

export class IpcValueInline {
  constructor(bytes: WireBytes);
  readonly bytes: readonly number[];
  toWire(): { Inline: number[] };
}

export class IpcValueSharedBlob {
  constructor(blob: ShmBlobRef | ShmBlobRefWire);
  readonly blob: ShmBlobRef;
  toWire(): { SharedBlob: ShmBlobRefWire };
}

export type IpcValueValue = IpcValueInline | IpcValueSharedBlob;

export const IpcValue: {
  inline(bytes: WireBytes): IpcValueInline;
  sharedBlob(blob: ShmBlobRef | ShmBlobRefWire): IpcValueSharedBlob;
  of(value: IpcValueValue | ShmBlobRef | ShmBlobRefWire | WireBytes): IpcValueValue;
  fromWire(value: unknown): IpcValueValue;
};

export class NodeSnapshot {
  constructor(node: NodeId, typeTag: string, state: NodeStateValue, key?: string | null);
  readonly node: NodeId;
  readonly typeTag: string;
  readonly state: NodeStateValue;
  readonly key: string | null;
  toWire(): unknown;
  static payload(node: NodeId, typeTag: string, bytes: WireBytes, key?: string | null): NodeSnapshot;
  static sharedBlob(
    node: NodeId,
    typeTag: string,
    blob: ShmBlobRef | ShmBlobRefWire,
    key?: string | null,
  ): NodeSnapshot;
  static opaque(node: NodeId, typeTag: string, key?: string | null): NodeSnapshot;
  static fromWire(value: unknown): NodeSnapshot;
}

export class EdgeSnapshot {
  constructor(dependent: NodeId, dependency: NodeId);
  readonly dependent: NodeId;
  readonly dependency: NodeId;
  toWire(): { dependent: NodeId; dependency: NodeId };
  isReadableBy(permissions: PeerPermissions, peer: PeerId): boolean;
  static fromWire(value: unknown): EdgeSnapshot;
}

export class Snapshot {
  constructor(fields: {
    epoch: number;
    nodes?: readonly NodeSnapshot[];
    edges?: readonly EdgeSnapshot[];
    roots?: readonly NodeId[];
  });
  readonly epoch: number;
  readonly nodes: readonly NodeSnapshot[];
  readonly edges: readonly EdgeSnapshot[];
  readonly roots: readonly NodeId[];
  toWire(): unknown;
  filterReadable(permissions: PeerPermissions, peer: PeerId): Snapshot;
  static fromWire(value: unknown): Snapshot;
}

export type DeltaOpValue =
  | DeltaOpCellSet
  | DeltaOpSlotValue
  | DeltaOpInvalidate
  | DeltaOpNodeAdd
  | DeltaOpNodeRemove
  | DeltaOpEdgeAdd
  | DeltaOpEdgeRemove;

export class DeltaOpCellSet {
  constructor(node: NodeId, payload: IpcValueValue | ShmBlobRef | WireBytes);
  readonly node: NodeId;
  readonly payload: IpcValueValue;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
}

export class DeltaOpSlotValue {
  constructor(node: NodeId, payload: IpcValueValue | ShmBlobRef | WireBytes);
  readonly node: NodeId;
  readonly payload: IpcValueValue;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
}

export class DeltaOpInvalidate {
  constructor(node: NodeId);
  readonly node: NodeId;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
}

export class DeltaOpNodeAdd {
  constructor(node: NodeId, typeTag: string, state: NodeStateValue, key?: string | null);
  readonly node: NodeId;
  readonly typeTag: string;
  readonly state: NodeStateValue;
  readonly key: string | null;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
}

export class DeltaOpNodeRemove {
  constructor(node: NodeId);
  readonly node: NodeId;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
}

export class DeltaOpEdgeAdd {
  constructor(dependent: NodeId, dependency: NodeId);
  readonly dependent: NodeId;
  readonly dependency: NodeId;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
}

export class DeltaOpEdgeRemove {
  constructor(dependent: NodeId, dependency: NodeId);
  readonly dependent: NodeId;
  readonly dependency: NodeId;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
}

export const DeltaOp: {
  cellSet(node: NodeId, payload: IpcValueValue | ShmBlobRef | WireBytes): DeltaOpCellSet;
  slotValue(node: NodeId, payload: IpcValueValue | ShmBlobRef | WireBytes): DeltaOpSlotValue;
  invalidate(node: NodeId): DeltaOpInvalidate;
  nodeAdd(node: NodeId, typeTag: string, state: NodeStateValue, key?: string | null): DeltaOpNodeAdd;
  nodeRemove(node: NodeId): DeltaOpNodeRemove;
  edgeAdd(dependent: NodeId, dependency: NodeId): DeltaOpEdgeAdd;
  edgeRemove(dependent: NodeId, dependency: NodeId): DeltaOpEdgeRemove;
  fromWire(value: unknown): DeltaOpValue;
};

export const DeltaApplyStatusKind: {
  readonly Apply: "apply";
  readonly ResyncRequired: "resync_required";
};

export class DeltaApplyStatus {
  readonly kind: "apply" | "resync_required";
  readonly lastEpoch?: number;
  readonly baseEpoch?: number;
  readonly epoch?: number;
  readonly isApply: boolean;
  readonly isResyncRequired: boolean;
  static apply(): DeltaApplyStatus;
  static resyncRequired(lastEpoch: number, baseEpoch: number, epoch: number): DeltaApplyStatus;
}

export class Delta {
  constructor(fields: {
    baseEpoch: number;
    epoch: number;
    ops?: readonly DeltaOpValue[];
  });
  readonly baseEpoch: number;
  readonly epoch: number;
  readonly ops: readonly DeltaOpValue[];
  isNextAfter(lastEpoch: number): boolean;
  /** Accepted-event span `epoch - baseEpoch` (>= 0); > 1 for a multi-epoch-span delta (#lzsync). */
  span(): number;
  applyStatus(lastEpoch: number): DeltaApplyStatus;
  filterReadable(permissions: PeerPermissions, peer: PeerId): Delta;
  toWire(): unknown;
  static next(baseEpoch: number, ops?: readonly DeltaOpValue[]): Delta;
  static fromWire(value: unknown): Delta;
}

export class WireStamp {
  constructor(fields: { wallTime: number; logical: number; peer: number });
  readonly wallTime: number;
  readonly logical: number;
  readonly peer: number;
  toWire(): WireStampWire;
  static fromWire(value: WireStampWire): WireStamp;
}

export type WireStampWire = {
  wall_time: number;
  logical: number;
  peer: number;
};

export type FrontierEntry = { peer: NodeId; stamp: WireStamp };

export class CrdtOp {
  constructor(
    node: NodeId,
    stamp: WireStamp | WireStampWire,
    state: IpcValueValue | ShmBlobRef | WireBytes,
    key?: string | null,
  );
  readonly node: NodeId;
  readonly stamp: WireStamp;
  readonly state: IpcValueValue;
  readonly key: string | null;
  toWire(): unknown;
  targetReadable(permissions: PeerPermissions, peer: PeerId): boolean;
  static keyed(
    node: NodeId,
    key: string | null,
    stamp: WireStamp | WireStampWire,
    state: IpcValueValue | ShmBlobRef | WireBytes,
  ): CrdtOp;
  static fromWire(value: unknown): CrdtOp;
}

export class CrdtSync {
  constructor(fields: {
    frontier?: ReadonlyArray<FrontierEntry | [NodeId, WireStamp | WireStampWire]>;
    ops?: ReadonlyArray<CrdtOp>;
  });
  readonly frontier: ReadonlyArray<FrontierEntry>;
  readonly ops: ReadonlyArray<CrdtOp>;
  filterReadable(permissions: PeerPermissions, peer: PeerId): CrdtSync;
  toWire(): unknown;
  static fromWire(value: unknown): CrdtSync;
}

export const ReceiptOutcome: {
  readonly Observed: "observed";
  readonly Accepted: "accepted";
  readonly Applied: "applied";
  readonly Rejected: "rejected";
};

export type ReceiptOutcomeValue =
  | "observed"
  | "accepted"
  | "applied"
  | "rejected";

export function isTerminalReceiptOutcome(outcome: ReceiptOutcomeValue): boolean;

export type CausalReceiptWire = {
  receipt_id: string;
  causation_id: string;
  observer: string;
  generation: number;
  outcome: ReceiptOutcomeValue;
  reason: string | null;
  payload_hash: string | null;
};

export class CausalReceipt {
  constructor(fields: {
    receiptId: string;
    causationId: string;
    observer: string;
    generation: number;
    outcome: ReceiptOutcomeValue;
    reason?: string | null;
    payloadHash?: string | null;
  });
  readonly receiptId: string;
  readonly causationId: string;
  readonly observer: string;
  readonly generation: number;
  readonly outcome: ReceiptOutcomeValue;
  readonly reason: string | null;
  readonly payloadHash: string | null;
  readonly isTerminal: boolean;
  toWire(): CausalReceiptWire;
  static observed(receiptId: string, causationId: string, observer: string, generation: number): CausalReceipt;
  static accepted(receiptId: string, causationId: string, observer: string, generation: number): CausalReceipt;
  static applied(receiptId: string, causationId: string, observer: string, generation: number, payloadHash?: string | null): CausalReceipt;
  static rejected(receiptId: string, causationId: string, observer: string, generation: number, reason?: string | null): CausalReceipt;
  static fromWire(value: unknown): CausalReceipt;
}

export class CausalReceipts {
  constructor(receipts?: ReadonlyArray<CausalReceipt | CausalReceiptWire>);
  readonly receipts: readonly CausalReceipt[];
  toWire(): { receipts: CausalReceiptWire[] };
  static fromWire(value: unknown): CausalReceipts;
}

export class ReceiptMessage {
  readonly kind: "CausalReceipts";
  readonly causalReceipts: CausalReceipts;
  toWire(): unknown;
  encodeJson(): Uint8Array;
  static causalReceipts(causalReceipts: CausalReceipts): ReceiptMessage;
  static fromWire(value: unknown): ReceiptMessage;
  static decodeJson(data: Uint8Array | string): ReceiptMessage;
}

export const ReceiptApplyStatusKind: {
  readonly Recorded: "recorded";
  readonly Duplicate: "duplicate";
  readonly StaleGeneration: "stale_generation";
  readonly TerminalConflict: "terminal_conflict";
};

export type ReceiptApplyStatus =
  | { kind: "recorded" }
  | { kind: "duplicate" }
  | { kind: "stale_generation"; expected: number; actual: number }
  | {
      kind: "terminal_conflict";
      causationId: string;
      existing: ReceiptOutcomeValue;
      incoming: ReceiptOutcomeValue;
    };

export class ReceiptProjection {
  observe(currentGeneration: number | null | undefined, receipt: CausalReceipt | CausalReceiptWire): ReceiptApplyStatus;
  latestFor(causationId: string): CausalReceipt | null;
  terminalFor(causationId: string): CausalReceipt | null;
  containsReceipt(receiptId: string): boolean;
  staleReceiptIds(): string[];
}

// Reliable-sync reverse-channel control frames (#lzsync).
export class ResyncRequest {
  constructor(fields: { fromEpoch: number });
  readonly fromEpoch: number;
  toWire(): { from_epoch: number };
  static fromWire(value: unknown): ResyncRequest;
}

export class OutboxAck {
  constructor(fields: { throughEpoch: number });
  readonly throughEpoch: number;
  toWire(): { through_epoch: number };
  static fromWire(value: unknown): OutboxAck;
}

export class IpcMessage {
  readonly kind: "Snapshot" | "Delta" | "CrdtSync" | "ResyncRequest" | "OutboxAck";
  readonly snapshot?: Snapshot;
  readonly delta?: Delta;
  readonly crdtSync?: CrdtSync;
  readonly resyncRequest?: ResyncRequest;
  readonly outboxAck?: OutboxAck;
  readonly isSnapshot: boolean;
  readonly isDelta: boolean;
  readonly isCrdtSync: boolean;
  readonly isResyncRequest: boolean;
  readonly isOutboxAck: boolean;
  readonly isControl: boolean;
  toWire(): unknown;
  encodeJson(): Uint8Array;
  static snapshot(snapshot: Snapshot): IpcMessage;
  static delta(delta: Delta): IpcMessage;
  static crdtSync(crdtSync: CrdtSync): IpcMessage;
  static resyncRequestMessage(request: ResyncRequest): IpcMessage;
  static outboxAckMessage(ack: OutboxAck): IpcMessage;
  static fromWire(value: unknown): IpcMessage;
  static decodeJson(data: Uint8Array | string): IpcMessage;
}

// Reliable sync protocol (#lzsync).
export const ResyncAction: {
  readonly Apply: "Apply";
  readonly RequestSnapshot: "RequestSnapshot";
  readonly Ignore: "Ignore";
};

export type ResyncActionResult = {
  action: "Apply" | "RequestSnapshot" | "Ignore";
  fromEpoch?: number;
};

export class ResyncCoordinator {
  constructor(lastEpoch?: number);
  lastEpoch: number;
  resyncing: boolean;
  ingestDelta(delta: Delta): ResyncActionResult;
  ingestSnapshot(snapshotEpoch: number): ResyncActionResult;
  ingest(msg: IpcMessage): ResyncActionResult;
  ack(): IpcMessage;
}

export interface DurableOutbox {
  append(epoch: number, msg: IpcMessage): void | Promise<void>;
  ackThrough(epoch: number): void | Promise<void>;
  replayFrom(cursor: number): Array<[number, IpcMessage]>;
  retainedEpochs(): number[];
}

export interface OutboxStore {
  put(epoch: number, frame: Uint8Array): void | Promise<void>;
  deleteThrough(epoch: number): void | Promise<void>;
  scanAfter(cursor: number): Array<[number, Uint8Array]>;
  loadCursor(): number;
  saveCursor(epoch: number): void | Promise<void>;
}

export class Outbox<S extends OutboxStore = OutboxStore> implements DurableOutbox {
  constructor(store: S);
  readonly store: S;
  ackedThrough: number;
  append(epoch: number, msg: IpcMessage): void | Promise<void>;
  ackThrough(epoch: number): void | Promise<void>;
  replayFrom(cursor: number): Array<[number, IpcMessage]>;
  retainedEpochs(): number[];
}

export class InMemoryStore implements OutboxStore {
  put(epoch: number, frame: Uint8Array): void;
  deleteThrough(epoch: number): void;
  scanAfter(cursor: number): Array<[number, Uint8Array]>;
  loadCursor(): number;
  saveCursor(epoch: number): void;
}

export class InMemoryOutbox extends Outbox<InMemoryStore> {}

export class OrSet {
  add(tag: string): void;
  removeObserved(tags: Iterable<string>): void;
  present(): boolean;
  join(other: OrSet): void;
}

export function wireStampGreater(a: WireStamp, b: WireStamp): boolean;

export class WireLwwRegister<V = unknown> {
  constructor(stamp: WireStamp, value: V);
  stamp: WireStamp;
  value: V;
  set(stamp: WireStamp, value: V): void;
  join(other: WireLwwRegister<V>): void;
}

// Full-duplex reliable-sync loop driver (#sync-driver).

// Outbound transport seam: deliver one already-framed IpcMessage. Returning
// `false` (or throwing) means the frame was not durably handed to the peer;
// at-least-once is a driver property, not a sink property.
export interface IpcSink {
  send(msg: IpcMessage): boolean | void;
}

// Inbound transport seam: poll for the next frame without blocking. `null`/
// `undefined` means currently exhausted/closed; a throw is the reconnect signal
// (surfaces from tick() as a DriverError).
export interface IpcSource {
  recv(): IpcMessage | null | undefined;
}

// Monotonic clock seam (host owns cadence; the driver only timestamps stalls).
export interface Clock {
  nowMillis(): number;
}

// Sender-side answer to a peer's ResyncRequest: a covering Snapshot at
// epoch >= fromEpoch.
export interface SnapshotProvider {
  snapshot(fromEpoch: number): IpcMessage;
}

export class SystemClock implements Clock {
  nowMillis(): number;
}

export class Progress {
  sent: number;
  applied: IpcMessage[];
  resyncRequested: boolean;
  snapshotsServed: number;
  peerAckedThrough: number;
  retained: number;
}

export class DriverError extends Error {
  readonly name: "DriverError";
  readonly kind: "Source";
  readonly cause: unknown;
}

export interface SyncDriverOptions {
  sink: IpcSink;
  source: IpcSource;
  outbox?: DurableOutbox;
  clock?: Clock;
  provider: SnapshotProvider;
  lastEpoch?: number;
}

export class SyncDriver {
  constructor(options: SyncDriverOptions);
  sink: IpcSink;
  source: IpcSource;
  outbox: DurableOutbox;
  clock: Clock;
  provider: SnapshotProvider;
  coordinator: ResyncCoordinator;
  peerAckedThrough: number;
  enqueue(epoch: number, msg: IpcMessage): void;
  onReconnect(): void;
  lastEpoch(): number;
  isStalled(): boolean;
  stalledFor(now: number): number;
  /** One scheduler-driven step. Throws DriverError on an inbound source read failure. */
  tick(): Progress;
}

export const OpKind: {
  readonly Read: "read";
  readonly Write: "write";
  readonly TriggerEffect: "trigger_effect";
};

export class RemoteOp {
  constructor(kind: string, node: NodeId);
  readonly kind: string;
  readonly node: NodeId;
  static read(node: NodeId): RemoteOp;
  static write(node: NodeId): RemoteOp;
  static triggerEffect(node: NodeId): RemoteOp;
}

export class PermissionDenied extends Error {
  readonly peer: PeerId;
  readonly op: RemoteOp;
}

export class PeerPermissions {
  allow(peer: PeerId, op: RemoteOp): boolean;
  allowMany(peer: PeerId, kind: string, nodes: Iterable<NodeId>): void;
  revoke(peer: PeerId, op: RemoteOp): boolean;
  revokePeer(peer: PeerId): boolean;
  isAllowed(peer: PeerId, op: RemoteOp): boolean;
  canRead(peer: PeerId, node: NodeId): boolean;
  check(peer: PeerId, op: RemoteOp): void;
  filterReadable(peer: PeerId, nodes: Iterable<NodeId>): NodeId[];
  peerCount(): number;
}

// Capability negotiation (protocol.md § Capability Negotiation).
export const PROTOCOL_ID: "lazily-ipc";
export const PROTOCOL_MAJOR_VERSION: 1;

export const Codec: {
  readonly Json: "json";
  readonly Bincode: "bincode";
  readonly Postcard: "postcard";
};

export const LazilyFfiMessageKind: {
  readonly Unknown: 0;
  readonly Snapshot: 1;
  readonly Delta: 2;
  readonly CrdtSync: 3;
  readonly ResyncRequest: 4;
  readonly OutboxAck: 5;
};

export const LazilyFfiStatus: {
  readonly Ok: 0;
  readonly Empty: 1;
  readonly NullPointer: 2;
  readonly InvalidMessage: 3;
  readonly EncodeFailed: 4;
  readonly Panic: 5;
};

export type HandshakeWire = {
  protocol_id: string;
  protocol_major_version: number;
  codec: string;
  max_frame_size: number;
  fragmentation_supported: boolean;
  ordered_reliable: boolean;
  peer_id: NodeId;
  session_id: string;
  features?: string[];
};

export type CompatibilityResult =
  | { ok: true }
  | { ok: false; field: string; reason: string };

export class SessionHandshake {
  constructor(fields: HandshakeWire);
  readonly protocolId: string;
  readonly protocolMajorVersion: number;
  readonly codec: string;
  readonly maxFrameSize: number;
  readonly fragmentationSupported: boolean;
  readonly orderedReliable: boolean;
  readonly peerId: NodeId;
  readonly sessionId: string;
  readonly features: readonly string[];
  toWire(): HandshakeWire;
  encodeJson(): Uint8Array;
  checkCompatible(other: SessionHandshake, requiredFeatures?: string[]): CompatibilityResult;
  static fromWire(value: unknown): SessionHandshake;
  static decodeJson(data: Uint8Array | string): SessionHandshake;
}

export const FfiCapability: {
  readonly Host: "host";
  readonly None: "none";
};

export type BindingCapabilities = {
  readonly binding: string;
  readonly ffi: "host" | "none";
  readonly reactive_core: boolean;
  readonly async_context: boolean;
  readonly ipc: boolean;
  readonly crdt: boolean;
  readonly collections: { cellmap: boolean; celltree: boolean; reconcile: boolean };
  readonly sem_tree: boolean;
  readonly seq_crdt: boolean;
  readonly text_crdt: boolean;
  readonly stable_id: boolean;
  readonly state_machine: boolean;
  readonly state_charts: boolean;
  readonly permissions: boolean;
  readonly capability_negotiation: boolean;
  readonly causal_receipts: boolean;
  readonly signaling: boolean;
  readonly webrtc: boolean;
};

export const BINDING_CAPABILITIES: BindingCapabilities;

// ===========================================================================
// Command / RPC message plane (command-plane-v1)
// ===========================================================================

export const DedupePolicy: {
  readonly None: "none";
  readonly SameIdempotencyKey: "same_idempotency_key";
  readonly SameCommandId: "same_command_id";
};

export type DedupePolicyValue = "none" | "same_idempotency_key" | "same_command_id";

export type CommandPolicyWire = {
  dedupe: DedupePolicyValue;
  supersede: boolean;
  cancel_on_preempt: boolean;
};

export class CommandPolicy {
  constructor(fields: { dedupe: DedupePolicyValue; supersede: boolean; cancelOnPreempt: boolean });
  readonly dedupe: DedupePolicyValue;
  readonly supersede: boolean;
  readonly cancelOnPreempt: boolean;
  toWire(): CommandPolicyWire;
  static fromWire(value: unknown): CommandPolicy;
}

export type CommandSubmitWire = {
  command_id: string;
  causation_id: string;
  source: string;
  target: string;
  namespace: string;
  name: string;
  authority_generation: number;
  idempotency_key: string;
  deadline_ms: number;
  policy: CommandPolicyWire;
  payload_type: string;
  payload_hash: string;
  payload: unknown;
  required_features: string[];
};

export class CommandSubmit {
  constructor(fields: {
    commandId: string;
    causationId: string;
    source: string;
    target: string;
    namespace: string;
    name: string;
    authorityGeneration: number;
    idempotencyKey: string;
    deadlineMs: number;
    policy: CommandPolicy | CommandPolicyWire;
    payloadType: string;
    payloadHash: string;
    payload: IpcValueValue | ShmBlobRef | WireBytes;
    requiredFeatures?: ReadonlyArray<string>;
  });
  readonly commandId: string;
  readonly causationId: string;
  readonly source: string;
  readonly target: string;
  readonly namespace: string;
  readonly name: string;
  readonly authorityGeneration: number;
  readonly idempotencyKey: string;
  readonly deadlineMs: number;
  readonly policy: CommandPolicy;
  readonly payloadType: string;
  readonly payloadHash: string;
  readonly payload: IpcValueValue;
  readonly requiredFeatures: readonly string[];
  toWire(): CommandSubmitWire;
  static fromWire(value: unknown): CommandSubmit;
}

export type CommandCancelWire = {
  command_id: string;
  causation_id: string;
  source: string;
  authority_generation: number;
  reason: string | null;
};

export class CommandCancel {
  constructor(fields: {
    commandId: string;
    causationId: string;
    source: string;
    authorityGeneration: number;
    reason?: string | null;
  });
  readonly commandId: string;
  readonly causationId: string;
  readonly source: string;
  readonly authorityGeneration: number;
  readonly reason: string | null;
  toWire(): CommandCancelWire;
  static fromWire(value: unknown): CommandCancel;
}

export const CommandEventKind: {
  readonly Observed: "observed";
  readonly Accepted: "accepted";
  readonly Started: "started";
  readonly Progress: "progress";
  readonly Cancelled: "cancelled";
  readonly Superseded: "superseded";
  readonly TimedOut: "timed_out";
};

export type CommandEventKindValue =
  | "observed"
  | "accepted"
  | "started"
  | "progress"
  | "cancelled"
  | "superseded"
  | "timed_out";

export type CommandEventWire = {
  event_id: string;
  command_id: string;
  kind: CommandEventKindValue;
  generation: number;
  detail: string | null;
};

export class CommandEvent {
  constructor(fields: {
    eventId: string;
    commandId: string;
    kind: CommandEventKindValue;
    generation: number;
    detail?: string | null;
  });
  readonly eventId: string;
  readonly commandId: string;
  readonly kind: CommandEventKindValue;
  readonly generation: number;
  readonly detail: string | null;
  toWire(): CommandEventWire;
  static fromWire(value: unknown): CommandEvent;
}

export class CommandEvents {
  constructor(events?: ReadonlyArray<CommandEvent | CommandEventWire>);
  readonly events: readonly CommandEvent[];
  toWire(): { events: CommandEventWire[] };
  static fromWire(value: unknown): CommandEvents;
}

export const CommandStatus: {
  readonly Submitted: "submitted";
  readonly Accepted: "accepted";
  readonly Running: "running";
  readonly Applied: "applied";
  readonly Rejected: "rejected";
  readonly Cancelled: "cancelled";
  readonly Superseded: "superseded";
  readonly TimedOut: "timed_out";
};

export type CommandStatusValue =
  | "submitted"
  | "accepted"
  | "running"
  | "applied"
  | "rejected"
  | "cancelled"
  | "superseded"
  | "timed_out";

export function isTerminalCommandStatus(status: CommandStatusValue): boolean;

export type CommandProjectionEntryWire = {
  command_id: string;
  status: CommandStatusValue;
  terminal: boolean;
  generation: number;
  reason: string | null;
  terminal_receipt_id: string | null;
  last_event_id: string | null;
};

export class CommandProjectionEntry {
  constructor(fields: {
    commandId: string;
    status: CommandStatusValue;
    terminal: boolean;
    generation: number;
    reason?: string | null;
    terminalReceiptId?: string | null;
    lastEventId?: string | null;
  });
  readonly commandId: string;
  readonly status: CommandStatusValue;
  readonly terminal: boolean;
  readonly generation: number;
  readonly reason: string | null;
  readonly terminalReceiptId: string | null;
  readonly lastEventId: string | null;
  with(patch: Partial<{
    status: CommandStatusValue;
    terminal: boolean;
    reason: string | null;
    terminalReceiptId: string | null;
    lastEventId: string | null;
  }>): CommandProjectionEntry;
  toWire(): CommandProjectionEntryWire;
  static fromWire(value: unknown): CommandProjectionEntry;
}

export class CommandProjectionImage {
  constructor(generation: number, commands?: ReadonlyArray<CommandProjectionEntry | CommandProjectionEntryWire>);
  readonly generation: number;
  readonly commands: readonly CommandProjectionEntry[];
  toWire(): { generation: number; commands: CommandProjectionEntryWire[] };
  static fromWire(value: unknown): CommandProjectionImage;
}

export class CommandMessage {
  readonly kind: "CommandSubmit" | "CommandCancel" | "CommandEvents" | "CommandProjection";
  readonly submit?: CommandSubmit;
  readonly cancel?: CommandCancel;
  readonly events?: CommandEvents;
  readonly projection?: CommandProjectionImage;
  toWire(): unknown;
  encodeJson(): Uint8Array;
  static ofSubmit(submit: CommandSubmit): CommandMessage;
  static ofCancel(cancel: CommandCancel): CommandMessage;
  static ofEvents(events: CommandEvents): CommandMessage;
  static ofProjection(image: CommandProjectionImage): CommandMessage;
  static fromWire(value: unknown): CommandMessage;
  static decodeJson(data: Uint8Array | string): CommandMessage;
}

export const CommandApplyStatusKind: {
  readonly Recorded: "recorded";
  readonly Duplicate: "duplicate";
  readonly Unknown: "unknown";
  readonly StaleGeneration: "stale_generation";
  readonly TerminalConflict: "terminal_conflict";
};

export type CommandApplyStatus =
  | { kind: "recorded" }
  | { kind: "duplicate" }
  | { kind: "unknown" }
  | { kind: "stale_generation"; expected: number; actual: number }
  | { kind: "terminal_conflict"; commandId: string; existing: CommandStatusValue; incoming: CommandStatusValue };

export class CommandProjection {
  readonly generation: number;
  applyMessage(message: CommandMessage | unknown): CommandApplyStatus;
  submit(submit: CommandSubmit | CommandSubmitWire): CommandApplyStatus;
  event(event: CommandEvent | CommandEventWire): CommandApplyStatus;
  cancel(cancel: CommandCancel | CommandCancelWire): CommandApplyStatus;
  observeReceipt(receipt: CausalReceipt | CausalReceiptWire): CommandApplyStatus;
  applyProjection(image: CommandProjectionImage | unknown): CommandApplyStatus;
  entry(commandId: string): CommandProjectionEntry | null;
  terminalFor(commandId: string): CommandProjectionEntry | null;
  hasConflict(commandId: string): boolean;
  toImage(): CommandProjectionImage;
}

export const CallStateKind: {
  readonly Pending: "pending";
  readonly Resolved: "resolved";
  readonly Conflict: "conflict";
};

export type CallState =
  | { kind: "pending" }
  | { kind: "resolved"; entry: CommandProjectionEntry }
  | { kind: "conflict" };

export type CommandTransport = (message: CommandMessage) => void;

export function submitCommand(transport: CommandTransport, projection: CommandProjection, submit: CommandSubmit): string;
export function cancelCommand(transport: CommandTransport, projection: CommandProjection, cancel: CommandCancel): void;

export class CommandRpcClient {
  constructor(transport: CommandTransport);
  readonly projection: CommandProjection;
  submit(submit: CommandSubmit): string;
  cancel(cancel: CommandCancel): void;
  ingestCommand(message: CommandMessage): CommandApplyStatus;
  ingestReceipt(receipt: CausalReceipt): CommandApplyStatus;
  pollCall(commandId: string): CallState;
}
