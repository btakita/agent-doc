import { EventEmitter } from "node:events";
import { createRequire } from "node:module";
import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { createHash } from "node:crypto";

/**
 * JS-friendly interface to the lazily / agent-doc state-projection FFI channel.
 *
 * The native loader (`loadAgentDocFFI`) returns an object implementing this
 * interface; tests inject a plain-object mock instead (no native dependency).
 *
 * Mirrors the Kotlin `LazilyFFI` + `StateProjectionClient` pair from lazily-kt
 * (#mes4), but adapts the pointer/free lifecycle into the loader boundary so
 * the consumer never handles raw C pointers.
 *
 * @typedef {object} LazilyFFI
 * @property {(documentHash: string) => (string | null)} stateProjection
 *   Read the binary's `DocumentStateProjection` JSON for a document, or `null`
 *   when no events have been recorded. The loader reads the NUL-terminated C
 *   string and frees the backing pointer before returning.
 * @property {(documentHash: string, factJson: string) => boolean} recordStateEvent
 *   Feed a `StateEvent` JSON object into the binary's state backbone. Returns
 *   `true` if accepted, `false` on parse/ledger failure.
 */

const nativeRequire = createRequire(import.meta.url);

/**
 * Compute the canonical document key used by agent-doc snapshots and editor
 * state-projection events.
 *
 * @param {string} filePath
 * @returns {string}
 */
export function documentHash(filePath) {
  let canonical;
  try {
    canonical = realpathSync(filePath);
  } catch {
    canonical = resolve(filePath);
  }
  return createHash("sha256").update(canonical, "utf-8").digest("hex");
}

/**
 * Build the Rust `StateEvent` JSON shape for a typed fact.
 *
 * @param {string} documentHashValue
 * @param {string} type
 * @param {Record<string, unknown>} fields
 * @param {string} eventSuffix
 * @returns {{ event_id: string, fact: Record<string, unknown> & { type: string, document_hash: string } }}
 */
export function buildStateEvent(documentHashValue, type, fields, eventSuffix) {
  return {
    event_id: `${documentHashValue}:${eventSuffix}`,
    fact: {
      type,
      document_hash: documentHashValue,
      ...fields,
    },
  };
}

/**
 * Reduce a `DocumentStateProjection` JSON object into the compact editor status
 * shape used by JetBrains and VS Code.
 *
 * @param {any} projection
 * @returns {{ routeReadiness?: string, routePaneId?: string, latestTransportPatchId?: string, latestTransportPhase?: string, proofMarkers: number } | null}
 */
export function projectionSummary(projection) {
  if (!projection || typeof projection !== "object") return null;
  const route = projection.route ?? {};
  const transport = projection.transport ?? {};
  const proof = projection.proof ?? {};
  const patches = transport.patches && typeof transport.patches === "object"
    ? Object.entries(transport.patches)
    : [];
  const sortedPatches = patches.sort(([a], [b]) => a.localeCompare(b));
  const latest = sortedPatches.length > 0 ? sortedPatches[sortedPatches.length - 1] : undefined;
  return {
    routeReadiness: typeof route.readiness === "string" ? route.readiness : undefined,
    routePaneId: typeof route.pane_id === "string" ? route.pane_id : undefined,
    latestTransportPatchId: latest?.[0],
    latestTransportPhase: typeof latest?.[1]?.phase === "string" ? latest[1].phase : undefined,
    proofMarkers: proof.markers && typeof proof.markers === "object"
      ? Object.keys(proof.markers).length
      : 0,
  };
}

/**
 * Render a compact editor-visible status string for a projection summary.
 *
 * @param {{ routeReadiness?: string, routePaneId?: string, latestTransportPatchId?: string, latestTransportPhase?: string, proofMarkers: number }} summary
 * @returns {string}
 */
export function compactProjectionSummary(summary) {
  return `route=${summary.routeReadiness ?? "unknown"} pane=${summary.routePaneId ?? "-"} `
    + `transport=${summary.latestTransportPatchId ?? "-"}:${summary.latestTransportPhase ?? "-"} `
    + `proof_markers=${summary.proofMarkers}`;
}

/**
 * Decode and free one `agent_doc_state_projection` pointer.
 *
 * @param {any} koffi
 * @param {any} ptr
 * @param {(ptr: any) => void} freeString
 * @returns {string | null}
 */
export function decodeStateProjectionPointer(koffi, ptr, freeString) {
  if (ptr === null || ptr === undefined) return null;
  const address = typeof koffi.address === "function" ? koffi.address(ptr) : 1n;
  if (address === 0n || address === 0) return null;
  try {
    const json = koffi.decode.string(ptr);
    return json === "null" || json == null ? null : json;
  } finally {
    freeString(ptr);
  }
}

/**
 * Wrap an already-loaded native library with the lazily state-projection FFI
 * interface. Exported so package and editor parity tests can cover pointer/free
 * lifecycle without loading a real native library.
 *
 * @param {any} koffi
 * @param {any} lib
 * @returns {LazilyFFI}
 */
export function wrapAgentDocStateProjectionFFI(koffi, lib) {
  const state_projection = lib.func(
    "void *agent_doc_state_projection(const char *document_hash)",
  );
  const record_state_event = lib.func(
    "int agent_doc_record_state_event(const char *document_hash, const char *fact_json)",
  );
  const free_string = lib.func("void agent_doc_free_string(void *ptr)");
  return {
    stateProjection(documentHashValue) {
      return decodeStateProjectionPointer(
        koffi,
        state_projection(documentHashValue),
        free_string,
      );
    },
    recordStateEvent(documentHashValue, factJson) {
      return record_state_event(documentHashValue, factJson) === 1;
    },
  };
}

/**
 * Lazily load the `agent_doc` native library and wrap its state-projection C
 * ABI into the {@link LazilyFFI} interface.
 *
 * Uses `createRequire` so `koffi` is resolved **only when this function is
 * called**, not when the module is imported — keeping `make check` (pure-JS
 * tests with injected mocks) free of any native build dependency. koffi ships
 * prebuilt binaries (no native compile step), so — unlike the previous
 * ffi-napi/ref-napi loader — it installs cleanly on Node >= 23 (ffi-napi's
 * `napi_add_finalizer` ABI break made it uninstallable there).
 *
 * @param {string} [libPath] Library name or path (defaults to `agent_doc`,
 *   resolved from the process load path / `LD_LIBRARY_PATH`).
 * @returns {LazilyFFI}
 */
export function loadAgentDocFFI(libPath) {
  const koffi = nativeRequire("koffi");
  const lib = koffi.load(libPath ?? "agent_doc");
  return wrapAgentDocStateProjectionFFI(koffi, lib);
}

/**
 * Holds the raw JSON projection returned by the binary's
 * `agent_doc_state_projection` FFI, exposed as an `EventEmitter` for reactive
 * UI binding (VSCode status items, webviews), plus {@link recordStateEvent}
 * for feeding facts into the binary's state backbone.
 *
 * `null` means no state events have been recorded for the document. The JSON
 * contains document/queue/closeout/transport/supervisor/route/proof slices —
 * consumers parse it with their preferred JSON library.
 *
 * This is the JS analogue of lazily-kt's `StateProjectionClient` (Kotlin
 * `StateFlow` → JS `EventEmitter`): plugins become thin projection-renderer +
 * event-reporter, NOT a reactive-core port (FFI-first rule).
 *
 * @example
 * import { StateProjectionClient, loadAgentDocFFI } from "@lazily-hub/js/state-projection";
 * const ffi = loadAgentDocFFI();
 * const client = new StateProjectionClient(documentHash, ffi);
 * client.on("projection", (json) => renderStatus(json));
 * client.refresh();                 // pull latest projection, emits "projection"
 * client.recordStateEvent(JSON.stringify({ type: "BaselineSaved" }));
 */
export class StateProjectionClient extends EventEmitter {
  #documentHash;
  #ffi;
  #projection = null;

  /**
   * @param {string} documentHash
   * @param {LazilyFFI} [ffi] Native FFI handle. When omitted the `agent_doc`
   *   library is loaded eagerly via {@link loadAgentDocFFI}. Tests pass a
   *   plain-object mock implementing {@link LazilyFFI}.
   */
  constructor(documentHash, ffi = loadAgentDocFFI()) {
    super();
    this.#documentHash = documentHash;
    this.#ffi = ffi;
  }

  /** The document hash this client is bound to. */
  get documentHash() {
    return this.#documentHash;
  }

  /** Current projection JSON, or `null` when no events have been recorded. */
  get projection() {
    return this.#projection;
  }

  /** `true` once a non-null projection has been observed. */
  get isAvailable() {
    return this.#projection !== null;
  }

  /**
   * Pull the latest state projection from the binary. Updates {@link projection}
   * and emits a `"projection"` event with the new JSON (or `null`). Safe to call
   * on any thread — the FFI is thread-safe.
   *
   * @returns {string | null} The freshly-read projection JSON, or `null`.
   */
  refresh() {
    const json = this.#ffi.stateProjection(this.#documentHash);
    this.#projection = json;
    this.emit("projection", json);
    return json;
  }

  /**
   * Record a state event in the binary's backbone.
   *
   * @param {string} factJson JSON object deserializable as a `StateEvent`
   *   (internally tagged `{ "event_id": "...", "fact": { "type": "..." } }`).
   * @returns {boolean} `true` if accepted, `false` on parse/ledger failure.
   */
  recordStateEvent(factJson) {
    return this.#ffi.recordStateEvent(this.#documentHash, factJson);
  }
}
