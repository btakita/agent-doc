import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    glyphIconDataUri,
    parseQueueTreeJson,
    planQueueTreeAutoFolds,
    queueTreeFolds,
    queueTreeTooltip,
} from './queueTreeGutter.js';

// `#queuetreegutter`: the shape `agent_doc_queue_tree_json` returns.
const TREE = JSON.stringify({
    summary: { items: 3, counts: [] },
    nodes: [
        { line: 2, end_line: 3, depth: 0, queue: 'main', state: 'done', glyph: '✓', text: 'Ship alpha', id: 'q-alpha', closed: true, waiting_item: false, tooltip: 'done' },
        { line: 3, end_line: 3, depth: 1, queue: 'main', state: 'done', glyph: '✓', text: 'step one', closed: true, waiting_item: false, tooltip: 'done' },
        { line: 4, end_line: 6, depth: 0, queue: 'main', state: 'waiting-on-you', glyph: '🙋', text: 'Approve', closed: false, waiting_item: true, rollup: { you: 1, agent: 0, done: 0 }, tooltip: 'waiting on you · you 1 · agent 0 · done 0' },
    ],
});

describe('queue tree gutter (#queuetreegutter)', () => {
    it('parses native nodes and words tooltips from the binary', () => {
        const nodes = parseQueueTreeJson(TREE)!;
        assert.strictEqual(nodes.length, 3);
        assert.strictEqual(queueTreeTooltip(nodes[2]), 'agent-doc queue: waiting on you · you 1 · agent 0 · done 0');
        // An older library without `tooltip` falls back to the state label.
        assert.strictEqual(
            queueTreeTooltip({ ...nodes[2], tooltip: undefined, state: 'needs-integration' }),
            'agent-doc queue: needs integration',
        );
        assert.strictEqual(parseQueueTreeJson('not json'), null);
        assert.strictEqual(parseQueueTreeJson('{}'), null);
    });

    it('folds only closed nodes with a subtree, as 0-based lines', () => {
        const folds = queueTreeFolds(parseQueueTreeJson(TREE)!);
        assert.deepStrictEqual(folds.map((fold) => [fold.start, fold.end]), [[1, 2]]);
    });

    it('collapses each closed subtree once and never under a cursor', () => {
        const folds = queueTreeFolds(parseQueueTreeJson(TREE)!);
        const first = planQueueTreeAutoFolds(folds, new Set(), [0]);
        assert.deepStrictEqual(first.collapse, folds);
        const again = planQueueTreeAutoFolds(folds, first.remembered, [0]);
        assert.deepStrictEqual(again.collapse, [], 'an operator who reopened it keeps it open');
        const underCursor = planQueueTreeAutoFolds(folds, new Set(), [2]);
        assert.deepStrictEqual(underCursor.collapse, []);
        assert.strictEqual(underCursor.remembered.size, 0, 'deferred, not forgotten');
        const reopened = planQueueTreeAutoFolds([], first.remembered, []);
        assert.strictEqual(reopened.remembered.size, 0, 'a reopened row folds again when it closes');
    });

    it('paints a glyph as an SVG data URI', () => {
        const uri = glyphIconDataUri('<✓>');
        assert.ok(uri.startsWith('data:image/svg+xml;utf8,'));
        assert.ok(decodeURIComponent(uri).includes('&lt;✓&gt;'));
    });
});
