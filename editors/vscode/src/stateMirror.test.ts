import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as os from 'os';
import * as path from 'path';
import * as fs from 'fs';
import {
    AgentDocNodeType,
    GraphView,
    agentDocProjectionFromView,
    agentDocTurnProjectionFromView,
    applyIpcMessageToView,
    compactAgentDocProjection,
} from './stateMirror.js';
import {
    documentHash,
    mirrorEpochForFile,
    mirrorSummaryForFile,
    evictStateMirrorForFile,
    debugStateMirrorCount,
    seedStateMirrorMessageForTest,
} from './native.js';

// --- native wire builders (externally-tagged IpcMessage JSON) -----------------

/** Component JSON object → native `Payload`/`Inline` byte array. */
function inlineBytes(obj: unknown): number[] {
    return [...Buffer.from(JSON.stringify(obj), 'utf-8')];
}

interface SnapshotNode {
    node: number;
    typeTag: string;
    payload?: unknown;
}

function snapshotMsg(epoch: number, nodes: SnapshotNode[]): string {
    return JSON.stringify({
        Snapshot: {
            epoch,
            nodes: nodes.map((n) => ({
                node: n.node,
                type_tag: n.typeTag,
                state: n.payload !== undefined ? { Payload: inlineBytes(n.payload) } : 'Opaque',
            })),
            edges: [],
            roots: [],
        },
    });
}

function deltaMsg(baseEpoch: number, epoch: number, ops: unknown[]): string {
    return JSON.stringify({ Delta: { base_epoch: baseEpoch, epoch, ops } });
}

const cellSet = (node: number, payload: unknown) => ({ CellSet: { node, payload: { Inline: inlineBytes(payload) } } });
const nodeAdd = (node: number, typeTag: string, payload: unknown) => ({
    NodeAdd: { node, type_tag: typeTag, state: { Payload: inlineBytes(payload) } },
});
const nodeRemove = (node: number) => ({ NodeRemove: { node } });
const edgeAdd = (dependent: number, dependency: number) => ({ EdgeAdd: { dependent, dependency } });
const edgeRemove = (dependent: number, dependency: number) => ({ EdgeRemove: { dependent, dependency } });

const ROUTE = 11;
const PATCH = 40;
const CLOSEOUT = 21;
const PROOF = 70;

function routeSnapshot(epoch: number): string {
    return snapshotMsg(epoch, [
        { node: ROUTE, typeTag: AgentDocNodeType.ROUTE, payload: { readiness: 'dispatch_authorized', pane_id: '%2' } },
    ]);
}

describe('GraphView fold + AgentDocProjection (#lzsync 3B clean split)', () => {
    it('applies a native snapshot and derives the projection from folded nodes', () => {
        const view = new GraphView();
        assert.strictEqual(view.isInitialized, false);

        assert.strictEqual(applyIpcMessageToView(view, routeSnapshot(3)), 'snapshot');

        assert.strictEqual(view.isInitialized, true);
        assert.strictEqual(view.epoch, 3);
        assert.strictEqual(view.nodeCount, 1);

        const projection = agentDocProjectionFromView(view);
        assert.strictEqual(projection.routeReadiness, 'dispatch_authorized');
        assert.strictEqual(projection.routePaneId, '%2');
        assert.strictEqual(projection.proofMarkers, 0);
        assert.strictEqual(projection.latestTransportPhase, null);
    });

    it('applies a delta reactively: node_add + cell_set update the derived projection', () => {
        const view = new GraphView();
        applyIpcMessageToView(view, routeSnapshot(1));

        assert.strictEqual(agentDocProjectionFromView(view).latestTransportPhase, null);
        assert.strictEqual(agentDocProjectionFromView(view).routeReadiness, 'dispatch_authorized');

        const delta = deltaMsg(1, 4, [
            nodeAdd(PATCH, AgentDocNodeType.TRANSPORT_PATCH, { phase: 'queued' }),
            nodeAdd(PROOF, AgentDocNodeType.PROOF_MARKER, { phase: 'observed', sources: ['route'] }),
            cellSet(ROUTE, { readiness: 'dispatch_proven', pane_id: '%2' }),
            cellSet(PATCH, { phase: 'applied' }),
        ]);

        assert.strictEqual(applyIpcMessageToView(view, delta), 'delta');
        assert.strictEqual(view.epoch, 4);

        const projection = agentDocProjectionFromView(view);
        assert.strictEqual(projection.routeReadiness, 'dispatch_proven', 'cell_set flips route reactively');
        assert.strictEqual(projection.latestTransportPhase, 'applied', 'cell_set advances transport phase reactively');
        assert.strictEqual(projection.proofMarkers, 1, 'node_add proof marker counted reactively');
    });

    it('node_remove + edge ops are applied verbatim', () => {
        const view = new GraphView();
        applyIpcMessageToView(view, routeSnapshot(1));
        applyIpcMessageToView(view, deltaMsg(1, 2, [
            nodeAdd(PATCH, AgentDocNodeType.TRANSPORT_PATCH, { phase: 'queued' }),
            edgeAdd(PATCH, ROUTE),
        ]));
        assert.strictEqual(view.nodeCount, 2);

        applyIpcMessageToView(view, deltaMsg(2, 3, [
            edgeRemove(PATCH, ROUTE),
            nodeRemove(PATCH),
        ]));
        assert.strictEqual(view.nodeCount, 1);
        assert.strictEqual(agentDocProjectionFromView(view).latestTransportPhase, null);
    });

    it('idempotent no-op delta only advances the epoch (#qdedupsync property)', () => {
        const view = new GraphView();
        applyIpcMessageToView(view, routeSnapshot(2));
        const before = agentDocProjectionFromView(view);

        assert.strictEqual(applyIpcMessageToView(view, deltaMsg(2, 5, [])), 'delta');

        assert.strictEqual(view.epoch, 5);
        assert.deepStrictEqual(agentDocProjectionFromView(view), before, 'no-op delta leaves derived state unchanged');
    });

    it('epoch never regresses on an out-of-order delta', () => {
        const view = new GraphView();
        applyIpcMessageToView(view, routeSnapshot(10));
        applyIpcMessageToView(view, deltaMsg(10, 4, []));
        assert.strictEqual(view.epoch, 10, 'epoch is max(current, delta.epoch)');
    });

    it('rejects malformed / unknown-variant messages (fail safe)', () => {
        const view = new GraphView();
        assert.strictEqual(applyIpcMessageToView(view, '{not json'), null);
        assert.strictEqual(applyIpcMessageToView(view, JSON.stringify({ Bogus: {} })), null);
        assert.strictEqual(applyIpcMessageToView(view, JSON.stringify({ epoch: 1 })), null);
        assert.strictEqual(view.isInitialized, false);
    });

    it('compactAgentDocProjection renders the editor-visible status string (kt .compact() parity)', () => {
        assert.strictEqual(
            compactAgentDocProjection({
                routeReadiness: 'dispatch_proven',
                routePaneId: '%2',
                latestTransportPhase: 'applied',
                proofMarkers: 1,
            }),
            'route=dispatch_proven pane=%2 transport=applied proof_markers=1',
        );
        assert.strictEqual(
            compactAgentDocProjection({
                routeReadiness: null,
                routePaneId: null,
                latestTransportPhase: null,
                proofMarkers: 0,
            }),
            'route=unknown pane=- transport=- proof_markers=0',
        );
    });

    it('agentDocTurnProjectionFromView derives from the closeout cycle phase', () => {
        const view = new GraphView();
        assert.strictEqual(applyIpcMessageToView(view, snapshotMsg(1, [
            {
                node: CLOSEOUT,
                typeTag: AgentDocNodeType.CLOSEOUT_CYCLE,
                payload: {
                    phase: 'preflight_started',
                    realtime_steering: {
                        observed_content_hash: 'current-hash',
                        state: 'prompt_target',
                        count: 2,
                        preview: 'First edit',
                        verbatim: 'First edit\n\nSecond edit',
                        elements: {
                            first: {
                                state: 'prompt_target',
                                ordinal: 0,
                                preview: 'First edit',
                                verbatim: 'First edit',
                            },
                            second: {
                                state: 'prompt_target',
                                ordinal: 1,
                                preview: 'Second edit',
                                verbatim: 'Second edit',
                            },
                        },
                    },
                },
            },
        ])), 'snapshot');
        assert.deepStrictEqual(agentDocTurnProjectionFromView(view), {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
            realtime_steering: {
                observed_content_hash: 'current-hash',
                state: 'prompt_target',
                count: 2,
                preview: 'First edit',
                verbatim: 'First edit\n\nSecond edit',
                elements: {
                    first: {
                        state: 'prompt_target',
                        ordinal: 0,
                        preview: 'First edit',
                        verbatim: 'First edit',
                    },
                    second: {
                        state: 'prompt_target',
                        ordinal: 1,
                        preview: 'Second edit',
                        verbatim: 'Second edit',
                    },
                },
            },
        });

        applyIpcMessageToView(view, deltaMsg(1, 2, [cellSet(CLOSEOUT, { phase: 'write_applied' })]));
        assert.deepStrictEqual(agentDocTurnProjectionFromView(view), {
            state: 'persisting',
            turn_in_flight: true,
            transition_authority: 'project_controller',
        });

        applyIpcMessageToView(view, deltaMsg(2, 3, [cellSet(CLOSEOUT, { phase: 'committed' })]));
        assert.deepStrictEqual(agentDocTurnProjectionFromView(view), {
            state: 'idle',
            turn_in_flight: false,
            transition_authority: 'project_controller',
        });
    });

    it('preserves a controller-confirmed empty steering set receipt', () => {
        const view = new GraphView();
        applyIpcMessageToView(view, snapshotMsg(1, [
            {
                node: CLOSEOUT,
                typeTag: AgentDocNodeType.CLOSEOUT_CYCLE,
                payload: {
                    phase: 'preflight_started',
                    realtime_steering: {
                        observed_content_hash: 'current-hash',
                    },
                },
            },
        ]));

        assert.deepStrictEqual(agentDocTurnProjectionFromView(view), {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
            realtime_steering: {
                observed_content_hash: 'current-hash',
            },
        });
    });
});

describe('native per-document view registry (#lzsync 3B)', () => {
    it('seedStateMirrorMessageForTest drives the read-path projection from folded nodes', () => {
        const tmp = path.join(os.tmpdir(), `agent_doc_view_${Date.now()}.md`);
        fs.writeFileSync(tmp, 'state');
        try {
            // No view yet → cold read path (null without FFI).
            assert.strictEqual(mirrorSummaryForFile(tmp), null);

            assert.strictEqual(seedStateMirrorMessageForTest(tmp, routeSnapshot(7)), true);

            const summary = mirrorSummaryForFile(tmp);
            assert.ok(summary, 'projection derived from seeded snapshot');
            assert.strictEqual(summary!.routeReadiness, 'dispatch_authorized');
            assert.strictEqual(summary!.routePaneId, '%2');
            assert.strictEqual(mirrorEpochForFile(tmp), 7);

            // Eviction clears it (reused-path stale-state guard).
            evictStateMirrorForFile(tmp);
            assert.strictEqual(mirrorSummaryForFile(tmp), null);
            assert.strictEqual(mirrorEpochForFile(tmp), null);
        } finally {
            evictStateMirrorForFile(tmp);
            fs.unlinkSync(tmp);
        }
    });

    it('evictStateMirrorForFile removes only the targeted document view', () => {
        const a = path.join(os.tmpdir(), `agent_doc_view_a_${Date.now()}.md`);
        const b = path.join(os.tmpdir(), `agent_doc_view_b_${Date.now()}.md`);
        fs.writeFileSync(a, 'a');
        fs.writeFileSync(b, 'b');
        try {
            const baseline = debugStateMirrorCount();
            seedStateMirrorMessageForTest(a, routeSnapshot(1));
            seedStateMirrorMessageForTest(b, routeSnapshot(1));
            assert.strictEqual(debugStateMirrorCount(), baseline + 2);

            evictStateMirrorForFile(a);
            assert.strictEqual(debugStateMirrorCount(), baseline + 1);
            assert.strictEqual(mirrorSummaryForFile(a), null);
            assert.ok(mirrorSummaryForFile(b), 'untargeted view survives');
        } finally {
            evictStateMirrorForFile(a);
            evictStateMirrorForFile(b);
            fs.unlinkSync(a);
            fs.unlinkSync(b);
        }
    });
});
