import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as path from 'path';
import * as fs from 'fs';
import { AgentDocNodeType, StateGraphMirror } from './stateMirror';

/**
 * `#lazilystatesync5` / `#6n5j` — cross-editor convergence parity (JS half).
 *
 * The shared canonical input is the lazily-spec conformance fixture pair
 * (`conformance/agent-doc/{snapshot,delta}_agent_doc_state.json`), vendored into
 * `editors/vscode/test-fixtures/` byte-identical from
 * `src/lazily-spec/conformance/agent-doc/`. The Rust authoritative graph
 * (`state_wire.rs mod conformance_parity`) and the JetBrains mirror
 * (`StateGraphMirrorConformanceTest.kt`) assert the SAME expectation against the
 * SAME fixtures, so all three implementations are pinned to one canonical answer
 * without a live cross-language harness:
 *
 * | field            | snapshot          | after delta |
 * |------------------|-------------------|-------------|
 * | cycle_phase      | preflight_started | committed   |
 * | queue_head_phase | selected          | completed   |
 * | epoch            | 3                 | 6           |
 * | transport phase  | (absent)          | applied       |
 *
 * The fixtures use the lazily-spec *generic graph* wire shape
 * (`node` / `state.Payload` byte arrays / adjacently-tagged `{ CellSet: … }`).
 * The agent-doc StateGraphMirror applies the flattened agent-doc wire shape;
 * `adaptSnapshot` / `adaptDelta` translate the canonical generic ops into the
 * agent-doc mirror format, then we assert the mirror converges to the pinned
 * expectation.
 */

const FIXTURE_DIR = path.resolve(__dirname, '../test-fixtures/conformance/agent-doc');

function loadFixture(name: string): any {
    const raw = fs.readFileSync(path.join(FIXTURE_DIR, name), 'utf-8');
    return JSON.parse(raw);
}

/** lazily-spec byte array → base64 string (the agent-doc payload encoding). */
function bytesToBase64(arr: number[]): string {
    return Buffer.from(Uint8Array.from(arr)).toString('base64');
}

/** Translate the generic-graph snapshot fixture into the agent-doc mirror wire. */
function adaptSnapshot(fixture: any): string {
    const wire = fixture.wire.Snapshot;
    const nodes = wire.nodes.map((n: any) => ({
        slot_id: n.node,
        type_tag: n.type_tag,
        state: 'resolved',
        payload: bytesToBase64(n.state.Payload),
    }));
    const edges = wire.edges.map((e: any) => ({ dependent: e.dependent, dependency: e.dependency }));
    return JSON.stringify({
        type: 'snapshot',
        epoch: wire.epoch,
        document_hash: 'parity-doc',
        nodes,
        edges,
        roots: [],
    });
}

/** Translate the generic-graph delta fixture into the agent-doc mirror wire. */
function adaptDelta(fixture: any): string {
    const wire = fixture.wire.Delta;
    const ops = wire.ops.map((op: any) => {
        const key = Object.keys(op)[0];
        const body = op[key];
        switch (key) {
            case 'CellSet':
                return { op: 'cell_set', slot_id: body.node, payload: bytesToBase64(body.payload.Inline) };
            case 'SlotValue':
                return { op: 'slot_value', slot_id: body.node, payload: bytesToBase64(body.payload.Inline) };
            case 'NodeAdd':
                return {
                    op: 'node_add',
                    slot_id: body.node,
                    type_tag: body.type_tag,
                    payload: bytesToBase64(body.state.Payload),
                };
            case 'NodeRemove':
                return { op: 'node_remove', slot_id: body.node };
            case 'EdgeAdd':
                return { op: 'edge_add', dependent: body.dependent, dependency: body.dependency };
            case 'EdgeRemove':
                return { op: 'edge_remove', dependent: body.dependent, dependency: body.dependency };
            case 'Invalidate':
                return { op: 'invalidate', slot_id: body.node };
            default:
                throw new Error(`unknown generic-graph op: ${key}`);
        }
    });
    return JSON.stringify({
        type: 'delta',
        base_epoch: wire.base_epoch,
        epoch: wire.epoch,
        document_hash: 'parity-doc',
        ops,
    });
}

function phaseOf(mirror: StateGraphMirror, typeTag: string): string | undefined {
    const payload = mirror.payloadObject(typeTag);
    const phase = payload?.phase;
    return typeof phase === 'string' ? phase : undefined;
}

describe('state mirror cross-editor conformance parity (#6n5j)', () => {
    it('fixtures declare the canonical cross-language expectation', () => {
        const snapshot = loadFixture('snapshot_agent_doc_state.json').assertions;
        assert.strictEqual(snapshot.epoch, 3);
        assert.strictEqual(snapshot.cycle_phase, 'preflight_started');
        assert.strictEqual(snapshot.queue_head_phase, 'selected');

        const delta = loadFixture('delta_agent_doc_state.json').assertions;
        assert.strictEqual(delta.base_epoch, 3);
        assert.strictEqual(delta.epoch, 6);
        assert.strictEqual(delta.cycle_phase_after, 'committed');
        assert.strictEqual(delta.queue_head_phase_after, 'completed');
        assert.strictEqual(delta.added_type_tags[0], 'agent_doc.transport.patch');
    });

    it('js mirror applying canonical snapshot then delta converges to the pinned expectation', () => {
        const snapshotFixture = loadFixture('snapshot_agent_doc_state.json');
        const deltaFixture = loadFixture('delta_agent_doc_state.json');

        const mirror = new StateGraphMirror();
        assert.strictEqual(mirror.applyMessage(adaptSnapshot(snapshotFixture)), true);

        // Snapshot-time canonical phases (preflight_started / selected).
        assert.strictEqual(mirror.epoch, 3);
        assert.strictEqual(phaseOf(mirror, AgentDocNodeType.CLOSEOUT_CYCLE), 'preflight_started');
        assert.strictEqual(phaseOf(mirror, AgentDocNodeType.QUEUE_HEAD), 'selected');

        // Apply the warm delta — the mirror must converge to the after-state.
        assert.strictEqual(mirror.applyMessage(adaptDelta(deltaFixture)), true);
        assert.strictEqual(mirror.epoch, 6);
        assert.strictEqual(phaseOf(mirror, AgentDocNodeType.CLOSEOUT_CYCLE), 'committed');
        assert.strictEqual(phaseOf(mirror, AgentDocNodeType.QUEUE_HEAD), 'completed');

        // Transport patch added mid-cycle, phase applied — readable via the
        // reactive summary the editor consumes.
        const summary = mirror.summary();
        assert.strictEqual(summary.latestTransportPhase, 'applied');
        assert.strictEqual(mirror.nodesOfType(AgentDocNodeType.TRANSPORT_PATCH).length, 1);
    });

    it('js mirror reapplying the canonical delta is idempotent', () => {
        const mirror = new StateGraphMirror();
        mirror.applyMessage(adaptSnapshot(loadFixture('snapshot_agent_doc_state.json')));
        const delta = adaptDelta(loadFixture('delta_agent_doc_state.json'));
        mirror.applyMessage(delta);
        const epochAfterFirst = mirror.epoch;
        const nodesAfterFirst = mirror.nodeCount;
        const summaryAfterFirst = JSON.stringify(mirror.summary());

        // Re-emit the SAME delta — the pure-fold property means a replay is a
        // no-op: epoch frontier holds, node set + derived summary unchanged.
        mirror.applyMessage(delta);
        assert.strictEqual(mirror.epoch, epochAfterFirst);
        assert.strictEqual(mirror.nodeCount, nodesAfterFirst);
        assert.strictEqual(JSON.stringify(mirror.summary()), summaryAfterFirst);
        assert.strictEqual(phaseOf(mirror, AgentDocNodeType.CLOSEOUT_CYCLE), 'committed');
    });
});
