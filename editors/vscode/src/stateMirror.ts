/**
 * agent-doc VS Code state mirror — thin consumer of the lazily-js library
 * `StateGraphMirror` (`#r5at` / `#lazilystatesync4` / `#s5`).
 *
 * S5: the reactive mirror graph now lives IN the lazily-js library
 * (`src/lazily-js/src/state-graph-mirror.js`) as a real reactive graph on
 * lazily-js primitives (a `Context` holding per-`slot_id` reactive cells + a
 * memoized projection summary). This extension module is now a thin consumer:
 * the `StateGraphMirror` class here DELEGATES every apply/read to a lazily-js
 * instance, preserving the exact public surface the extension's other
 * consumers (`native.ts`, `extension.ts`) depend on.
 *
 * lazily-js is ESM + unpublished. The production `out/extension.js` is bundled
 * by **esbuild** (`esbuild.js`), which statically resolves the relative import
 * below from the monorepo tree and INLINES lazily-js's source into the single
 * CJS bundle — so the packaged `.vsix` is self-contained (no dangling
 * `../../../../lazily-js` runtime path, no ESM/CJS resolution at load time).
 * For the tsc test build (`out/*.test.js`) the same static import compiles to a
 * `require()` that Node's `require(ESM)` support resolves against the sibling
 * source. Either way the constructor is bound at module load; `initStateMirror`
 * stays for API/ordering compatibility (it resolves synchronously).
 *
 * `MirrorTurnProjection` (idle/awaiting_response/persisting) has no lazily-js
 * equivalent; it is still computed here from the delegated mirror's
 * `closeout.cycle` payload.
 */

// esbuild inlines this at build time; tsc emits a `require()` resolved by
// Node's require(ESM). The 4-up path reaches `src/lazily-js` in the monorepo.
import { StateGraphMirror as LazilyStateGraphMirrorImpl } from '../../../../lazily-js/src/state-graph-mirror.js';

/** The agent-doc state node `type_tag`s (cross-language vocabulary). */
export const AgentDocNodeType = {
    ROUTE: 'agent_doc.route',
    QUEUE: 'agent_doc.queue',
    QUEUE_HEAD: 'agent_doc.queue.head',
    CLOSEOUT_CYCLE: 'agent_doc.closeout.cycle',
    TRANSPORT_PATCH: 'agent_doc.transport.patch',
    SUPERVISOR_OWNER: 'agent_doc.supervisor.owner',
    DOCUMENT_BASELINE: 'agent_doc.document.baseline',
    DOCUMENT_AUTHORITY: 'agent_doc.document.authority',
    PROOF_MARKER: 'agent_doc.proof.marker',
} as const;

/**
 * One tracked cell in the plugin mirror graph.
 * `payload` is `base64(serde_json(struct))`, exactly as the FFI emits it.
 */
export interface MirrorNode {
    slotId: number;
    typeTag: string;
    payload: string | null;
}

/**
 * Reactive projection summary derived from a {@link StateGraphMirror}'s tracked
 * cells instead of re-parsing the full projection JSON (`#lazilystatesync4`).
 */
export interface MirrorProjectionSummary {
    routeReadiness?: string;
    routePaneId?: string;
    latestTransportPatchId?: string;
    latestTransportPhase?: string;
    proofMarkers: number;
}

export interface MirrorTurnProjection {
    state: 'idle' | 'awaiting_response' | 'persisting';
    turn_in_flight: boolean;
    transition_authority: 'project_controller';
}

/** Render the compact editor-visible status string (matches the kt `.compact()`). */
export function compactMirrorSummary(summary: MirrorProjectionSummary): string {
    return `route=${summary.routeReadiness ?? 'unknown'} pane=${summary.routePaneId ?? '-'} `
        + `transport=${summary.latestTransportPatchId ?? '-'}:${summary.latestTransportPhase ?? '-'} `
        + `proof_markers=${summary.proofMarkers}`;
}

/**
 * Decode a `base64(serde_json(struct))` payload to a JSON object, or null on
 * failure / unset payload. Pure (no FFI) — exported for tests + consumers.
 */
export function decodePayload(payload: string | null | undefined): Record<string, any> | null {
    if (payload == null || payload === '') return null;
    try {
        const json = Buffer.from(payload, 'base64').toString('utf-8');
        const parsed = JSON.parse(json);
        return parsed && typeof parsed === 'object' ? parsed : null;
    } catch {
        return null;
    }
}

function stringField(obj: Record<string, any> | null | undefined, key: string): string | undefined {
    if (!obj) return undefined;
    const value = obj[key];
    return typeof value === 'string' ? value : undefined;
}

function turnProjectionFromPhase(phase: string | undefined): MirrorTurnProjection {
    if (phase === 'preflight_started') {
        return {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
        };
    }
    if (phase === 'response_captured' || phase === 'write_applied') {
        return {
            state: 'persisting',
            turn_in_flight: true,
            transition_authority: 'project_controller',
        };
    }
    return {
        state: 'idle',
        turn_in_flight: false,
        transition_authority: 'project_controller',
    };
}

/** The lazily-js `StateGraphMirror` surface this module delegates to. */
interface LazilyStateGraphMirror {
    readonly epoch: number;
    readonly documentHash: string | null;
    readonly isInitialized: boolean;
    readonly nodeCount: number;
    applySnapshot(snapshot: any): boolean;
    applyDelta(delta: any): boolean;
    applyMessage(raw: string): boolean;
    nodesOfType(typeTag: string): MirrorNode[];
    singletonNode(typeTag: string): MirrorNode | null;
    payloadObject(typeTag: string): Record<string, any> | null;
    summary(): MirrorProjectionSummary;
}

type LazilyMirrorCtor = new () => LazilyStateGraphMirror;

/**
 * The lazily-js `StateGraphMirror` constructor, bound at module load from the
 * static import above (inlined by esbuild in the bundle; `require`d by tsc in
 * tests).
 */
let LazilyMirror: LazilyMirrorCtor | null =
    (LazilyStateGraphMirrorImpl as unknown as LazilyMirrorCtor) ?? null;

/**
 * Ensure the lazily-js `StateGraphMirror` constructor is bound. With the static
 * import it is already bound at module load, so this resolves synchronously;
 * it is kept as an awaitable seam so `activate()`/test setup can order it before
 * the per-doc mirror registries construct mirrors, and so a future dynamic load
 * strategy stays source-compatible. Idempotent.
 */
export async function initStateMirror(): Promise<void> {
    if (!LazilyMirror) {
        LazilyMirror = LazilyStateGraphMirrorImpl as unknown as LazilyMirrorCtor;
    }
}

/**
 * The per-document mirror the VS Code extension holds. A thin delegating
 * wrapper over the lazily-js reactive `StateGraphMirror`: apply/read all go to
 * the library instance; only `turnProjection()` (no lazily-js equivalent) is
 * computed here from the delegated closeout-cycle payload.
 *
 * Requires {@link initStateMirror} to have resolved first — the ctor throws a
 * clear error otherwise (the FFI/registry construction sites run after
 * activation, when the constructor is already cached).
 */
export class StateGraphMirror {
    private inner: LazilyStateGraphMirror;

    constructor() {
        if (!LazilyMirror) {
            throw new Error(
                'StateGraphMirror used before initStateMirror() resolved; ' +
                'await initStateMirror() in activate()/test setup before constructing a mirror',
            );
        }
        this.inner = new LazilyMirror();
    }

    /** Monotonic frontier — the highest lazily-spec epoch applied so far. */
    get epoch(): number {
        return this.inner.epoch;
    }

    /** The document hash declared by the last applied snapshot/delta, or null. */
    get documentHash(): string | null {
        return this.inner.documentHash;
    }

    /** True until at least one snapshot/delta has been applied. */
    get isInitialized(): boolean {
        return this.inner.isInitialized;
    }

    get nodeCount(): number {
        return this.inner.nodeCount;
    }

    /** Apply a cold-read snapshot JSON, replacing the whole graph image. */
    applySnapshotJson(raw: string): boolean {
        let root: any;
        try {
            root = JSON.parse(raw);
        } catch {
            return false;
        }
        return this.inner.applySnapshot(root);
    }

    /** Apply a warm delta JSON. Ops applied verbatim in emission order. */
    applyDeltaJson(raw: string): boolean {
        let root: any;
        try {
            root = JSON.parse(raw);
        } catch {
            return false;
        }
        return this.inner.applyDelta(root);
    }

    /**
     * Apply a raw `agent_doc_state_subscribe` message, dispatching on the
     * lazily-spec `"type"` discriminator (`snapshot` or `delta`). Returns false
     * when the message cannot be parsed.
     */
    applyMessage(raw: string): boolean {
        return this.inner.applyMessage(raw);
    }

    /** All tracked nodes of [typeTag] (stable insertion order). */
    nodesOfType(typeTag: string): MirrorNode[] {
        return this.inner.nodesOfType(typeTag);
    }

    /** The single document-level node for [typeTag], or null. */
    singletonNode(typeTag: string): MirrorNode | null {
        return this.inner.singletonNode(typeTag);
    }

    /** Decode a node payload (`base64(serde_json(struct))`) as a JSON object, or null. */
    payloadObject(typeTag: string): Record<string, any> | null {
        return this.inner.payloadObject(typeTag);
    }

    /**
     * Reactive summary derived from the delegated mirror's tracked cells (the
     * lazily-js analog of the kt `MirrorProjectionSummary.fromMirror`).
     */
    summary(): MirrorProjectionSummary {
        return this.inner.summary();
    }

    /**
     * Turn projection (idle/awaiting_response/persisting) — no lazily-js
     * equivalent; computed here from the delegated closeout-cycle phase.
     */
    turnProjection(): MirrorTurnProjection {
        const closeout = this.inner.payloadObject(AgentDocNodeType.CLOSEOUT_CYCLE);
        return turnProjectionFromPhase(stringField(closeout, 'phase'));
    }
}
