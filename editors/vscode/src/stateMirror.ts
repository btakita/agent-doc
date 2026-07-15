/**
 * agent-doc VS Code state view — thin consumer of the lazily-js generic
 * `GraphView` (`#lzsync` 3B clean split).
 *
 * The clean split retires the agent-doc-bespoke, base64-`WireDelta`
 * `StateGraphMirror`. The reactive read path now folds the canonical lazily wire
 * (native `IpcMessage` Snapshot/Delta, `NodeId`/`IpcValue`) through a generic
 * lazily {@link GraphView} and layers agent-doc's own `AgentDocProjection`
 * (route/transport/proof) and {@link agentDocTurnProjectionFromView}
 * (idle/awaiting_response/persisting) domain reads on top — agent-doc is a peer
 * product surface over lazily's view, sibling to signal-space's patchboard
 * surface, reading only its own `agent_doc.*` node vocabulary.
 *
 * lazily-js is ESM + unpublished. The production `out/extension.js` is bundled by
 * **esbuild** (`esbuild.js`), which statically resolves the relative imports below
 * from the monorepo tree and INLINES lazily-js's source into the single CJS
 * bundle — so the packaged `.vsix` is self-contained. For the tsc test build
 * (`out/*.test.js`) the same static imports compile to `require()`s that Node's
 * `require(ESM)` support resolves against the sibling source.
 */

// esbuild inlines these at build time; tsc emits `require()`s resolved by Node's
// require(ESM). The 4-up path reaches `src/lazily-js` in the monorepo.
import { GraphView } from '../../../../lazily-js/src/graph-view.js';
import { IpcMessage } from '../../../../lazily-js/src/index.js';

export { GraphView } from '../../../../lazily-js/src/graph-view.js';
export { agentDocProjectionFromView } from './agentDocProjection';
export type { AgentDocProjection } from './agentDocProjection';

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

/** agent-doc's turn-state projection, derived from the closeout-cycle phase. */
export interface AgentDocTurnProjection {
    state: 'idle' | 'awaiting_response' | 'persisting';
    turn_in_flight: boolean;
    transition_authority: 'project_controller';
    realtime_steering?: {
        state: 'prompt_target' | 'content_edit' | 'prompt_deleted' | 'prompt_reduced';
        count?: number;
        preview?: string;
        verbatim?: string;
    };
}

/**
 * Fold one native `agent_doc_state_subscribe` message (externally-tagged
 * `IpcMessage` JSON) into [view], dispatching on the variant (`Snapshot`/`Delta`).
 * Control frames and malformed JSON are ignored. Returns the applied kind
 * (`'snapshot'`/`'delta'`) or null.
 */
export function applyIpcMessageToView(view: InstanceType<typeof GraphView>, raw: string): 'snapshot' | 'delta' | null {
    let message: IpcMessage;
    try {
        message = IpcMessage.fromWire(JSON.parse(raw));
    } catch {
        return null;
    }
    if (message.isSnapshot) {
        view.applySnapshot(message.snapshot);
        return 'snapshot';
    }
    if (message.isDelta) {
        view.applyDelta(message.delta);
        return 'delta';
    }
    return null;
}

/** Raw component payload bytes (native `Inline` JSON) → a parsed object, or null. */
function payloadJson(bytes: number[] | null | undefined): Record<string, unknown> | null {
    if (!bytes || bytes.length === 0) return null;
    try {
        const parsed = JSON.parse(new TextDecoder().decode(Uint8Array.from(bytes)));
        return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : null;
    } catch {
        return null;
    }
}

function stringField(obj: Record<string, unknown> | null, key: string): string | undefined {
    if (!obj) return undefined;
    const value = obj[key];
    return typeof value === 'string' ? value : undefined;
}

function realtimeSteeringField(
    obj: Record<string, unknown> | null,
): AgentDocTurnProjection['realtime_steering'] | undefined {
    const value = obj?.realtime_steering;
    if (!value || typeof value !== 'object') return undefined;
    const steering = value as Record<string, unknown>;
    const state = stringField(steering, 'state');
    if (!['prompt_target', 'content_edit', 'prompt_deleted', 'prompt_reduced'].includes(state ?? '')) {
        return undefined;
    }
    return {
        state: state as NonNullable<AgentDocTurnProjection['realtime_steering']>['state'],
        count: typeof steering.count === 'number' ? steering.count : undefined,
        preview: stringField(steering, 'preview'),
        verbatim: stringField(steering, 'verbatim'),
    };
}

function turnProjectionFromPhase(
    phase: string | undefined,
    realtimeSteering?: AgentDocTurnProjection['realtime_steering'],
): AgentDocTurnProjection {
    if (phase === 'preflight_started') {
        return {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
            ...(realtimeSteering ? { realtime_steering: realtimeSteering } : {}),
        };
    }
    if (phase === 'response_captured' || phase === 'write_applied') {
        return {
            state: 'persisting',
            turn_in_flight: true,
            transition_authority: 'project_controller',
            ...(realtimeSteering ? { realtime_steering: realtimeSteering } : {}),
        };
    }
    return { state: 'idle', turn_in_flight: false, transition_authority: 'project_controller' };
}

/**
 * Derive the turn projection (idle/awaiting_response/persisting) from a folded
 * {@link GraphView}'s `agent_doc.closeout.cycle` phase — symmetric with the
 * JetBrains `AgentDocTurnProjection.fromView`.
 */
export function agentDocTurnProjectionFromView(view: InstanceType<typeof GraphView>): AgentDocTurnProjection {
    const closeout = payloadJson(view.singletonNode(AgentDocNodeType.CLOSEOUT_CYCLE)?.payload);
    return turnProjectionFromPhase(
        stringField(closeout, 'phase'),
        realtimeSteeringField(closeout),
    );
}

/** Render the compact editor-visible status string (matches the kt `.compact()`). */
export function compactAgentDocProjection(projection: import('./agentDocProjection').AgentDocProjection): string {
    return `route=${projection.routeReadiness ?? 'unknown'} pane=${projection.routePaneId ?? '-'} `
        + `transport=${projection.latestTransportPhase ?? '-'} `
        + `proof_markers=${projection.proofMarkers}`;
}
