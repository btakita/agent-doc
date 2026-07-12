// agent-doc's document-lifecycle inspector projection, derived from a generic lazily
// GraphView (`#lzsync` 3B clean split).
//
// Symmetric with the JetBrains AgentDocProjection.kt: the agent-doc-owned half that used
// to be welded into lazily's StateGraphMirror. The generic materialized view lives in
// lazily (GraphView); this DOMAIN projection lives in agent-doc. agent-doc is a peer
// product surface over lazily's view, sibling to signal-space's patchboard surface —
// it reads its own `agent_doc.*` node vocabulary, nothing generic.
//
// esbuild inlines the lazily-js import at build time (same 4-up path the state-graph-mirror
// and reliable-sync-liveness imports use).
import { GraphView } from "../../../../lazily-js/src/graph-view.js";

export interface AgentDocProjection {
    routeReadiness: string | null;
    routePaneId: string | null;
    latestTransportPhase: string | null;
    proofMarkers: number;
}

const ROUTE = "agent_doc.route";
const TRANSPORT_PATCH = "agent_doc.transport.patch";
const PROOF_MARKER = "agent_doc.proof.marker";

/** Raw component payload bytes (native `Inline` JSON) → a parsed object, or null. */
function payloadJson(bytes: number[] | null | undefined): Record<string, unknown> | null {
    if (!bytes || bytes.length === 0) return null;
    try {
        const parsed = JSON.parse(new TextDecoder().decode(Uint8Array.from(bytes)));
        return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : null;
    } catch {
        return null;
    }
}

const stringField = (obj: Record<string, unknown> | null, key: string): string | null =>
    obj && typeof obj[key] === "string" ? (obj[key] as string) : null;

/** Derive the agent-doc inspector projection from a folded GraphView. */
export function agentDocProjectionFromView(view: InstanceType<typeof GraphView>): AgentDocProjection {
    const route = payloadJson(view.singletonNode(ROUTE)?.payload);
    const patches = view.nodesOfType(TRANSPORT_PATCH);
    const latestPatch = patches.reduce(
        (best: { id: number; payload: number[] | null } | null, n: { id: number; payload: number[] | null }) =>
            best === null || n.id > best.id ? n : best,
        null,
    );
    return {
        routeReadiness: stringField(route, "readiness"),
        routePaneId: stringField(route, "pane_id"),
        latestTransportPhase: stringField(payloadJson(latestPatch?.payload), "phase"),
        proofMarkers: view.nodesOfType(PROOF_MARKER).length,
    };
}
