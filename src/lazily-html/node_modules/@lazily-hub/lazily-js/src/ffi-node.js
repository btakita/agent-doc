// The C-ABI FFI boundary — Node build (`"node"` package export condition).
//
// Re-exports the isomorphic codec + in-process channel from `./ffi.js` verbatim,
// and ADDS a native binding that speaks the real C ABI (`lazily_ffi_channel_*`,
// schemas/ffi.json) to a `lazily` shared library via koffi. The native
// `NativeFfiChannel` presents the SAME interface as the in-process `FfiChannel`,
// so callers are transport-agnostic: the browser shim (`./ffi.js`) and this
// native binding are drop-in interchangeable.
//
// koffi and the native `.so` are optional: importing this module never fails.
// `loadNativeChannel(libPath)` throws a clear error if the binding cannot be
// established, and `hasNativeBinding()` reports availability.

import { createRequire } from "node:module";
import {
  FfiChannel,
  IpcMessage,
  LazilyFfiMessageKind,
  LazilyFfiStatus,
  decodeMessage,
  encodeMessage,
  kindOf,
  validateMessage,
} from "./ffi.js";

export {
  FfiChannel,
  IpcMessage,
  LazilyFfiMessageKind,
  LazilyFfiStatus,
  decodeMessage,
  encodeMessage,
  kindOf,
  validateMessage,
};

const require = createRequire(import.meta.url);

let koffiModule = null;
let koffiTried = false;

function loadKoffi() {
  if (!koffiTried) {
    koffiTried = true;
    try {
      koffiModule = require("koffi");
    } catch {
      koffiModule = null;
    }
  }
  return koffiModule;
}

/**
 * Whether koffi is installed so a native binding can be established. (Does not
 * imply a specific `.so` is loadable — that's checked at
 * {@link loadNativeChannel}.)
 * @returns {boolean}
 */
export function hasNativeBinding() {
  return loadKoffi() !== null;
}

// Bind the `lazily_ffi_*` C ABI once per library path.
const bindingCache = new Map();

function bindLibrary(libPath) {
  const cached = bindingCache.get(libPath);
  if (cached) {
    return cached;
  }
  const koffi = loadKoffi();
  if (!koffi) {
    throw new Error(
      "native lazily FFI requires the optional `koffi` dependency (npm i koffi); " +
        "use the isomorphic FfiChannel from `@lazily-hub/lazily-js/ffi` for a pure-JS channel",
    );
  }
  const lib = koffi.load(libPath);
  // LazilyFfiBytes { uint8_t* ptr; size_t len; } — Rust-owned output buffer.
  const LazilyFfiBytes = koffi.struct("LazilyFfiBytes", {
    ptr: "uint8_t*",
    len: "size_t",
  });
  const binding = {
    koffi,
    LazilyFfiBytes,
    channel_new: lib.func("void* lazily_ffi_channel_new()"),
    channel_free: lib.func("void lazily_ffi_channel_free(void*)"),
    channel_send_json: lib.func("int lazily_ffi_channel_send_json(void*, uint8_t*, size_t)"),
    channel_recv_json: lib.func("int lazily_ffi_channel_recv_json(void*, _Out_ LazilyFfiBytes*)"),
    channel_len: lib.func("int lazily_ffi_channel_len(void*, _Out_ size_t*)"),
    bytes_free: lib.func("void lazily_ffi_bytes_free(LazilyFfiBytes)"),
  };
  bindingCache.set(libPath, binding);
  return binding;
}

/**
 * A native C-ABI channel backed by a `lazily` shared library. Interface-parallel
 * to the in-process {@link FfiChannel}. Honors the `LazilyFfiBytes {ptr,len}`
 * ownership contract: every received buffer is copied out of Rust-owned memory
 * and freed exactly once via `lazily_ffi_bytes_free`.
 */
export class NativeFfiChannel {
  #binding;
  #ptr;

  /** @param {string} [libPath] shared-library name/path (default `"lazily"`). */
  constructor(libPath = "lazily") {
    this.#binding = bindLibrary(libPath);
    this.#ptr = this.#binding.channel_new();
    if (!this.#ptr) {
      throw new Error("lazily_ffi_channel_new returned null");
    }
  }

  /** `lazily_ffi_channel_send_json` over the real C ABI. */
  sendJson(payload) {
    if (!this.#ptr) {
      return LazilyFfiStatus.NullPointer;
    }
    const bytes = typeof payload === "string" ? new TextEncoder().encode(payload) : payload;
    return this.#binding.channel_send_json(this.#ptr, bytes, bytes.length);
  }

  send(message) {
    const { status, payload } = encodeMessage(message);
    if (status !== LazilyFfiStatus.Ok) {
      return status;
    }
    return this.sendJson(payload);
  }

  /** `lazily_ffi_channel_recv_json`; copies + frees the Rust-owned buffer. */
  recvJson() {
    if (!this.#ptr) {
      return { status: LazilyFfiStatus.NullPointer, payload: new Uint8Array(0) };
    }
    const { koffi, LazilyFfiBytes } = this.#binding;
    const out = {};
    const status = this.#binding.channel_recv_json(this.#ptr, out);
    if (status !== LazilyFfiStatus.Ok) {
      return { status, payload: new Uint8Array(0) };
    }
    const view = out; // koffi decodes the _Out_ struct into `out`
    const len = Number(view.len);
    let payload = new Uint8Array(0);
    if (len > 0 && view.ptr) {
      payload = Uint8Array.from(koffi.decode(view.ptr, koffi.array("uint8_t", len)));
    }
    // Free exactly once — the C ABI transfers ownership of the buffer to us.
    this.#binding.bytes_free(view);
    return { status: LazilyFfiStatus.Ok, payload };
  }

  recv() {
    const { status, payload } = this.recvJson();
    if (status !== LazilyFfiStatus.Ok) {
      return { status, message: null };
    }
    return decodeMessage(payload);
  }

  /** `lazily_ffi_channel_len`. @returns {number} */
  len() {
    if (!this.#ptr) {
      return 0;
    }
    const out = {};
    const status = this.#binding.channel_len(this.#ptr, out);
    return status === LazilyFfiStatus.Ok ? Number(out.value ?? out) : 0;
  }

  get isEmpty() {
    return this.len() === 0;
  }

  /** `lazily_ffi_channel_free`; idempotent. */
  free() {
    if (this.#ptr) {
      this.#binding.channel_free(this.#ptr);
      this.#ptr = null;
    }
  }
}

/**
 * Open a native C-ABI channel to a `lazily` shared library. Throws if koffi or
 * the library is unavailable. For a pure-JS channel that works everywhere, use
 * {@link FfiChannel} from `@lazily-hub/lazily-js/ffi` instead.
 * @param {string} [libPath] shared-library name/path (default `"lazily"`).
 * @returns {NativeFfiChannel}
 */
export function loadNativeChannel(libPath = "lazily") {
  return new NativeFfiChannel(libPath);
}
