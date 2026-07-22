import type { IpcMessage } from "./index.js";

export { IpcMessage } from "./index.js";

/** IPC message-kind discriminant (schemas/ffi.json § LazilyFfiMessageKind). */
export const LazilyFfiMessageKind: {
  readonly Unknown: 0;
  readonly Snapshot: 1;
  readonly Delta: 2;
  readonly CrdtSync: 3;
};
export type LazilyFfiMessageKind =
  (typeof LazilyFfiMessageKind)[keyof typeof LazilyFfiMessageKind];

/** FFI operation status code (schemas/ffi.json § LazilyFfiStatus). */
export const LazilyFfiStatus: {
  readonly Ok: 0;
  readonly Empty: 1;
  readonly NullPointer: 2;
  readonly InvalidMessage: 3;
  readonly EncodeFailed: 4;
  readonly Panic: 5;
};
export type LazilyFfiStatus = (typeof LazilyFfiStatus)[keyof typeof LazilyFfiStatus];

export interface EncodeResult {
  status: LazilyFfiStatus;
  kind: LazilyFfiMessageKind;
  payload: Uint8Array;
}

export interface DecodeResult {
  status: LazilyFfiStatus;
  message: IpcMessage | null;
}

export interface RecvBytesResult {
  status: LazilyFfiStatus;
  payload: Uint8Array;
}

/** The IPC message-kind discriminant for `message`. */
export function kindOf(message: IpcMessage): LazilyFfiMessageKind;
/** Encode an IPC message to canonical JSON wire bytes (never throws). */
export function encodeMessage(message: IpcMessage): EncodeResult;
/** Decode canonical JSON wire bytes to an IPC message (never throws). */
export function decodeMessage(payload: Uint8Array | string): DecodeResult;
/** Validate wire bytes without materializing the message. */
export function validateMessage(payload: Uint8Array | string): LazilyFfiStatus;
/** Whether a native C-ABI binding is available (always false in the isomorphic core). */
export function hasNativeBinding(): boolean;

/**
 * The isomorphic in-process `LazilyFfiChannel`: a validated JSON-frame FIFO.
 * The browser shim for the native channel — identical semantics, no `.so`.
 */
export class FfiChannel {
  static create(): FfiChannel;
  sendJson(payload: Uint8Array | string): LazilyFfiStatus;
  send(message: IpcMessage): LazilyFfiStatus;
  recvJson(): RecvBytesResult;
  recv(): DecodeResult;
  len(): number;
  readonly isEmpty: boolean;
  free(): void;
}
