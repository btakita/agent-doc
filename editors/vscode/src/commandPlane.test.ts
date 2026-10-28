import assert from 'node:assert/strict';
import test from 'node:test';

import {
    buildEditorRoutePayload,
    buildEditorRouteCommandMessage,
    buildEditorRouteSuccessHint,
    isEditorRouteSteeringDelivered,
    resolveEditorRouteTerminal,
} from './commandPlane.js';

test('buildEditorRouteCommandMessage produces an agent-doc.editor_route.v1 CommandSubmit', () => {
    const { commandId, message } = buildEditorRouteCommandMessage('plan.md', 'root:plan.md:run', [], 120, 'cmd-fixed');
    assert.equal(commandId, 'cmd-fixed');
    const submit: any = message.CommandSubmit;
    assert.equal(submit.namespace, 'agent-doc');
    assert.equal(submit.name, 'editor_route');
    assert.equal(submit.payload_type, 'agent-doc.editor_route.v1');
    assert.equal(submit.command_id, 'cmd-fixed');
    assert.equal(submit.causation_id, 'cmd-fixed');
    assert.equal(submit.idempotency_key, 'root:plan.md:run');
    assert.equal(submit.policy.dedupe, 'same_idempotency_key');
    assert.ok(String(submit.payload_hash).startsWith('sha256:'));
    assert.deepEqual(submit.required_features, ['causal-receipts', 'command-events']);
});

test('inline payload round-trips to the editor_route payload the controller consumes', () => {
    const { message } = buildEditorRouteCommandMessage('plan.md', 'root:plan.md:run', ['-h'], 120, 'cmd-1');
    const bytes: number[] = (message.CommandSubmit as any).payload.Inline;
    const payload = JSON.parse(Buffer.from(bytes).toString('utf8'));
    assert.equal(payload.relative_path, 'plan.md');
    assert.equal(payload.dispatch_only, true);
    assert.equal(payload.plain_trigger, true);
    assert.equal(payload.route_key, 'root:plan.md:run');
    assert.deepEqual(payload.layout_args, ['-h']);
    assert.equal(payload.wait_for_ready_secs, 120);
    // buildEditorRoutePayload is the single source of that shape.
    assert.deepEqual(payload, buildEditorRoutePayload('plan.md', 'root:plan.md:run', ['-h'], 120));
});

test('resolveEditorRouteTerminal returns output only on an applied terminal projection', () => {
    const data = {
        output: 'routed ok',
        projection: {
            generation: 0,
            commands: [
                { command_id: 'cmd-1', status: 'applied', terminal: true, generation: 0, reason: null, terminal_receipt_id: 'cmd-1-receipt', last_event_id: 'cmd-1-started' },
            ],
        },
    };
    assert.equal(resolveEditorRouteTerminal(data, 'cmd-1'), 'routed ok');
});

test('resolveEditorRouteTerminal throws on a non-terminal projection (accepted/queued never resolves)', () => {
    const data = {
        output: '',
        projection: {
            generation: 0,
            commands: [
                { command_id: 'cmd-1', status: 'running', terminal: false, generation: 0, reason: null, terminal_receipt_id: null, last_event_id: 'cmd-1-started' },
            ],
        },
    };
    assert.throws(() => resolveEditorRouteTerminal(data, 'cmd-1'), /non-terminal projection/);
});

test('resolveEditorRouteTerminal throws on a rejected terminal', () => {
    const data = {
        output: 'boom',
        projection: {
            generation: 0,
            commands: [
                { command_id: 'cmd-1', status: 'rejected', terminal: true, generation: 0, reason: 'editor_route exit_code=1', terminal_receipt_id: 'cmd-1-receipt', last_event_id: null },
            ],
        },
    };
    assert.throws(() => resolveEditorRouteTerminal(data, 'cmd-1'), /boom/);
});

test('explicit steering delivered behind the owner is presented as concise success', () => {
    const output = 'Error: [route] Sent 2 updates to the agent already running for /repo/tasks/sampleapp/notes.md; '
        + 'No second trigger was injected. steering_delivered=2 sent=explicit '
        + 'ui_outcome_contract=ui-outcome-v1 ui_outcome=steering_delivered_to_owner '
        + 'ui_outcome_class=ok next_action=owner_turn_handles_steering';
    assert.equal(isEditorRouteSteeringDelivered(output), true);
    const hint = buildEditorRouteSuccessHint('tasks/sampleapp/notes.md', output);
    assert.equal(
        hint,
        'Sent 2 updates to the Agent Doc turn already running for tasks/sampleapp/notes.md; no second turn was started.',
    );
    assert.equal(hint.includes('No second trigger was injected'), false);
    assert.equal(hint.startsWith('Error'), false);
    assert.equal(
        buildEditorRouteSuccessHint(
            'tasks/sampleapp/notes.md',
            'steering_delivered=1 ui_outcome=steering_delivered_to_owner',
        ),
        'Sent 1 update to the Agent Doc turn already running for tasks/sampleapp/notes.md; no second turn was started.',
    );
});
