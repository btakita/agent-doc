import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    buildRouteFailurePresentation,
    buildSessionCommandArgs,
    buildSessionStatusPresentation,
    buildSessionSuccessHint,
} from './sessionUi';

describe('sessionUi', () => {
    it('builds session command args for clear context routing', () => {
        assert.deepStrictEqual(
            buildSessionCommandArgs('clear', 'tasks/agent-doc/agent-doc-bugs2.md'),
            ['session', 'clear', 'tasks/agent-doc/agent-doc-bugs2.md'],
        );
    });

    it('keeps exact session status output in the diagnostics surface', () => {
        const output = 'generation=4\nstate=waiting_input\npane=%12';
        assert.deepStrictEqual(
            buildSessionStatusPresentation('tasks/agent-doc/agent-doc-bugs2.md', output),
            {
                title: 'Session status: tasks/agent-doc/agent-doc-bugs2.md',
                body: output,
                hint: 'Session status: tasks/agent-doc/agent-doc-bugs2.md',
            },
        );
    });

    it('falls back to the clear-session success hint when the CLI returns no text', () => {
        assert.strictEqual(
            buildSessionSuccessHint('clear', 'tasks/agent-doc/agent-doc-bugs2.md', ''),
            'Cleared session context for tasks/agent-doc/agent-doc-bugs2.md',
        );
    });

    it('preserves stage-specific dispatch failures in the persistent route surface', () => {
        const output =
            'authoritative actor for tasks/agent-doc/agent-doc-bugs2.md rejected routed trigger in pane %12: the authoritative actor is busy';
        assert.deepStrictEqual(
            buildRouteFailurePresentation('tasks/agent-doc/agent-doc-bugs2.md', output),
            {
                title: 'Route failure: tasks/agent-doc/agent-doc-bugs2.md',
                body: output,
                toast: `route failed: ${output}`,
            },
        );
    });
});
