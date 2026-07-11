import { before, describe, it } from 'node:test';
import assert from 'node:assert';
import * as os from 'os';
import * as path from 'path';
import * as fs from 'fs';
import {
    AgentDocNodeType,
    StateGraphMirror,
    compactMirrorSummary,
    decodePayload,
    initStateMirror,
} from './stateMirror';

// S5: load the lazily-js StateGraphMirror ctor before any `new StateGraphMirror()`.
before(async () => {
    await initStateMirror();
});
import {
    documentHash,
    mirrorEpochForFile,
    mirrorSummaryForFile,
    evictStateMirrorForFile,
    debugStateMirrorCount,
    seedStateMirrorMessageForTest,
} from './native';

/** base64(serde_json(struct)) — the FFI node payload encoding. */
function payload(obj: unknown): string {
    return Buffer.from(JSON.stringify(obj), 'utf-8').toString('base64');
}

function routeSlot(): number {
    return 1001;
}
function patchSlot(): number {
    return 2002;
}
function closeoutSlot(): number {
    return 2502;
}
function proofSlot(): number {
    return 3003;
}

function snapshotMessage(epoch: number, docHash: string): string {
    return JSON.stringify({
        type: 'snapshot',
        epoch,
        document_hash: docHash,
        nodes: [
            {
                slot_id: routeSlot(),
                type_tag: AgentDocNodeType.ROUTE,
                payload: payload({ readiness: 'dispatch_authorized', pane_id: '%2' }),
            },
        ],
        edges: [],
        roots: [routeSlot()],
    });
}

describe('StateGraphMirror (#r5at lazily-js reactive mirror)', () => {
    it('applies a snapshot and derives the summary from tracked cells', () => {
        const mirror = new StateGraphMirror();
        assert.strictEqual(mirror.isInitialized, false);

        assert.strictEqual(mirror.applyMessage(snapshotMessage(3, 'doc-a')), true);

        assert.strictEqual(mirror.isInitialized, true);
        assert.strictEqual(mirror.documentHash, 'doc-a');
        assert.strictEqual(mirror.epoch, 3);
        assert.strictEqual(mirror.nodeCount, 1);

        const summary = mirror.summary();
        assert.strictEqual(summary.routeReadiness, 'dispatch_authorized');
        assert.strictEqual(summary.routePaneId, '%2');
        assert.strictEqual(summary.proofMarkers, 0);
        assert.strictEqual(summary.latestTransportPhase, undefined);
    });

    it('applies a delta reactively: node_add + cell_set update the derived summary', () => {
        const mirror = new StateGraphMirror();
        mirror.applyMessage(snapshotMessage(1, 'doc-b'));

        // Before the delta: no transport patch, route still authorized.
        assert.strictEqual(mirror.summary().latestTransportPhase, undefined);
        assert.strictEqual(mirror.summary().routeReadiness, 'dispatch_authorized');

        const delta = JSON.stringify({
            type: 'delta',
            base_epoch: 1,
            epoch: 4,
            document_hash: 'doc-b',
            ops: [
                {
                    op: 'node_add',
                    slot_id: patchSlot(),
                    type_tag: AgentDocNodeType.TRANSPORT_PATCH,
                    payload: payload({ phase: 'queued' }),
                },
                {
                    op: 'node_add',
                    slot_id: proofSlot(),
                    type_tag: AgentDocNodeType.PROOF_MARKER,
                    payload: payload({ phase: 'observed', sources: ['route'] }),
                },
                // Reactively flip the route readiness.
                {
                    op: 'cell_set',
                    slot_id: routeSlot(),
                    payload: payload({ readiness: 'dispatch_proven', pane_id: '%2' }),
                },
                // Reactively advance the transport phase.
                {
                    op: 'cell_set',
                    slot_id: patchSlot(),
                    payload: payload({ phase: 'applied' }),
                },
            ],
        });

        assert.strictEqual(mirror.applyMessage(delta), true);
        assert.strictEqual(mirror.epoch, 4);

        const summary = mirror.summary();
        assert.strictEqual(summary.routeReadiness, 'dispatch_proven', 'cell_set flips route reactively');
        assert.strictEqual(summary.latestTransportPhase, 'applied', 'cell_set advances transport phase reactively');
        assert.strictEqual(summary.proofMarkers, 1, 'node_add proof marker counted reactively');
    });

    it('node_remove + edge ops are applied verbatim', () => {
        const mirror = new StateGraphMirror();
        mirror.applyMessage(snapshotMessage(1, 'doc-c'));
        mirror.applyMessage(JSON.stringify({
            type: 'delta',
            base_epoch: 1,
            epoch: 2,
            document_hash: 'doc-c',
            ops: [
                { op: 'node_add', slot_id: patchSlot(), type_tag: AgentDocNodeType.TRANSPORT_PATCH, payload: payload({ phase: 'queued' }) },
                { op: 'edge_add', dependent: patchSlot(), dependency: routeSlot() },
            ],
        }));
        assert.strictEqual(mirror.nodeCount, 2);

        mirror.applyMessage(JSON.stringify({
            type: 'delta',
            base_epoch: 2,
            epoch: 3,
            document_hash: 'doc-c',
            ops: [
                { op: 'edge_remove', dependent: patchSlot(), dependency: routeSlot() },
                { op: 'node_remove', slot_id: patchSlot() },
            ],
        }));
        assert.strictEqual(mirror.nodeCount, 1);
        assert.strictEqual(mirror.summary().latestTransportPhase, undefined);
    });

    it('idempotent no-op delta only advances the epoch (#qdedupsync property)', () => {
        const mirror = new StateGraphMirror();
        mirror.applyMessage(snapshotMessage(2, 'doc-d'));
        const before = mirror.summary();

        assert.strictEqual(mirror.applyMessage(JSON.stringify({
            type: 'delta',
            base_epoch: 2,
            epoch: 5,
            document_hash: 'doc-d',
            ops: [],
        })), true);

        assert.strictEqual(mirror.epoch, 5);
        assert.deepStrictEqual(mirror.summary(), before, 'no-op delta leaves derived state unchanged');
    });

    it('epoch never regresses on an out-of-order delta', () => {
        const mirror = new StateGraphMirror();
        mirror.applyMessage(snapshotMessage(10, 'doc-e'));
        mirror.applyMessage(JSON.stringify({ type: 'delta', base_epoch: 10, epoch: 4, document_hash: 'doc-e', ops: [] }));
        assert.strictEqual(mirror.epoch, 10, 'epoch is max(current, delta.epoch)');
    });

    it('rejects malformed / unknown-type messages (fail safe)', () => {
        const mirror = new StateGraphMirror();
        assert.strictEqual(mirror.applyMessage('{not json'), false);
        assert.strictEqual(mirror.applyMessage(JSON.stringify({ type: 'bogus' })), false);
        assert.strictEqual(mirror.applyMessage(JSON.stringify({ epoch: 1 })), false);
        assert.strictEqual(mirror.isInitialized, false);
    });

    it('compactMirrorSummary renders the editor-visible status string (kt .compact() parity)', () => {
        assert.strictEqual(
            compactMirrorSummary({
                routeReadiness: 'dispatch_proven',
                routePaneId: '%2',
                latestTransportPatchId: 'patch-2',
                latestTransportPhase: 'applied',
                proofMarkers: 1,
            }),
            'route=dispatch_proven pane=%2 transport=patch-2:applied proof_markers=1',
        );
        assert.strictEqual(
            compactMirrorSummary({ proofMarkers: 0 }),
            'route=unknown pane=- transport=-:- proof_markers=0',
        );
    });

    it('turnProjection derives from the closeout cycle phase', () => {
        const mirror = new StateGraphMirror();
        assert.strictEqual(mirror.applyMessage(JSON.stringify({
            type: 'snapshot',
            epoch: 1,
            document_hash: 'doc-turn',
            nodes: [
                {
                    slot_id: closeoutSlot(),
                    type_tag: AgentDocNodeType.CLOSEOUT_CYCLE,
                    payload: payload({ phase: 'preflight_started' }),
                },
            ],
            edges: [],
            roots: [closeoutSlot()],
        })), true);
        assert.deepStrictEqual(mirror.turnProjection(), {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
        });

        assert.strictEqual(mirror.applyMessage(JSON.stringify({
            type: 'delta',
            base_epoch: 1,
            epoch: 2,
            document_hash: 'doc-turn',
            ops: [
                { op: 'cell_set', slot_id: closeoutSlot(), payload: payload({ phase: 'write_applied' }) },
            ],
        })), true);
        assert.deepStrictEqual(mirror.turnProjection(), {
            state: 'persisting',
            turn_in_flight: true,
            transition_authority: 'project_controller',
        });

        assert.strictEqual(mirror.applyMessage(JSON.stringify({
            type: 'delta',
            base_epoch: 2,
            epoch: 3,
            document_hash: 'doc-turn',
            ops: [
                { op: 'cell_set', slot_id: closeoutSlot(), payload: payload({ phase: 'committed' }) },
            ],
        })), true);
        assert.deepStrictEqual(mirror.turnProjection(), {
            state: 'idle',
            turn_in_flight: false,
            transition_authority: 'project_controller',
        });
    });

    it('decodePayload decodes base64(serde_json(struct)) and fails safe', () => {
        assert.deepStrictEqual(decodePayload(payload({ phase: 'queued' })), { phase: 'queued' });
        assert.strictEqual(decodePayload(null), null);
        assert.strictEqual(decodePayload(''), null);
        assert.strictEqual(decodePayload('!!!not-base64-json'), null);
    });
});

describe('native per-document mirror registry (#r5at)', () => {
    it('seedStateMirrorMessageForTest drives the read-path summary from tracked cells', () => {
        const tmp = path.join(os.tmpdir(), `agent_doc_mirror_${Date.now()}.md`);
        fs.writeFileSync(tmp, 'state');
        try {
            const docHash = documentHash(tmp);
            // No mirror yet → cold read path (null without FFI).
            assert.strictEqual(mirrorSummaryForFile(tmp), null);

            assert.strictEqual(
                seedStateMirrorMessageForTest(tmp, snapshotMessage(7, docHash)),
                true,
            );

            const summary = mirrorSummaryForFile(tmp);
            assert.ok(summary, 'mirror summary derived from seeded snapshot');
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

    it('evictStateMirrorForFile removes only the targeted document mirror', () => {
        const a = path.join(os.tmpdir(), `agent_doc_mirror_a_${Date.now()}.md`);
        const b = path.join(os.tmpdir(), `agent_doc_mirror_b_${Date.now()}.md`);
        fs.writeFileSync(a, 'a');
        fs.writeFileSync(b, 'b');
        try {
            const baseline = debugStateMirrorCount();
            seedStateMirrorMessageForTest(a, snapshotMessage(1, documentHash(a)));
            seedStateMirrorMessageForTest(b, snapshotMessage(1, documentHash(b)));
            assert.strictEqual(debugStateMirrorCount(), baseline + 2);

            evictStateMirrorForFile(a);
            assert.strictEqual(debugStateMirrorCount(), baseline + 1);
            assert.strictEqual(mirrorSummaryForFile(a), null);
            assert.ok(mirrorSummaryForFile(b), 'untargeted mirror survives');
        } finally {
            evictStateMirrorForFile(a);
            evictStateMirrorForFile(b);
            fs.unlinkSync(a);
            fs.unlinkSync(b);
        }
    });
});
