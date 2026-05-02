export interface TabSyncState {
    activeFile: string;
    visibleSignature: string;
}

export interface TabChangeInput {
    activeFile: string;
    visibleMd: string[];
    previous?: TabSyncState;
}

export type TabChangeCommand =
    | { kind: 'focus'; args: ['focus', string] }
    | { kind: 'sync'; args: string[] };

export interface PlannedTabChange {
    command: TabChangeCommand;
    nextState: TabSyncState;
}

function normalizeVisibleMd(visibleMd: string[]): string[] {
    return [...new Set(visibleMd)].sort();
}

export function buildTabChangeCommand(input: TabChangeInput): PlannedTabChange | null {
    const visibleMd = normalizeVisibleMd(input.visibleMd);
    if (visibleMd.length === 0) {
        return null;
    }

    const nextState: TabSyncState = {
        activeFile: input.activeFile,
        visibleSignature: visibleMd.join('\u0000'),
    };
    const previous = input.previous;
    if (
        previous &&
        previous.activeFile === nextState.activeFile &&
        previous.visibleSignature === nextState.visibleSignature
    ) {
        return null;
    }

    if (previous && previous.visibleSignature === nextState.visibleSignature) {
        return {
            command: { kind: 'focus', args: ['focus', input.activeFile] },
            nextState,
        };
    }

    return {
        command: {
            kind: 'sync',
            args: ['sync', '--col', visibleMd.join(','), '--focus', input.activeFile, '--no-autostart'],
        },
        nextState,
    };
}
