import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { explorerRows, RepoContextRealtimeView, type ExplorerViewFrame, type ExplorerViewTemplate } from './repoContextRealtimeView.js';

test('shared View resumes an Explorer submit without authorizing it twice', async context => {
    const runtimeName = '@lazily-hub/lazily-js/realtime-view';
    const runtime = await import(runtimeName).catch((error: { code?: string }) => {
        if (error.code !== 'ERR_PACKAGE_PATH_NOT_EXPORTED') throw error;
        return undefined;
    });
    if (!runtime) {
        context.skip('requires the Lazily realtime View candidate package');
        return;
    }
    const template = JSON.parse(readFileSync(new URL('../media/repoContextView.json', import.meta.url), 'utf8')) as ExplorerViewTemplate;
    const view = new RepoContextRealtimeView(runtime, template);
    const rows = explorerRows('results', [
        { file: 'first.md', line: 1, endLine: 2, name: 'first', kind: 'heading' },
        { file: 'second.md', line: 4, endLine: 4, name: 'second', kind: 'heading' },
    ]);
    const available = { selection: true, session: true, answer: false };
    const related = explorerRows('related', [
        { file: 'ref.md', line: 5, endLine: 5, name: 'reference', relation: 'Links to' },
    ]);
    const initial = view.publish(rows, [], available) as Array<{ type: string; revision?: { view_instance: string } }>;
    assert.deepEqual(initial.map(message => message.type), ['view', 'view-values']);
    const frame: ExplorerViewFrame = {
        version: 1, view_instance: initial[0].revision!.view_instance, displayed_revision: 1,
        node_id: 'submit-question', event_id: 'question-1', event: 'submit', payload: 'Draft',
    };

    let runs = 0;
    const submit = (event: ExplorerViewFrame) => {
        const result = view.authorize(event, action => action === 'ask');
        if (result.status === 'accepted') runs++;
        return result.status;
    };
    assert.equal(view.authorize(frame, () => false).status, 'denied');
    assert.equal(submit(frame), 'accepted');

    const [first, second] = rows;
    const update = view.publish([second, first], related, available) as Array<{ type: string; relatedCount?: number }>;
    assert.deepEqual(update.map(message => message.type), ['view-delta', 'view', 'view-values']);
    assert.equal(update[2].relatedCount, 1);
    assert.equal(submit({ ...frame, event_id: 'question-stale' }), 'stale');
    const [attach] = view.publish([second, first], related, available, true) as Array<{ snapshot?: unknown }>;
    assert.ok(attach.snapshot);
    assert.equal(submit(frame), 'duplicate');
    assert.equal(runs, 1);
});
