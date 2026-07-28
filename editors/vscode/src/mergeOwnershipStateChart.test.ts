import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    MERGE_OWNERSHIP_EVENTS,
    MERGE_OWNERSHIP_PHASES,
    MergeOwnershipStateChart,
    type MergeOwnershipEvent,
    type MergeOwnershipPhase,
} from './mergeOwnershipStateChart.js';

function chartAt(phase: MergeOwnershipPhase): MergeOwnershipStateChart {
    const chart = new MergeOwnershipStateChart();
    const path: Record<MergeOwnershipPhase, MergeOwnershipEvent[]> = {
        detached: [],
        attached: ['editor_attached'],
        editor_owns_buffer: ['editor_attached', 'editor_buffer_observed'],
        binary_write_requested: [
            'editor_attached',
            'editor_buffer_observed',
            'binary_write_requested',
        ],
        lazily_patch_applied_proven: [
            'editor_attached',
            'editor_buffer_observed',
            'binary_write_requested',
            'lazily_patch_applied_observed',
        ],
        committed: ['committed'],
    };
    for (const event of path[phase]) assert.equal(chart.send(event), true);
    assert.equal(chart.phase, phase);
    return chart;
}

describe('merge ownership state chart', () => {
    it('matches the binary happy path and attachment projection', () => {
        const chart = new MergeOwnershipStateChart();
        assert.equal(chart.phase, 'detached');
        assert.equal(chart.editorAttached, false);

        assert.equal(chart.send('editor_attached'), true);
        assert.equal(chart.phase, 'attached');
        assert.equal(chart.editorAttached, true);

        assert.equal(chart.send('editor_buffer_observed'), true);
        assert.equal(chart.phase, 'editor_owns_buffer');
        assert.equal(chart.send('binary_write_requested'), true);
        assert.equal(chart.phase, 'binary_write_requested');
        assert.equal(chart.send('lazily_patch_applied_observed'), true);
        assert.equal(chart.phase, 'lazily_patch_applied_proven');
        assert.equal(chart.send('committed'), true);
        assert.equal(chart.phase, 'committed');
        assert.equal(chart.editorAttached, false);
    });

    it('keeps the complete transition matrix aligned with binary ownership rules', () => {
        const expected: Record<
            MergeOwnershipPhase,
            Partial<Record<MergeOwnershipEvent, MergeOwnershipPhase>>
        > = {
            detached: {
                editor_attached: 'attached',
                editor_buffer_observed: 'editor_owns_buffer',
                editor_detached: 'detached',
                committed: 'committed',
            },
            attached: {
                editor_attached: 'attached',
                editor_buffer_observed: 'editor_owns_buffer',
                editor_detached: 'detached',
                heartbeat_stale: 'detached',
            },
            editor_owns_buffer: {
                editor_attached: 'attached',
                editor_buffer_observed: 'editor_owns_buffer',
                editor_detached: 'detached',
                binary_write_requested: 'binary_write_requested',
            },
            binary_write_requested: {
                binary_write_requested: 'binary_write_requested',
                lazily_patch_applied_observed: 'lazily_patch_applied_proven',
            },
            lazily_patch_applied_proven: {
                lazily_patch_applied_observed: 'lazily_patch_applied_proven',
                committed: 'committed',
            },
            committed: {
                committed: 'committed',
            },
        };

        for (const phase of MERGE_OWNERSHIP_PHASES) {
            for (const event of MERGE_OWNERSHIP_EVENTS) {
                const chart = chartAt(phase);
                const next = expected[phase][event];
                assert.equal(
                    chart.send(event),
                    next !== undefined,
                    `${phase} + ${event} acceptance`,
                );
                assert.equal(chart.phase, next ?? phase, `${phase} + ${event} state`);
            }
        }
    });
});
