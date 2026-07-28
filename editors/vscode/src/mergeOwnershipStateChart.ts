import {
    ChartBuilder,
    StateBuilder,
    StateChart,
} from '@lazily-hub/lazily-js/statechart';

/**
 * VS Code-side projection of the binary-owned merge-ownership state machine.
 *
 * The controller remains authoritative for disk-write permission. JavaScript
 * has no shared-thread reactive Context, so this chart owns only the extension
 * host's local replica lifecycle and uses the same phase/event vocabulary as
 * `agent_doc_merge::ownership`.
 */
export const MERGE_OWNERSHIP_PHASES = [
    'detached',
    'attached',
    'editor_owns_buffer',
    'binary_write_requested',
    'lazily_patch_applied_proven',
    'committed',
] as const;

export type MergeOwnershipPhase = (typeof MERGE_OWNERSHIP_PHASES)[number];

export const MERGE_OWNERSHIP_EVENTS = [
    'editor_attached',
    'editor_buffer_observed',
    'editor_detached',
    'heartbeat_stale',
    'binary_write_requested',
    'lazily_patch_applied_observed',
    'committed',
] as const;

export type MergeOwnershipEvent = (typeof MERGE_OWNERSHIP_EVENTS)[number];

const ROOT = 'merge_ownership';

const DEFINITION = new ChartBuilder()
    .state(StateBuilder.compound(ROOT, 'detached'))
    .state(
        StateBuilder.atomic('detached')
            .parent(ROOT)
            .on('editor_attached', 'attached')
            .on('editor_buffer_observed', 'editor_owns_buffer')
            .on('editor_detached', 'detached')
            .on('committed', 'committed'),
    )
    .state(
        StateBuilder.atomic('attached')
            .parent(ROOT)
            .on('editor_attached', 'attached')
            .on('editor_buffer_observed', 'editor_owns_buffer')
            .on('editor_detached', 'detached')
            .on('heartbeat_stale', 'detached'),
    )
    .state(
        StateBuilder.atomic('editor_owns_buffer')
            .parent(ROOT)
            .on('editor_attached', 'attached')
            .on('editor_buffer_observed', 'editor_owns_buffer')
            .on('editor_detached', 'detached')
            .on('binary_write_requested', 'binary_write_requested'),
    )
    .state(
        StateBuilder.atomic('binary_write_requested')
            .parent(ROOT)
            .on('binary_write_requested', 'binary_write_requested')
            .on('lazily_patch_applied_observed', 'lazily_patch_applied_proven'),
    )
    .state(
        StateBuilder.atomic('lazily_patch_applied_proven')
            .parent(ROOT)
            .on('lazily_patch_applied_observed', 'lazily_patch_applied_proven')
            .on('committed', 'committed'),
    )
    .state(
        StateBuilder.final('committed')
            .parent(ROOT)
            .on('committed', 'committed'),
    )
    .build();

export class MergeOwnershipStateChart {
    private readonly chart = new StateChart(DEFINITION);

    get phase(): MergeOwnershipPhase {
        const activeLeaf = this.chart.activeLeaves()[0];
        if (!MERGE_OWNERSHIP_PHASES.includes(activeLeaf as MergeOwnershipPhase)) {
            throw new Error(`unknown merge-ownership phase: ${activeLeaf ?? '<none>'}`);
        }
        return activeLeaf as MergeOwnershipPhase;
    }

    get editorAttached(): boolean {
        const phase = this.phase;
        return phase !== 'detached' && phase !== 'committed';
    }

    send(event: MergeOwnershipEvent): boolean {
        return this.chart.send(event);
    }
}
