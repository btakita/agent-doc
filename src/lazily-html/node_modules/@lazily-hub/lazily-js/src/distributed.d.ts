import type {
  CrdtOp,
  CrdtSync,
  IpcMessage,
  IpcValueInline,
  IpcValueSharedBlob,
  PeerPermissions,
  WireStamp,
} from "./index.js";
import type { SignalingClient } from "./signaling.js";

export type PeerId = number;
export type NodeId = number;
export type IpcValue = IpcValueInline | IpcValueSharedBlob;

// --- DataChannel transport ---

/**
 * Minimal ordered, reliable, bidirectional byte-frame channel. One frame is one
 * serialized IpcMessage; the methods are non-blocking.
 */
export interface DataChannel {
  sendFrame(frame: Uint8Array): void;
  tryRecvFrame(): Uint8Array | null;
  isOpen(): boolean;
  close(): void;
}

export const WebRtcTransportErrorKind: {
  readonly Closed: "Closed";
  readonly Encode: "Encode";
  readonly Decode: "Decode";
  readonly Channel: "Channel";
};

export class WebRtcTransportError extends Error {
  readonly name: "WebRtcTransportError";
  readonly kind: "Closed" | "Encode" | "Decode" | "Channel";
  readonly cause?: unknown;
  constructor(kind: "Closed" | "Encode" | "Decode" | "Channel", cause?: unknown);
  static closed(): WebRtcTransportError;
  static encode(cause: unknown): WebRtcTransportError;
  static decode(cause: unknown): WebRtcTransportError;
  static channel(cause: unknown): WebRtcTransportError;
}

export class InMemoryDataChannel implements DataChannel {
  static pair(): [InMemoryDataChannel, InMemoryDataChannel];
  sendFrame(frame: Uint8Array): void;
  tryRecvFrame(): Uint8Array | null;
  isOpen(): boolean;
  close(): void;
}

export class WebRtcSink {
  constructor(channel: DataChannel, permissions: PeerPermissions, peer: PeerId);
  readonly channel: DataChannel;
  send(message: IpcMessage): void;
}

export class WebRtcSource {
  constructor(channel: DataChannel);
  readonly channel: DataChannel;
  recv(): IpcMessage | null;
}

// --- CRDT anti-entropy runtime ---

export interface PlaneCell {
  merge(op: CrdtOp): boolean;
  readonly value: IpcValue | undefined;
}

export interface FrontierEntry {
  peer: PeerId;
  stamp: WireStamp;
}

export interface ConvergedEntry {
  node: NodeId;
  key?: string;
  state: unknown;
}

export class CrdtPlaneRuntime {
  constructor(peer: PeerId);
  readonly peer: PeerId;
  readonly size: number;
  isEmpty(): boolean;
  register(node: NodeId, key?: string | null, cell?: PlaneCell | null): this;
  value(node: NodeId): IpcValue | undefined;
  winningOp(node: NodeId): CrdtOp | undefined;
  nodes(): NodeId[];
  converged(): ConvergedEntry[];
  localUpdate(node: NodeId, nowMicros: number, state: IpcValue | Uint8Array): CrdtOp | null;
  ingest(sync: CrdtSync, nowMicros: number): number;
  frontierEntries(): FrontierEntry[];
  wireFrontier(): Array<[PeerId, unknown]>;
  membership(): PeerId[];
  membershipCount(): number;
  syncFrame(): CrdtSync;
  syncFrameSince(
    since: Iterable<FrontierEntry | [PeerId, WireStamp | unknown]>,
  ): CrdtSync;
  syncReply(request: CrdtSync): CrdtSync;
  // Family sync (#lzfamilysync)
  registerFamilyLww(namespace: string): this;
  membershipEpoch(): number;
  familyKeys(namespace: string): string[];
  familyValueLww(namespace: string, keySuffix: string): boolean | undefined;
  familySetLww(
    namespace: string,
    keySuffix: string,
    value: boolean,
    nowMicros: number,
  ): CrdtOp | null;
}

// --- Browser WebRTC platform adapter ---

export function isWebRtcAvailable(): boolean;

export class RtcPeerChannel implements DataChannel {
  constructor(dataChannel: RTCDataChannel);
  sendFrame(frame: Uint8Array): void;
  tryRecvFrame(): Uint8Array | null;
  isOpen(): boolean;
  close(): void;
}

export interface RtcPeerConnectorOptions {
  rtcConfig?: RTCConfiguration;
  remote?: PeerId | null;
}

export class RtcPeerConnector {
  constructor(signalingClient: SignalingClient, options?: RtcPeerConnectorOptions);
  readonly connection: RTCPeerConnection;
  set remote(peer: PeerId);
  createDataChannel(label?: string, options?: RTCDataChannelInit): RtcPeerChannel;
  onDataChannel(): Promise<RtcPeerChannel>;
  createOffer(to: PeerId): Promise<RTCSessionDescriptionInit>;
  acceptOffer(from: PeerId, sdp: string): Promise<RTCSessionDescriptionInit>;
  acceptAnswer(sdp: string): Promise<void>;
  addIceCandidate(candidate: string | RTCIceCandidateInit): Promise<void>;
  close(): void;
}
