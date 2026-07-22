import {
  BlobBackendKindValue,
  Delta,
  CrdtSync,
  IpcMessage,
  IpcValue,
  ShmBlobRef,
  Snapshot,
} from "./index.js";

export { BlobBackendKind } from "./index.js";
export type { BlobBackendKindValue } from "./index.js";

/** A zero-copy view into a backend's resolved bytes, or `null` when a
 * descriptor did not resolve (unknown / stale / corrupt / wrong-backend). */
export type BlobView = Uint8Array | null;

/** Default byte threshold at/above which a payload is spilled to a backend. */
export const DEFAULT_SPILL_THRESHOLD: number;

/** 53-bit-reduced FNV-1a-64 of `bytes`, as a non-negative safe integer. */
export function blobChecksum(bytes: ArrayLike<number>): number;

/** In-process blob arena: hands out `ShmBlobRef` descriptors over an immutable,
 * stably-indexed entry table. Resolve zero-copy via `readView`. */
export class ShmBlobArena {
  constructor(options?: { epoch?: number });
  readonly epoch: number;
  readonly length: number;
  readonly isEmpty: boolean;
  write(bytes: ArrayLike<number>): ShmBlobRef;
  readView(ref: ShmBlobRef): BlobView;
  read(ref: ShmBlobRef): Uint8Array | null;
  retain(ref: ShmBlobRef): boolean;
  free(ref: ShmBlobRef): boolean;
  advanceEpoch(): void;
}

export function validateBlobRef(ref: ShmBlobRef, maxLen?: number): boolean;

/** Adapter seam: mint descriptors via `write`, resolve zero-copy via `readView`. */
export abstract class BlobBackend {
  readonly kind: BlobBackendKindValue;
  write(bytes: ArrayLike<number>): ShmBlobRef;
  readView(ref: ShmBlobRef): BlobView;
  advanceEpoch(): void;
}

/** In-process backend (single address space). Descriptors carry `in_process`. */
export class InProcessBackend extends BlobBackend {
  constructor(arena?: ShmBlobArena);
  readonly arena: ShmBlobArena;
  readonly epoch: number;
}

/** Apache Arrow backend: stores Arrow IPC stream bytes; descriptors carry `arrow`. */
export class ArrowBackend extends BlobBackend {
  constructor(arena?: ShmBlobArena);
  readonly arena: ShmBlobArena;
  readonly epoch: number;
}

/** Receiver-side multi-backend resolver, routing by the `backend` discriminator. */
export class BlobRouter {
  register(backend: BlobBackend): this;
  readView(ref: ShmBlobRef): BlobView;
  resolve(value: IpcValue): BlobView;
}

export function spillValue(
  value: IpcValue,
  backend: BlobBackend,
  threshold?: number,
): { value: IpcValue; spilled: number };

export function resolveValue(value: IpcValue, backend: BlobBackend): BlobView;

export function spillMessage(
  message: IpcMessage,
  backend: BlobBackend,
  threshold?: number,
): { message: IpcMessage; spilledBytes: number };

/** Thrown when the `shm` backend is requested in a runtime without FFI
 * (the browser, or a server runtime without FFI access). */
export class ShmUnavailableError extends Error {}

/** Whether this runtime can host the `shm` backend (Node/Bun/Deno). Cheap,
 * synchronous, imports no FFI — `false` in the browser. */
export function shmSupported(): boolean;

export interface ShmBackend extends BlobBackend {
  readonly name: string;
  readonly capacity: number;
  readonly epoch: number;
  close(): void;
}

export interface ShmBackendOptions {
  /** Region size in bytes (default 1 MiB). Attachers must pass the same value. */
  capacity?: number;
  /** `true` (default) creates + owns the region; `false` attaches to it. */
  create?: boolean;
  /** Initial validity epoch (creator only). */
  epoch?: number;
}

/** Create or attach a cross-process POSIX shared-memory `shm` backend.
 * Rejects with {@link ShmUnavailableError} in a runtime without FFI. */
export function createShmBackend(
  name: string,
  options?: ShmBackendOptions,
): Promise<ShmBackend>;
