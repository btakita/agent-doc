export enum EditorCommandKind {
    RunAgentDoc = 'run-agent-doc',
    ClearSessionContext = 'clear-session-context',
}

export enum EditorCommandDecision {
    StartNow = 'start-now',
    DedupeActiveRun = 'dedupe-active-run',
    DedupeActiveClear = 'dedupe-active-clear',
    QueueRunAfterClear = 'queue-run-after-clear',
    PreemptRunWithClear = 'preempt-run-with-clear',
    Ignored = 'ignored',
}

export enum EditorCommandCompletion {
    Idle = 'idle',
    StartQueuedRun = 'start-queued-run',
    Ignored = 'ignored',
}

export interface EditorCommandState {
    active?: EditorCommandKind;
    queuedRunAfterClear: boolean;
}

export function createIdleEditorCommandState(): EditorCommandState {
    return { queuedRunAfterClear: false };
}

export function onEditorCommandRequest(
    state: EditorCommandState,
    requested: EditorCommandKind,
): { state: EditorCommandState; decision: EditorCommandDecision } {
    const active = state.active;
    if (!active) {
        return {
            state: { active: requested, queuedRunAfterClear: false },
            decision: EditorCommandDecision.StartNow,
        };
    }

    if (active === requested) {
        return {
            state,
            decision: requested === EditorCommandKind.RunAgentDoc
                ? EditorCommandDecision.DedupeActiveRun
                : EditorCommandDecision.DedupeActiveClear,
        };
    }

    if (
        active === EditorCommandKind.ClearSessionContext &&
        requested === EditorCommandKind.RunAgentDoc
    ) {
        return {
            state: { ...state, queuedRunAfterClear: true },
            decision: EditorCommandDecision.QueueRunAfterClear,
        };
    }

    if (
        active === EditorCommandKind.RunAgentDoc &&
        requested === EditorCommandKind.ClearSessionContext
    ) {
        return {
            state: {
                active: EditorCommandKind.ClearSessionContext,
                queuedRunAfterClear: false,
            },
            decision: EditorCommandDecision.PreemptRunWithClear,
        };
    }

    return { state, decision: EditorCommandDecision.Ignored };
}

export function onEditorCommandComplete(
    state: EditorCommandState,
    completed: EditorCommandKind,
): { state: EditorCommandState; completion: EditorCommandCompletion } {
    if (state.active !== completed) {
        return { state, completion: EditorCommandCompletion.Ignored };
    }

    if (
        completed === EditorCommandKind.ClearSessionContext &&
        state.queuedRunAfterClear
    ) {
        return {
            state: {
                active: EditorCommandKind.RunAgentDoc,
                queuedRunAfterClear: false,
            },
            completion: EditorCommandCompletion.StartQueuedRun,
        };
    }

    return {
        state: createIdleEditorCommandState(),
        completion: EditorCommandCompletion.Idle,
    };
}

export class EditorCommandRegistry {
    private readonly states = new Map<string, EditorCommandState>();

    request(routeKey: string, requested: EditorCommandKind): EditorCommandDecision {
        const current = this.states.get(routeKey) ?? createIdleEditorCommandState();
        const result = onEditorCommandRequest(current, requested);
        this.store(routeKey, result.state);
        return result.decision;
    }

    complete(routeKey: string, completed: EditorCommandKind): EditorCommandCompletion {
        const current = this.states.get(routeKey);
        if (!current) return EditorCommandCompletion.Ignored;
        const result = onEditorCommandComplete(current, completed);
        this.store(routeKey, result.state);
        return result.completion;
    }

    resetForTest(): void {
        this.states.clear();
    }

    private store(routeKey: string, state: EditorCommandState): void {
        if (!state.active) {
            this.states.delete(routeKey);
            return;
        }
        this.states.set(routeKey, state);
    }
}
