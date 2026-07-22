// The C-ABI FFI boundary (lazily-spec/protocol.md § FFI Boundary,
// schemas/ffi.json) — isomorphic core.
//
// This module is the ISOMORPHIC half of the boundary: the normative message
// codec (`encodeMessage` / `decodeMessage` / `validateMessage` / `kindOf`) and an
// in-process `FfiChannel` that models the `lazily_ffi_channel_*` FIFO semantics
// in pure JavaScript. It runs unchanged in the browser (it IS the browser shim)
// and in Node.
//
// The Node build additionally exposes a native binding that speaks the real
// C ABI to a `lazily` shared library — see `./ffi-node.js`, selected by the
// `"node"` package export condition. Both sides share this codec verbatim, so the
// C-ABI byte contract (externally-tagged JSON envelope, `LazilyFfiBytes {ptr,len}`
// ownership) is honored identically whether frames cross a real `.so` boundary or
// an in-process queue.
//
// Rust reference: `lazily-rs/src/ffi.rs`. Python peer: `lazily-py` `lazily.ffi`.

import { IpcMessage, LazilyFfiMessageKind, LazilyFfiStatus } from "./index.js";

export { LazilyFfiMessageKind, LazilyFfiStatus, IpcMessage };

function isUnknownKind(message) {
  return !(
    message &&
    (message.isSnapshot ||
      message.isDelta ||
      message.isCrdtSync ||
      message.isControl)
  );
}

/**
 * The IPC message-kind discriminant for `message` (schemas/ffi.json §
 * LazilyFfiMessageKind). Mirrors `lazily_ffi_ipc_message_kind_json`.
 * @param {IpcMessage} message
 * @returns {number} a {@link LazilyFfiMessageKind}
 */
export function kindOf(message) {
  if (!message) {
    return LazilyFfiMessageKind.Unknown;
  }
  if (message.isSnapshot) {
    return LazilyFfiMessageKind.Snapshot;
  }
  if (message.isDelta) {
    return LazilyFfiMessageKind.Delta;
  }
  if (message.isCrdtSync) {
    return LazilyFfiMessageKind.CrdtSync;
  }
  if (message.kind === "ResyncRequest") {
    return LazilyFfiMessageKind.ResyncRequest;
  }
  if (message.kind === "OutboxAck") {
    return LazilyFfiMessageKind.OutboxAck;
  }
  return LazilyFfiMessageKind.Unknown;
}

/**
 * Encode an {@link IpcMessage} to its canonical JSON wire bytes. Never throws —
 * mirrors `lazily_ffi_channel_send_json`'s status discipline.
 * @param {IpcMessage} message
 * @returns {{ status: number, kind: number, payload: Uint8Array }}
 *   `status` is {@link LazilyFfiStatus.Ok} on success,
 *   {@link LazilyFfiStatus.InvalidMessage} for an unknown kind, or
 *   {@link LazilyFfiStatus.EncodeFailed} on a serialization error.
 */
export function encodeMessage(message) {
  const kind = kindOf(message);
  if (isUnknownKind(message)) {
    return { status: LazilyFfiStatus.InvalidMessage, kind, payload: new Uint8Array(0) };
  }
  try {
    return { status: LazilyFfiStatus.Ok, kind, payload: message.encodeJson() };
  } catch {
    return { status: LazilyFfiStatus.EncodeFailed, kind, payload: new Uint8Array(0) };
  }
}

/**
 * Decode canonical JSON wire bytes to an {@link IpcMessage}. Never throws —
 * mirrors `lazily_ffi_channel_recv_json` / `lazily_ffi_ipc_message_clone_json`.
 * @param {Uint8Array | string} payload
 * @returns {{ status: number, message: IpcMessage | null }}
 *   `status` is {@link LazilyFfiStatus.Ok} on success,
 *   {@link LazilyFfiStatus.Empty} for empty input, or
 *   {@link LazilyFfiStatus.InvalidMessage} on a parse failure.
 */
export function decodeMessage(payload) {
  const len = typeof payload === "string" ? payload.length : payload?.length ?? 0;
  if (!payload || len === 0) {
    return { status: LazilyFfiStatus.Empty, message: null };
  }
  try {
    return { status: LazilyFfiStatus.Ok, message: IpcMessage.decodeJson(payload) };
  } catch {
    return { status: LazilyFfiStatus.InvalidMessage, message: null };
  }
}

/**
 * Validate wire bytes without materializing the message. Mirrors
 * `lazily_ffi_ipc_message_validate_json`.
 * @param {Uint8Array | string} payload
 * @returns {number} a {@link LazilyFfiStatus}
 */
export function validateMessage(payload) {
  return decodeMessage(payload).status;
}

/**
 * The isomorphic in-process `LazilyFfiChannel` (schemas/ffi.json,
 * `lazily_ffi_channel_*`): a validated JSON-frame FIFO. This is the browser shim
 * for the native channel — identical semantics, no `.so`. Under Node the native
 * binding in `./ffi-node.js` presents the same interface over a real C-ABI
 * channel.
 */
export class FfiChannel {
  /** @type {Uint8Array[]} */
  #queue = [];
  #open = true;

  /** `lazily_ffi_channel_new`: a fresh empty channel. */
  static create() {
    return new FfiChannel();
  }

  /**
   * `lazily_ffi_channel_send_json`: validate then enqueue a JSON frame. Invalid
   * or empty frames are rejected with a non-Ok status and NOT enqueued.
   * @param {Uint8Array | string} payload
   * @returns {number} a {@link LazilyFfiStatus}
   */
  sendJson(payload) {
    if (!this.#open) {
      return LazilyFfiStatus.NullPointer;
    }
    const status = validateMessage(payload);
    if (status !== LazilyFfiStatus.Ok) {
      return status;
    }
    const bytes =
      typeof payload === "string"
        ? new TextEncoder().encode(payload)
        : payload.slice(); // own a copy — mirrors the C-ABI buffer-copy contract
    this.#queue.push(bytes);
    return LazilyFfiStatus.Ok;
  }

  /**
   * Enqueue an {@link IpcMessage}, encoding it first.
   * @param {IpcMessage} message
   * @returns {number} a {@link LazilyFfiStatus}
   */
  send(message) {
    const { status, payload } = encodeMessage(message);
    if (status !== LazilyFfiStatus.Ok) {
      return status;
    }
    return this.sendJson(payload);
  }

  /**
   * `lazily_ffi_channel_recv_json`: dequeue the next frame's bytes.
   * @returns {{ status: number, payload: Uint8Array }} `status` is
   *   {@link LazilyFfiStatus.Ok} with the frame, or {@link LazilyFfiStatus.Empty}.
   */
  recvJson() {
    if (!this.#open) {
      return { status: LazilyFfiStatus.NullPointer, payload: new Uint8Array(0) };
    }
    if (this.#queue.length === 0) {
      return { status: LazilyFfiStatus.Empty, payload: new Uint8Array(0) };
    }
    return { status: LazilyFfiStatus.Ok, payload: this.#queue.shift() };
  }

  /**
   * Dequeue and decode the next frame as an {@link IpcMessage}.
   * @returns {{ status: number, message: IpcMessage | null }}
   */
  recv() {
    const { status, payload } = this.recvJson();
    if (status !== LazilyFfiStatus.Ok) {
      return { status, message: null };
    }
    return decodeMessage(payload);
  }

  /** `lazily_ffi_channel_len`: queued frame count. @returns {number} */
  len() {
    return this.#queue.length;
  }

  /** Whether the channel holds no frames. @returns {boolean} */
  get isEmpty() {
    return this.#queue.length === 0;
  }

  /** `lazily_ffi_channel_free`: release the channel; further ops fail closed. */
  free() {
    this.#queue = [];
    this.#open = false;
  }
}

/**
 * Whether a real native C-ABI binding is available in this runtime. Always
 * `false` for the isomorphic core; the Node build overrides this in
 * `./ffi-node.js`.
 * @returns {boolean}
 */
export function hasNativeBinding() {
  return false;
}
