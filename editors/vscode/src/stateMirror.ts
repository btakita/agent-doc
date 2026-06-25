/**
 * lazily-js reactive mirror graph for the agent-doc FFI state backbone
 * (`#r5at` / `#lazilystatesync4`).
 *
 * The VS Code counterpart of the JetBrains `StateGraphMirror.kt` /
 * `MirrorProjectionSummary` (`#n529` / `#n529b`, `#lazilystatesync3`). The binary
 * (lazily-rs) owns the authoritative reactive state graph; this mirror applies
 * the `agent_doc_state_subscribe` snapshot/delta messages so editor UI
 * (dispatch-ready / busy / queue indicators) derives from tracked cells instead
 * of re-rendering the full projection JSON on every observed event.
 *
 * #lzpkgwire: like the JB plugin, the VS Code extension keeps a plugin-local
 * canonical mirror. `@lazily/js` is the FFI *consumer* + IPC helper package, NOT
 * a reactive-core port (per its own package description), so it ships no mirror
 * graph to depend on. The extension is also compiled as CommonJS while
 * `@lazily/js` is ESM. The pure helpers (`documentHash`, `buildStateEvent`,
 * `projectionSummary`, `compactProjectionSummary`) stay pinned to `@lazily/js`
 * via `native.test.ts` so the duplicate adapter cannot silently drift; the
 * mirror's *apply* behavior is pinned 1:1 to the kt `StateGraphMirror` design.
 *
 * Wire shapes match `agent-doc-orchestration/src/state_wire.rs` and
 * `src/lazily-spec/schemas/{snapshot,delta}.json`:
 * - snapshot: { type:"snapshot", epoch, document_hash, nodes[], edges[], roots[] }
 * - delta:    { type:"delta", base_epoch, epoch, document_hash, ops[] }
 *
 * Because the projection is a pure fold of deduped events, delta application is
 * deterministic and idempotent — a no-op delta (re-emit) leaves the mirror
 * unchanged (`#qdedupsync` property).
 */

/** The eight agent-doc state node `type_tag`s (cross-language vocabulary). */
export const AgentDocNodeType = {
    ROUTE: 'agent_doc.route',
    QUEUE: 'agent_doc.queue',
    QUEUE_HEAD: 'agent_doc.queue.head',
    CLOSEOUT_CYCLE: 'agent_doc.closeout.cycle',
    TRANSPORT_PATCH: 'agent_doc.transport.patch',
    SUPERVISOR_OWNER: 'agent_doc.supervisor.owner',
    DOCUMENT_BASELINE: 'agent_doc.document.baseline',
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
 *
 * Reads `agent_doc.route`, `agent_doc.transport.patch`, and
 * `agent_doc.proof.marker` nodes. The wire node does not carry the transport
 * patch `entity_key` (patch_id); the binary's full-snapshot `transport` section
 * does, so `latestTransportPatchId` stays null on the warm path (the cold-read
 * `projectionSummary` supplies it). The phase — the signal that drives
 * busy/dispatch-ready — is always available.
 */
export interface MirrorProjectionSummary {
    routeReadiness?: string;
    routePaneId?: string;
    latestTransportPatchId?: string;
    latestTransportPhase?: string;
    proofMarkers: number;
}

/** Render the compact editor-visible status string (matches the kt `.compact()`). */
export function compactMirrorSummary(summary: MirrorProjectionSummary): string {
    return `route=${summary.routeReadiness ?? 'unknown'} pane=${summary.routePaneId ?? '-'} `
        + `transport=${summary.latestTransportPatchId ?? '-'}:${summary.latestTransportPhase ?? '-'} `
        + `proof_markers=${summary.proofMarkers}`;
}

/**
 * Decode a `base64(serde_json(struct))` payload to a JSON object, or null on
 * failure / unset payload. Pure (no FFI) — exported for tests.
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

function stringField(obj: Record<string, any> | null, key: string): string | undefined {
    if (!obj) return undefined;
    const value = obj[key];
    return typeof value === 'string' ? value : undefined;
}

/**
 * The pure, FFI-free mirror graph the VS Code extension holds per document.
 *
 * TS analog of the kt `StateGraphMirror`: applies `agent_doc_state_subscribe`
 * snapshot/delta messages verbatim in emission order. The binary owns the
 * authoritative graph; this mirror only applies deltas + derives the summary.
 */
export class StateGraphMirror {
    private nodes = new Map<number, MirrorNode>();
    private edges = new Set<string>();
    private declaredHash: string | null = null;

    /** Monotonic frontier — the highest lazily-spec epoch applied so far. */
    epoch = 0;

    /** The document hash declared by the last applied snapshot/delta, or null. */
    get documentHash(): string | null {
        return this.declaredHash;
    }

    /** True until at least one snapshot/delta has been applied. */
    get isInitialized(): boolean {
        return this.declaredHash !== null;
    }

    get nodeCount(): number {
        return this.nodes.size;
    }

    private static edgeKey(dependent: number, dependency: number): string {
        return `${dependent}:${dependency}`;
    }

    /** Apply a cold-read snapshot JSON, replacing the whole graph image. */
    applySnapshotJson(raw: string): boolean {
        let root: any;
        try {
            root = JSON.parse(raw);
        } catch {
            return false;
        }
        if (!root || typeof root !== 'object') return false;
        if (typeof root.document_hash === 'string') this.declaredHash = root.document_hash;
        this.nodes.clear();
        this.edges.clear();
        if (Array.isArray(root.nodes)) {
            for (const node of root.nodes) {
                if (!node || typeof node.slot_id !== 'number' || typeof node.type_tag !== 'string') continue;
                const payload = typeof node.payload === 'string' ? node.payload : null;
                this.nodes.set(node.slot_id, { slotId: node.slot_id, typeTag: node.type_tag, payload });
            }
        }
        if (Array.isArray(root.edges)) {
            for (const edge of root.edges) {
                if (!edge || typeof edge.dependent !== 'number' || typeof edge.dependency !== 'number') continue;
                this.edges.add(StateGraphMirror.edgeKey(edge.dependent, edge.dependency));
            }
        }
        if (typeof root.epoch === 'number') this.epoch = root.epoch;
        return true;
    }

    /**
     * Apply a warm delta JSON. Ops are applied verbatim in emission order; the
     * frontier advances to the delta's epoch. A no-op delta (empty ops) only
     * advances the epoch.
     */
    applyDeltaJson(raw: string): boolean {
        let root: any;
        try {
            root = JSON.parse(raw);
        } catch {
            return false;
        }
        if (!root || typeof root !== 'object') return false;
        if (typeof root.document_hash === 'string') this.declaredHash = root.document_hash;
        if (Array.isArray(root.ops)) {
            for (const op of root.ops) {
                if (!op || typeof op.op !== 'string') continue;
                switch (op.op) {
                    case 'node_add': {
                        if (typeof op.slot_id !== 'number' || typeof op.type_tag !== 'string') break;
                        const payload = typeof op.payload === 'string' ? op.payload : null;
                        this.nodes.set(op.slot_id, { slotId: op.slot_id, typeTag: op.type_tag, payload });
                        break;
                    }
                    case 'cell_set':
                    case 'slot_value': {
                        if (typeof op.slot_id !== 'number') break;
                        const existing = this.nodes.get(op.slot_id);
                        if (existing) {
                            existing.payload = typeof op.payload === 'string' ? op.payload : null;
                        }
                        break;
                    }
                    case 'invalidate':
                        // Derived recompute is plugin-side; the mirror holds the
                        // stale payload until a cell_set arrives.
                        break;
                    case 'node_remove':
                        if (typeof op.slot_id === 'number') this.nodes.delete(op.slot_id);
                        break;
                    case 'edge_add':
                        if (typeof op.dependent === 'number' && typeof op.dependency === 'number') {
                            this.edges.add(StateGraphMirror.edgeKey(op.dependent, op.dependency));
                        }
                        break;
                    case 'edge_remove':
                        if (typeof op.dependent === 'number' && typeof op.dependency === 'number') {
                            this.edges.delete(StateGraphMirror.edgeKey(op.dependent, op.dependency));
                        }
                        break;
                    default:
                        break;
                }
            }
        }
        if (typeof root.epoch === 'number') this.epoch = Math.max(this.epoch, root.epoch);
        return true;
    }

    /**
     * Apply a raw `agent_doc_state_subscribe` message, dispatching on the
     * lazily-spec `"type"` discriminator (`snapshot` or `delta`). Returns false
     * when the message cannot be parsed.
     */
    applyMessage(raw: string): boolean {
        let root: any;
        try {
            root = JSON.parse(raw);
        } catch {
            return false;
        }
        if (!root || typeof root !== 'object' || typeof root.type !== 'string') return false;
        if (root.type === 'snapshot') return this.applySnapshotJson(raw);
        if (root.type === 'delta') return this.applyDeltaJson(raw);
        return false;
    }

    /** All tracked nodes of [typeTag] (stable insertion order). */
    nodesOfType(typeTag: string): MirrorNode[] {
        const out: MirrorNode[] = [];
        for (const node of this.nodes.values()) {
            if (node.typeTag === typeTag) out.push(node);
        }
        return out;
    }

    /** The single document-level node for [typeTag], or null. */
    singletonNode(typeTag: string): MirrorNode | null {
        for (const node of this.nodes.values()) {
            if (node.typeTag === typeTag) return node;
        }
        return null;
    }

    /** Decode a node payload (`base64(serde_json(struct))`) as a JSON object, or null. */
    payloadObject(typeTag: string): Record<string, any> | null {
        const node = this.singletonNode(typeTag);
        return node ? decodePayload(node.payload) : null;
    }

    /**
     * Reactive summary derived from this mirror's tracked cells (the TS analog of
     * the kt `MirrorProjectionSummary.fromMirror`).
     */
    summary(): MirrorProjectionSummary {
        const route = this.payloadObject(AgentDocNodeType.ROUTE);
        const routeReadiness = stringField(route, 'readiness');
        const routePaneId = stringField(route, 'pane_id');

        const patches = this.nodesOfType(AgentDocNodeType.TRANSPORT_PATCH);
        const latest = patches.length > 0
            ? patches.reduce((a, b) => (b.slotId > a.slotId ? b : a))
            : undefined;
        const latestTransportPhase = latest
            ? stringField(decodePayload(latest.payload), 'phase')
            : undefined;

        const proofMarkers = this.nodesOfType(AgentDocNodeType.PROOF_MARKER).length;

        return {
            routeReadiness,
            routePaneId,
            latestTransportPatchId: undefined,
            latestTransportPhase,
            proofMarkers,
        };
    }
}
