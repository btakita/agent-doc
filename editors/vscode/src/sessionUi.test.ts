import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    buildForcedRestartSupervisorCommandArgs,
    buildRouteFailurePresentation,
    buildSessionCommandArgs,
    buildSessionStatusPresentation,
    buildSessionSuccessHint,
    buildTurnStatePresentation,
    buildDeferredClearSupersededMessage,
    DeferredClearSupersededNotices,
    TurnProjection,
    sessionStatusShowsIdleDirectPane,
} from './sessionUi.js';

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

    it('builds session command args for agent restart routing', () => {
        assert.deepStrictEqual(
            buildSessionCommandArgs('restart-agent', 'tasks/agent-doc/agent-doc-bugs2.md'),
            ['session', 'restart-agent', 'tasks/agent-doc/agent-doc-bugs2.md'],
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

    it('falls back to the agent restart success hint when the CLI returns no text', () => {
        assert.strictEqual(
            buildSessionSuccessHint('restart-agent', 'tasks/agent-doc/agent-doc-bugs2.md', ''),
            'Restart requested for agent handling tasks/agent-doc/agent-doc-bugs2.md',
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

describe('buildTurnStatePresentation (Project Controller turn-state coordination)', () => {
    it('shows the running task as one truncated line with the full text in the tooltip', () => {
        const task = 'do [#jbunloadsig]: ' + 'x'.repeat(80);
        const presentation = buildTurnStatePresentation({
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
            task,
        });
        const shown = presentation.label.split(' · ')[1];
        assert.strictEqual(presentation.label.split(' · ')[0], '⟳ agent-doc: awaiting response');
        assert.strictEqual(Array.from(shown).length, 60);
        assert.ok(shown.startsWith('do [#jbunloadsig]: x'));
        assert.ok(shown.endsWith('…'));
        assert.strictEqual(presentation.tooltip, `Task: ${task}`);
        assert.strictEqual(
            buildTurnStatePresentation({
                state: 'awaiting_response',
                turn_in_flight: true,
                transition_authority: 'project_controller',
                task: 'do [#a]',
            }).label,
            '⟳ agent-doc: awaiting response · do [#a]',
        );
    });

    it('is empty + ungated when idle or no projection', () => {
        assert.deepStrictEqual(buildTurnStatePresentation(null), {
            label: '',
            guardPromptForwarding: false,
        });
        const idle: TurnProjection = {
            state: 'idle',
            turn_in_flight: false,
            transition_authority: 'project_controller',
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
            transition_authority: 'project_controller',
        };
        const a = buildTurnStatePresentation(awaiting);
        assert.ok(a.label.includes('awaiting response'));
        assert.strictEqual(a.guardPromptForwarding, true);

        const persisting: TurnProjection = {
            state: 'persisting',
            turn_in_flight: true,
            transition_authority: 'project_controller',
        };
        const p = buildTurnStatePresentation(persisting);
        assert.ok(p.label.includes('persisting'));
        assert.strictEqual(p.guardPromptForwarding, true);
    });

    it('projects realtime steering onto the turn banner label', () => {
        const deleted: TurnProjection = {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
            realtime_steering: {
                state: 'prompt_deleted',
                count: 2,
                preview: 'removed prompt',
                verbatim: 'first removal\n\nsecond removal',
            },
        };

        const presentation = buildTurnStatePresentation(deleted);
        assert.strictEqual(
            presentation.label,
            '⟳ agent-doc: awaiting response · prompt deleted (2 edits)',
        );
        assert.strictEqual(presentation.tooltip, 'first removal\n\nsecond removal');
        assert.strictEqual(presentation.guardPromptForwarding, true);
    });

    it('projects merge conflicts without requesting another turn', () => {
        const projection: TurnProjection = {
            state: 'idle',
            turn_in_flight: false,
            transition_authority: 'project_controller',
            semantic_merge_conflicts: [{
                component: 'exchange',
                id: 'node-1',
                reason: 'same_node_operator_override',
                detail: 'operator value won',
            }],
        };

        const presentation = buildTurnStatePresentation(projection);
        assert.strictEqual(presentation.label, 'agent-doc: ⚠ merge conflict');
        assert.strictEqual(presentation.tooltip, 'exchange:node-1 — operator value won');
        assert.strictEqual(presentation.guardPromptForwarding, false);
    });
});

describe('DeferredClearSupersededNotices (#clearsupersedenotice)', () => {
    const doc = '/repo/tasks/root.md';
    const dropped = (epoch: number) => ({ clear_epoch: epoch, pane: '%149', reason: 'newer_prompt' });

    it('seeds the baseline from the first projection without announcing an old drop', () => {
        const notices = new DeferredClearSupersededNotices();
        assert.strictEqual(notices.observe(doc, dropped(3)), false);
        assert.strictEqual(notices.observe(doc, dropped(3)), false);
        assert.strictEqual(notices.observe(doc, undefined), false);
        assert.strictEqual(notices.observe(doc, dropped(4)), true);
        assert.strictEqual(notices.observe(doc, dropped(4)), false);
    });

    it('announces a drop after an empty baseline exactly once per epoch and per document', () => {
        const notices = new DeferredClearSupersededNotices();
        assert.strictEqual(notices.observe(doc, null), false);
        assert.strictEqual(notices.observe(doc, dropped(1)), true);
        assert.strictEqual(notices.observe(doc, dropped(1)), false);
        assert.strictEqual(notices.observe(doc, null), false);
        assert.strictEqual(notices.observe(doc, dropped(2)), true);
        assert.strictEqual(notices.observe('/repo/tasks/other.md', dropped(2)), false);
        notices.forget(doc);
        assert.strictEqual(notices.observe(doc, dropped(5)), false);
    });

    it('reads the field from the turn projection and names document, pane and the retry', () => {
        const projection: TurnProjection = {
            state: 'awaiting_response',
            turn_in_flight: true,
            transition_authority: 'project_controller',
            deferred_clear_superseded: { ...dropped(7), superseded_at: 1040 },
        };
        const message = buildDeferredClearSupersededMessage(
            'tasks/root.md',
            projection.deferred_clear_superseded!,
        );
        assert.ok(message.startsWith('Queued Clear Session Context was not sent for tasks/root.md.\n'));
        assert.ok(message.includes('A newer prompt started in pane %149'));
        assert.ok(message.includes('Press Clear Session Context again'));
        assert.ok(
            buildDeferredClearSupersededMessage('tasks/root.md', {
                clear_epoch: 8,
                pane: '%2',
                reason: 'session_restarted',
            }).includes('reason=session_restarted'),
        );
    });
});
