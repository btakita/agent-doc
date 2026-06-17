import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    EditorCommandCompletion,
    EditorCommandDecision,
    EditorCommandKind,
    createIdleEditorCommandState,
    onEditorCommandComplete,
    onEditorCommandRequest,
} from './editorCommandState';

describe('editorCommandState', () => {
    it('starts requested command for an idle document', () => {
        const result = onEditorCommandRequest(
            createIdleEditorCommandState(),
            EditorCommandKind.RunAgentDoc,
        );

        assert.strictEqual(result.decision, EditorCommandDecision.StartNow);
        assert.strictEqual(result.state.active, EditorCommandKind.RunAgentDoc);
    });

    it('dedupes duplicate run while a route is active', () => {
        const state = {
            active: EditorCommandKind.RunAgentDoc,
            queuedRunAfterClear: false,
        };

        const result = onEditorCommandRequest(state, EditorCommandKind.RunAgentDoc);

        assert.strictEqual(result.decision, EditorCommandDecision.DedupeActiveRun);
        assert.strictEqual(result.state, state);
    });

    it('lets normal clear preempt an active run dispatch', () => {
        const result = onEditorCommandRequest(
            {
                active: EditorCommandKind.RunAgentDoc,
                queuedRunAfterClear: false,
            },
            EditorCommandKind.ClearSessionContext,
        );

        assert.strictEqual(result.decision, EditorCommandDecision.PreemptRunWithClear);
        assert.strictEqual(result.state.active, EditorCommandKind.ClearSessionContext);
        assert.strictEqual(result.state.queuedRunAfterClear, false);
    });

    it('ignores run completion after preempting clear takes ownership', () => {
        const state = {
            active: EditorCommandKind.ClearSessionContext,
            queuedRunAfterClear: false,
        };

        const result = onEditorCommandComplete(state, EditorCommandKind.RunAgentDoc);

        assert.strictEqual(result.completion, EditorCommandCompletion.Ignored);
        assert.strictEqual(result.state, state);
    });

    it('queues run behind active clear and starts it after clear completes', () => {
        const queued = onEditorCommandRequest(
            {
                active: EditorCommandKind.ClearSessionContext,
                queuedRunAfterClear: false,
            },
            EditorCommandKind.RunAgentDoc,
        );

        assert.strictEqual(queued.decision, EditorCommandDecision.QueueRunAfterClear);
        assert.deepStrictEqual(queued.state, {
            active: EditorCommandKind.ClearSessionContext,
            queuedRunAfterClear: true,
        });

        const completed = onEditorCommandComplete(
            queued.state,
            EditorCommandKind.ClearSessionContext,
        );

        assert.strictEqual(completed.completion, EditorCommandCompletion.StartQueuedRun);
        assert.deepStrictEqual(completed.state, {
            active: EditorCommandKind.RunAgentDoc,
            queuedRunAfterClear: false,
        });
    });

    it('does not mutate active state on wrong completion', () => {
        const state = {
            active: EditorCommandKind.RunAgentDoc,
            queuedRunAfterClear: false,
        };

        const result = onEditorCommandComplete(state, EditorCommandKind.ClearSessionContext);

        assert.strictEqual(result.completion, EditorCommandCompletion.Ignored);
        assert.strictEqual(result.state, state);
    });
});
