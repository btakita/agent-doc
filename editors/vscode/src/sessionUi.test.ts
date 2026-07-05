import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    buildBusySessionRestartBlockedMessage,
    buildBusySessionClearBlockedMessage,
    buildForcedRestartSupervisorCommandArgs,
    buildRouteFailurePresentation,
    buildSessionCommandArgs,
    buildSessionStatusPresentation,
    buildSessionSuccessHint,
    buildStartingSessionRestartBlockedMessage,
    buildTurnStatePresentation,
    TurnProjection,
    parseBusySessionRestartRefusal,
    parseBusySessionClearRefusal,
    parseStartingSessionRestartRefusal,
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

    it('builds forced supervisor restart command args', () => {
        assert.deepStrictEqual(
            buildForcedRestartSupervisorCommandArgs('tasks/agent-doc/agent-doc-bugs2.md'),
            ['session', 'restart-supervisor', '--force', 'tasks/agent-doc/agent-doc-bugs2.md'],
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

    it('parses protected prompt input clear refusals', () => {
        const output =
            'Error: session_clear refused for /repo/tasks/root.md because pane %2 contains protected prompt input (reason=drafted prompt input, source=authoritative_actor, current_command=agent-doc, tail="› unfinished prompt"). Clear the prompt input manually, or run `agent-doc session interrupt-clear /repo/tasks/root.md` to intentionally interrupt the pane and clear context.';

        const refusal = parseBusySessionClearRefusal(output);

        assert.ok(refusal);
        assert.strictEqual(refusal.file, '/repo/tasks/root.md');
        assert.strictEqual(refusal.pane, '%2');
        assert.strictEqual(refusal.protectedReason, 'drafted prompt input');
        assert.strictEqual(refusal.tail, '› unfinished prompt');
    });

    it('builds protected prompt input warning without refresh retry guidance', () => {
        const message = buildBusySessionClearBlockedMessage(
            'tasks/root.md',
            {
                file: '/repo/tasks/root.md',
                pane: '%2',
                source: 'authoritative_actor',
                currentCommand: 'agent-doc',
                tail: '› unfinished prompt',
                protectedReason: 'drafted prompt input',
            },
        );

        assert.match(message, /protected prompt input/);
        assert.match(message, /Interrupt and clear/);
        assert.doesNotMatch(message, /Refresh and retry/);
    });

    it('parses busy restart refusals for typed operator UX', () => {
        const output =
            'agent-doc command failed (exit 1): Error: session_restart refused for /repo/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="gpt-5 high - ~/repo - Context 20% used"). Run `agent-doc session status /repo/tasks/root.md` and wait for an idle prompt, or pass `--force` to interrupt the running turn and restart anyway.';

        const refusal = parseBusySessionRestartRefusal(output);

        assert.ok(refusal);
        assert.strictEqual(refusal.file, '/repo/tasks/root.md');
        assert.strictEqual(refusal.pane, '%2');
        assert.strictEqual(refusal.source, 'authoritative_actor');
        assert.strictEqual(refusal.currentCommand, 'agent-doc');
        assert.strictEqual(refusal.tail, 'gpt-5 high - ~/repo - Context 20% used');
    });

    it('parses starting restart refusals for typed operator UX', () => {
        const output =
            'agent-doc command failed (exit 1): Error: session_restart refused for /repo/tasks/root.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /repo/tasks/root.md` to inspect the pane. Pass `--force` to interrupt the running turn and restart anyway.';

        const refusal = parseStartingSessionRestartRefusal(output);

        assert.ok(refusal);
        assert.strictEqual(refusal.file, '/repo/tasks/root.md');
        assert.strictEqual(refusal.reason, 'the document changed after the last committed cycle');
    });

    it('builds busy restart warning with interrupt action named', () => {
        const message = buildBusySessionRestartBlockedMessage(
            'tasks/root.md',
            {
                file: '/repo/tasks/root.md',
                pane: '%2',
                source: 'authoritative_actor',
                currentCommand: 'agent-doc',
                tail: 'gpt-5 high - ~/repo - Context 20% used',
            },
        );

        assert.match(message, /Restart Supervisor is blocked/);
        assert.match(message, /Pane %2 is busy \(agent-doc\)/);
        assert.match(message, /Interrupt and restart/);
    });

    it('builds starting restart warning with interrupt action named', () => {
        const message = buildStartingSessionRestartBlockedMessage(
            'tasks/root.md',
            {
                file: '/repo/tasks/root.md',
                reason: 'the document changed after the last committed cycle',
            },
        );

        assert.match(message, /Restart Supervisor is blocked/);
        assert.match(message, /authoritative actor is still starting/);
        assert.match(message, /document changed after the last committed cycle/);
        assert.match(message, /Interrupt and restart/);
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
            'Recycle requested for supervisor handling tasks/agent-doc/agent-doc-bugs2.md',
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

describe('buildTurnStatePresentation (CPC turn-state coordination)', () => {
    it('is empty + ungated when idle or no projection', () => {
        assert.deepStrictEqual(buildTurnStatePresentation(null), {
            label: '',
            guardPromptForwarding: false,
        });
        const idle: TurnProjection = {
            state: 'idle',
            turn_in_flight: false,
            transition_authority: 'cpc',
        };
        assert.deepStrictEqual(buildTurnStatePresentation(idle), {
            label: '',
            guardPromptForwarding: false,
        });
    });

    it('labels + gates prompt forwarding while a turn is in flight', () => {
        const awaiting: TurnProjection = {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'cpc',
        };
        const a = buildTurnStatePresentation(awaiting);
        assert.ok(a.label.includes('awaiting response'));
        assert.strictEqual(a.guardPromptForwarding, true);

        const persisting: TurnProjection = {
            state: 'persisting',
            turn_in_flight: true,
            transition_authority: 'cpc',
        };
        const p = buildTurnStatePresentation(persisting);
        assert.ok(p.label.includes('persisting'));
        assert.strictEqual(p.guardPromptForwarding, true);
    });

    it('projects realtime steering onto the turn banner label', () => {
        const deleted: TurnProjection = {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'cpc',
            realtime_steering: {
                state: 'prompt_deleted',
                preview: 'removed prompt',
            },
        };

        const presentation = buildTurnStatePresentation(deleted);
        assert.strictEqual(
            presentation.label,
            '⟳ agent-doc: awaiting response · prompt deleted',
        );
        assert.strictEqual(presentation.guardPromptForwarding, true);
    });
});
