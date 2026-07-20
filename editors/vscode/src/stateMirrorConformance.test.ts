import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as path from 'path';
import * as fs from 'fs';
import {
    AgentDocNodeType,
    GraphView,
    agentDocProjectionFromView,
    applyIpcMessageToView,
} from './stateMirror.js';
import { fileURLToPath } from 'node:url';

// ESM has no `__dirname`; derive it from the module URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * `#lzsync` 3B — cross-editor convergence parity (JS half), native wire.
 *
 * The shared canonical input is the lazily-spec conformance fixture pair
 * (`conformance/agent-doc/{snapshot,delta}_agent_doc_state.json`), vendored into
 * `editors/vscode/test-fixtures/` byte-identical from
 * `src/lazily-spec/conformance/agent-doc/`. The Rust authoritative graph
 * (`state_wire.rs mod conformance_parity`) and the JetBrains view
 * (`StateGraphMirrorConformanceTest.kt`) assert the SAME expectation against the
 * SAME fixtures, so all three implementations are pinned to one canonical answer
 * without a live cross-language harness:
 *
 * | field            | snapshot          | after delta |
 * |------------------|-------------------|-------------|
 * | cycle_phase      | preflight_started | committed   |
 * | queue_head_phase | selected          | completed   |
 * | epoch            | 3                 | 6           |
 * | transport phase  | (absent)          | applied     |
 *
 * The fixtures already carry the lazily-spec *generic graph* wire shape
 * (`node` / `state.Payload` byte arrays / externally-tagged `{ Snapshot: … }` /
 * `{ CellSet: … }`), which is exactly the native `IpcMessage` JSON. The clean
 * split (`#lzsync` 3B) means the generic {@link GraphView} folds them DIRECTLY —
 * no adaptation to a bespoke agent-doc wire — and agent-doc's projection layers
 * the domain read on top.
 */

const FIXTURE_DIR = path.resolve(__dirname, '../test-fixtures/conformance/agent-doc');

function loadFixture(name: string): any {
    const raw = fs.readFileSync(path.join(FIXTURE_DIR, name), 'utf-8');
    return JSON.parse(raw);
}

/** The fixture's `wire` object is already the native externally-tagged IpcMessage JSON. */
function loadMessage(name: string): string {
    return JSON.stringify(loadFixture(name).wire);
}

/** Read the `phase` field of the single node of `typeTag`, or undefined. */
function phaseOf(view: InstanceType<typeof GraphView>, typeTag: string): string | undefined {
    const bytes = view.singletonNode(typeTag)?.payload;
    if (!bytes) return undefined;
    const parsed = JSON.parse(new TextDecoder().decode(Uint8Array.from(bytes)));
    const phase = parsed?.phase;
    return typeof phase === 'string' ? phase : undefined;
}

describe('state view cross-editor conformance parity (#lzsync 3B)', () => {
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

    it('js view applying canonical snapshot then delta converges to the pinned expectation', () => {
        const view = new GraphView();
        assert.strictEqual(applyIpcMessageToView(view, loadMessage('snapshot_agent_doc_state.json')), 'snapshot');

        // Snapshot-time canonical phases (preflight_started / selected).
        assert.strictEqual(view.epoch, 3);
        assert.strictEqual(phaseOf(view, AgentDocNodeType.CLOSEOUT_CYCLE), 'preflight_started');
        assert.strictEqual(phaseOf(view, AgentDocNodeType.QUEUE_HEAD), 'selected');

        // Apply the warm delta — the view must converge to the after-state.
        assert.strictEqual(applyIpcMessageToView(view, loadMessage('delta_agent_doc_state.json')), 'delta');
        assert.strictEqual(view.epoch, 6);
        assert.strictEqual(phaseOf(view, AgentDocNodeType.CLOSEOUT_CYCLE), 'committed');
        assert.strictEqual(phaseOf(view, AgentDocNodeType.QUEUE_HEAD), 'completed');

        // Transport patch added mid-cycle, phase applied — readable via the
        // domain projection the editor consumes.
        const projection = agentDocProjectionFromView(view);
        assert.strictEqual(projection.latestTransportPhase, 'applied');
        assert.strictEqual(view.nodesOfType(AgentDocNodeType.TRANSPORT_PATCH).length, 1);
    });

    it('js view reapplying the canonical delta is idempotent', () => {
        const view = new GraphView();
        applyIpcMessageToView(view, loadMessage('snapshot_agent_doc_state.json'));
        const delta = loadMessage('delta_agent_doc_state.json');
        applyIpcMessageToView(view, delta);
        const epochAfterFirst = view.epoch;
        const nodesAfterFirst = view.nodeCount;
        const projectionAfterFirst = JSON.stringify(agentDocProjectionFromView(view));

        // Re-emit the SAME delta — the pure-fold property means a replay is a
        // no-op: epoch frontier holds, node set + derived projection unchanged.
        applyIpcMessageToView(view, delta);
        assert.strictEqual(view.epoch, epochAfterFirst);
        assert.strictEqual(view.nodeCount, nodesAfterFirst);
        assert.strictEqual(JSON.stringify(agentDocProjectionFromView(view)), projectionAfterFirst);
        assert.strictEqual(phaseOf(view, AgentDocNodeType.CLOSEOUT_CYCLE), 'committed');
    });
});
