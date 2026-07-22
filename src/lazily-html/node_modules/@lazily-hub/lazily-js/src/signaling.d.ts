export type PeerId = number;

export type ErrorCode =
  | "bad_message"
  | "not_joined"
  | "already_joined"
  | "duplicate_peer"
  | "unknown_target"
  | "permission_denied";

export const SignalingErrorCode: {
  readonly BadMessage: "bad_message";
  readonly NotJoined: "not_joined";
  readonly AlreadyJoined: "already_joined";
  readonly DuplicatePeer: "duplicate_peer";
  readonly UnknownTarget: "unknown_target";
  readonly PermissionDenied: "permission_denied";
};

export class SignalingError extends Error {
  readonly name: "SignalingError";
  readonly kind: "protocol" | "closed";
  readonly cause?: unknown;
  constructor(kind: "protocol" | "closed", message: string, cause?: unknown);
  static protocol(message: string, cause?: unknown): SignalingError;
  static closed(message?: string): SignalingError;
}

// --- ClientMessage variants ---

export interface ClientJoinWire {
  type: "join";
  peer: PeerId;
  capabilities?: string[];
}
export interface ClientOfferWire {
  type: "offer";
  to: PeerId;
  sdp: string;
}
export interface ClientAnswerWire {
  type: "answer";
  to: PeerId;
  sdp: string;
}
export interface ClientIceWire {
  type: "ice";
  to: PeerId;
  candidate: string;
}
export interface ClientRelayWire {
  type: "relay";
  to: PeerId;
  payload: unknown;
}
export interface ClientLeaveWire {
  type: "leave";
}
export type ClientMessageWire =
  | ClientJoinWire
  | ClientOfferWire
  | ClientAnswerWire
  | ClientIceWire
  | ClientRelayWire
  | ClientLeaveWire;

export class ClientJoin {
  readonly type: "join";
  readonly peer: PeerId;
  readonly capabilities: readonly string[] | null;
  constructor(peer: PeerId, capabilities?: Iterable<string> | null);
  toWire(): ClientJoinWire;
}
export class ClientOffer {
  readonly type: "offer";
  readonly to: PeerId;
  readonly sdp: string;
  constructor(to: PeerId, sdp: string);
  toWire(): ClientOfferWire;
}
export class ClientAnswer {
  readonly type: "answer";
  readonly to: PeerId;
  readonly sdp: string;
  constructor(to: PeerId, sdp: string);
  toWire(): ClientAnswerWire;
}
export class ClientIce {
  readonly type: "ice";
  readonly to: PeerId;
  readonly candidate: string;
  constructor(to: PeerId, candidate: string);
  toWire(): ClientIceWire;
}
export class ClientRelay {
  readonly type: "relay";
  readonly to: PeerId;
  readonly payload: unknown;
  constructor(to: PeerId, payload: unknown);
  toWire(): ClientRelayWire;
}
export class ClientLeave {
  readonly type: "leave";
  constructor();
  toWire(): ClientLeaveWire;
}

export type ClientMessageInstance =
  | ClientJoin
  | ClientOffer
  | ClientAnswer
  | ClientIce
  | ClientRelay
  | ClientLeave;

export const ClientMessage: {
  join(peer: PeerId, capabilities?: Iterable<string> | null): ClientJoin;
  offer(to: PeerId, sdp: string): ClientOffer;
  answer(to: PeerId, sdp: string): ClientAnswer;
  ice(to: PeerId, candidate: string): ClientIce;
  relay(to: PeerId, payload: unknown): ClientRelay;
  leave(): ClientLeave;
  fromWire(value: unknown): ClientMessageInstance;
};

// --- ServerMessage variants ---

export interface ServerWelcomeWire {
  type: "welcome";
  peer: PeerId;
  peers: PeerId[];
}
export interface ServerPeerJoinedWire {
  type: "peer-joined";
  peer: PeerId;
}
export interface ServerPeerLeftWire {
  type: "peer-left";
  peer: PeerId;
}
export interface ServerOfferWire {
  type: "offer";
  from: PeerId;
  sdp: string;
}
export interface ServerAnswerWire {
  type: "answer";
  from: PeerId;
  sdp: string;
}
export interface ServerIceWire {
  type: "ice";
  from: PeerId;
  candidate: string;
}
export interface ServerRelayWire {
  type: "relay";
  from: PeerId;
  payload: unknown;
}
export interface ServerErrorWire {
  type: "error";
  code: ErrorCode;
  message: string;
}
export type ServerMessageWire =
  | ServerWelcomeWire
  | ServerPeerJoinedWire
  | ServerPeerLeftWire
  | ServerOfferWire
  | ServerAnswerWire
  | ServerIceWire
  | ServerRelayWire
  | ServerErrorWire;

export class ServerWelcome {
  readonly type: "welcome";
  readonly peer: PeerId;
  readonly peers: readonly PeerId[];
  constructor(peer: PeerId, peers?: Iterable<PeerId>);
  toWire(): ServerWelcomeWire;
}
export class ServerPeerJoined {
  readonly type: "peer-joined";
  readonly peer: PeerId;
  constructor(peer: PeerId);
  toWire(): ServerPeerJoinedWire;
}
export class ServerPeerLeft {
  readonly type: "peer-left";
  readonly peer: PeerId;
  constructor(peer: PeerId);
  toWire(): ServerPeerLeftWire;
}
export class ServerOffer {
  readonly type: "offer";
  readonly from: PeerId;
  readonly sdp: string;
  constructor(from: PeerId, sdp: string);
  toWire(): ServerOfferWire;
}
export class ServerAnswer {
  readonly type: "answer";
  readonly from: PeerId;
  readonly sdp: string;
  constructor(from: PeerId, sdp: string);
  toWire(): ServerAnswerWire;
}
export class ServerIce {
  readonly type: "ice";
  readonly from: PeerId;
  readonly candidate: string;
  constructor(from: PeerId, candidate: string);
  toWire(): ServerIceWire;
}
export class ServerRelay {
  readonly type: "relay";
  readonly from: PeerId;
  readonly payload: unknown;
  constructor(from: PeerId, payload: unknown);
  toWire(): ServerRelayWire;
}
export class ServerErrorMessage {
  readonly type: "error";
  readonly code: ErrorCode;
  readonly message: string;
  constructor(code: ErrorCode, message: string);
  toWire(): ServerErrorWire;
}

export type ServerMessageInstance =
  | ServerWelcome
  | ServerPeerJoined
  | ServerPeerLeft
  | ServerOffer
  | ServerAnswer
  | ServerIce
  | ServerRelay
  | ServerErrorMessage;

export const ServerMessage: {
  welcome(peer: PeerId, peers?: Iterable<PeerId>): ServerWelcome;
  peerJoined(peer: PeerId): ServerPeerJoined;
  peerLeft(peer: PeerId): ServerPeerLeft;
  offer(from: PeerId, sdp: string): ServerOffer;
  answer(from: PeerId, sdp: string): ServerAnswer;
  ice(from: PeerId, candidate: string): ServerIce;
  relay(from: PeerId, payload: unknown): ServerRelay;
  error(code: ErrorCode, message: string): ServerErrorMessage;
  fromWire(value: unknown): ServerMessageInstance;
};

// --- Permissions ---

export type SignalingMode = "open" | "allowlist";

export type SignalOp = { kind: "join" } | { kind: "offer" | "answer" | "ice" | "relay"; to: PeerId };

export class SignalingPermissions {
  constructor(mode?: SignalingMode);
  readonly mode: SignalingMode;
  allowJoin(peer: PeerId): this;
  allowSignal(peer: PeerId, target: PeerId): this;
  allowMany(peer: PeerId, targets: Iterable<PeerId>): this;
  revokePeer(peer: PeerId): boolean;
  isAllowed(peer: PeerId, op: SignalOp): boolean;
}

// --- Room ---

export interface RoutedFrame {
  to: unknown;
  message: ServerMessageInstance;
}

export class SignalingRoom {
  constructor(options?: { mode?: SignalingMode; permissions?: SignalingPermissions });
  roster(): PeerId[];
  size(): number;
  receive(connId: unknown, message: ClientMessageInstance): RoutedFrame[];
  disconnect(connId: unknown): RoutedFrame[];
}

// --- Socket seam + client ---

export interface SignalingSocket {
  send(text: string): void | Promise<void>;
  recv(): Promise<string | null>;
  close(): void;
}

export class MemorySignalingSocket implements SignalingSocket {
  static pair(): [MemorySignalingSocket, MemorySignalingSocket];
  send(text: string): void;
  recv(): Promise<string | null>;
  close(): void;
}

export interface SignalingClientOptions {
  capabilities?: Iterable<string> | null;
}

export class SignalingClient {
  constructor(socket: SignalingSocket, peer: PeerId);
  static connect(
    socket: SignalingSocket,
    peer: PeerId,
    options?: SignalingClientOptions,
  ): SignalingClient;
  readonly peer: PeerId;
  send(message: ClientMessageInstance): void | Promise<void>;
  offer(to: PeerId, sdp: string): void | Promise<void>;
  answer(to: PeerId, sdp: string): void | Promise<void>;
  ice(to: PeerId, candidate: string): void | Promise<void>;
  relay(to: PeerId, payload: unknown): void | Promise<void>;
  leave(): void | Promise<void>;
  close(): void;
  recv(): Promise<ServerMessageInstance | null>;
  onMessage(callback: (message: ServerMessageInstance) => void): () => void;
}
