export interface TabSyncState {
    activeFile: string;
    visibleSignature: string;
}

export interface TabChangeInput {
    activeFile: string;
    visibleMd: string[];
    visibleColumns?: string[][];
    previous?: TabSyncState;
}

export type TabChangeCommand =
    | { kind: 'focus'; args: ['focus', string] }
    | { kind: 'sync'; args: string[] };

export interface PlannedTabChange {
    command: TabChangeCommand;
    nextState: TabSyncState;
}

export interface TabSyncCommandResult {
    applied: boolean;
    shouldRetry: boolean;
}

const SAFE_PASSIVE_LAYOUT_PRESERVED_MARKER =
    '[sync] safe passive sync preserved the current tmux layout because';
const SAFE_PASSIVE_LAYOUT_RESELECTED_FOCUS_MARKER =
    '[sync] safe_passive_layout_preserved_reselected_focus';

export function shouldReplayQueuedTabChange(startedGeneration: number, latestGeneration: number): boolean {
    return latestGeneration > startedGeneration;
}

export function isPreservedLayoutOutput(output: string): boolean {
    return output.includes(SAFE_PASSIVE_LAYOUT_PRESERVED_MARKER);
}

export function analyzeTabSyncCommandResult(
    command: TabChangeCommand,
    exitCode: number,
    output: string,
): TabSyncCommandResult {
    if (exitCode !== 0) {
        return { applied: false, shouldRetry: false };
    }
    if (command.kind === 'sync' && isPreservedLayoutOutput(output)) {
        if (output.includes(SAFE_PASSIVE_LAYOUT_RESELECTED_FOCUS_MARKER)) {
            return { applied: true, shouldRetry: false };
        }
        return { applied: false, shouldRetry: true };
    }
    return { applied: true, shouldRetry: false };
}

function normalizeVisibleMd(visibleMd: string[]): string[] {
    return [...new Set(visibleMd)].sort();
}

export function normalizeVisibleColumns(visibleColumns: string[][]): string[][] {
    return visibleColumns.map((column) => [...new Set(column.filter((file) => file.length > 0))]);
}

export function flattenVisibleColumns(visibleColumns: string[][]): string[] {
    return normalizeVisibleMd(normalizeVisibleColumns(visibleColumns).flat());
}

export function visibleSignatureFromColumns(visibleColumns: string[][]): string {
    return normalizeVisibleColumns(visibleColumns)
        .map((column) => column.join('\u0001'))
        .join('\u0000');
}

export function buildSyncCommandArgs(
    visibleColumns: string[][],
    activeFile: string,
    options?: { noAutostart?: boolean },
): string[] {
    const columns = normalizeVisibleColumns(visibleColumns);
    const normalizedColumns = columns.length > 0 ? columns : [[activeFile]];
    const args = ['sync'];
    for (const column of normalizedColumns) {
        args.push('--col', column.join(','));
    }
    args.push('--focus', activeFile);
    if (options?.noAutostart !== false) {
        args.push('--no-autostart');
    }
    return args;
}

export function buildTabChangeCommand(input: TabChangeInput): PlannedTabChange | null {
    const visibleColumns = normalizeVisibleColumns(input.visibleColumns ?? [input.visibleMd]);
    const visibleMd = normalizeVisibleMd(
        input.visibleMd.length > 0 ? input.visibleMd : flattenVisibleColumns(visibleColumns),
    );
    if (visibleMd.length === 0) {
        return null;
    }

    const nextState: TabSyncState = {
        activeFile: input.activeFile,
        visibleSignature: visibleSignatureFromColumns(visibleColumns),
    };
    const previous = input.previous;
    if (
        previous &&
        previous.activeFile === nextState.activeFile &&
        previous.visibleSignature === nextState.visibleSignature
    ) {
        return null;
    }

    return {
        command: {
            kind: 'sync',
            args: buildSyncCommandArgs(visibleColumns, input.activeFile),
        },
        nextState,
    };
}
