import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    buildBusySessionClearBlockedMessage,
    buildRouteFailurePresentation,
    buildSessionCommandArgs,
    buildSessionStatusPresentation,
    buildSessionSuccessHint,
    parseBusySessionClearRefusal,
    sessionStatusShowsIdleDirectPane,
} from './sessionUi';

describe('sessionUi', () => {
    it('builds session command args for clear context routing', () => {
        assert.deepStrictEqual(
            buildSessionCommandArgs('clear', 'tasks/agent-doc/agent-doc-bugs2.md'),
            ['session', 'clear', 'tasks/agent-doc/agent-doc-bugs2.md'],
        );
    });

    it('builds session command args for explicit interrupt clear routing', () => {
        assert.deepStrictEqual(
            buildSessionCommandArgs('interrupt-clear', 'tasks/agent-doc/agent-doc-bugs2.md'),
            ['session', 'interrupt-clear', 'tasks/agent-doc/agent-doc-bugs2.md'],
        );
    });

    it('builds session command args for supervisor restart routing', () => {
        assert.deepStrictEqual(
            buildSessionCommandArgs('restart-supervisor', 'tasks/agent-doc/agent-doc-bugs2.md'),
            ['session', 'restart-supervisor', 'tasks/agent-doc/agent-doc-bugs2.md'],
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

    it('parses protected busy clear refusals for typed operator UX', () => {
        const output =
            'agent-doc command failed (exit 1): Error: session_clear refused for /repo/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="Tip says \\"Override\\" settings (per agent)"). Run `agent-doc session status /repo/tasks/root.md` and wait for an idle prompt before retrying `agent-doc session clear`, or run `agent-doc session interrupt-clear /repo/tasks/root.md` to intentionally interrupt the pane and clear context.';

        const refusal = parseBusySessionClearRefusal(output);

        assert.ok(refusal);
        assert.strictEqual(refusal.file, '/repo/tasks/root.md');
        assert.strictEqual(refusal.pane, '%2');
        assert.strictEqual(refusal.source, 'authoritative_actor');
        assert.strictEqual(refusal.currentCommand, 'agent-doc');
        assert.strictEqual(refusal.tail, 'Tip says "Override" settings (per agent)');
    });

    it('builds busy clear warning with refresh and interrupt actions named', () => {
        const message = buildBusySessionClearBlockedMessage(
            'tasks/root.md',
            {
                file: '/repo/tasks/root.md',
                pane: '%2',
                source: 'authoritative_actor',
                currentCommand: 'agent-doc',
                tail: 'gpt-5.5 high - Context 59% used',
            },
        );

        assert.match(message, /Session is still running/);
        assert.match(message, /Pane %2 is busy \(agent-doc\)/);
        assert.match(message, /Refresh and retry/);
        assert.match(message, /Interrupt and clear/);
    });

    it('detects only idle direct pane status as refresh-retry eligible', () => {
        const output = [
            'document: /repo/tasks/root.md',
            'actor: generation=41 pane=%2 window=@1 state=busy',
            'live_pane: state=alive-idle pane=%2 source=authoritative_actor current_command=agent-doc prompt_ready=true tail=>',
            'supervisor: health=healthy state=healthy actor_state=busy restart_count=0 socket=/tmp/sup.sock',
            'controller_lease: generation=41 pid=100 runtime_state=busy heartbeat=2026-05-12T00:00:00Z socket=/tmp/sup.sock',
        ].join('\n');

        assert.strictEqual(sessionStatusShowsIdleDirectPane(output), true);
        assert.strictEqual(sessionStatusShowsIdleDirectPane(output.replace('alive-idle', 'alive-busy')), false);
    });

    it('falls back to the supervisor restart success hint when the CLI returns no text', () => {
        assert.strictEqual(
            buildSessionSuccessHint('restart-supervisor', 'tasks/agent-doc/agent-doc-bugs2.md', ''),
            'Restart requested for supervisor handling tasks/agent-doc/agent-doc-bugs2.md',
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
