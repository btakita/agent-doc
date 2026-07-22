import type { IpcMessage } from "./index.js";
import type { DecodeResult, LazilyFfiStatus, RecvBytesResult } from "./ffi.js";

export {
  FfiChannel,
  IpcMessage,
  LazilyFfiMessageKind,
  LazilyFfiStatus,
  decodeMessage,
  encodeMessage,
  kindOf,
  validateMessage,
} from "./ffi.js";
export type { EncodeResult, DecodeResult, RecvBytesResult } from "./ffi.js";

/** Whether koffi is installed so a native binding can be established. */
export function hasNativeBinding(): boolean;

/**
 * A native C-ABI channel backed by a `lazily` shared library. Interface-parallel
 * to the in-process `FfiChannel`.
 */
export class NativeFfiChannel {
  constructor(libPath?: string);
  sendJson(payload: Uint8Array | string): LazilyFfiStatus;
  send(message: IpcMessage): LazilyFfiStatus;
  recvJson(): RecvBytesResult;
  recv(): DecodeResult;
  len(): number;
  readonly isEmpty: boolean;
  free(): void;
}

/** Open a native C-ABI channel to a `lazily` shared library. */
export function loadNativeChannel(libPath?: string): NativeFfiChannel;
