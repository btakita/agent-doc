import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';
import {
    buildTreeStatusDeleteItems,
    parseQueueTreeStatusJson,
    treeStatusDeleteSpan,
} from './queueTreeStatus.js';
import { buildPrimaryPopupMenuItems } from './popupMenu.js';

const packageJson = JSON.parse(
    fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'package.json'), 'utf8'),
);

// `#treestatusquery`: the shape `agent_doc_queue_tree_status_json` returns
// (its `text` is the CLI rendering, shown verbatim).
const STATUS = JSON.stringify({
    queue: 'main',
    line: 2,
    end_line: 6,
    whole_queue: true,
    text: 'L2-6 queue main — 0 you · 1 agent · 2 deletable · 0 need review\nDeletable:\n',
    summary: { items: 3, counts: [] },
    you: [],
    agent: [{ line: 6, end_line: 6, state: 'claimed', glyph: '🚧', text: 'do [#beta]', id: 'beta', action: 'wait for the worker' }],
    deletable: [
        { line: 2, end_line: 3, state: 'done', glyph: '✓', text: 'do [#alpha]', id: 'alpha', action: 'delete the subtree' },
        { line: 4, end_line: 4, state: 'done', glyph: '✓', text: 'step', action: 'delete the row' },
        { line: 9, end_line: 12, state: 'done', glyph: '✓', text: 'past the end', action: 'delete the subtree' },
    ],
    needs_review: [],
});

describe('queue tree status (#treestatusvscode)', () => {
    it('keeps the native text verbatim and every row section', () => {
        const status = parseQueueTreeStatusJson(STATUS)!;
        assert.strictEqual(status.text, JSON.parse(STATUS).text);
        assert.strictEqual(status.whole_queue, true);
        assert.strictEqual(status.agent[0]?.id, 'beta');
        assert.strictEqual(status.deletable.length, 3);
        assert.deepStrictEqual(status.you, []);
        assert.deepStrictEqual(status.needs_review, []);
    });

    it('rejects JSON that is not a tree-status answer', () => {
        assert.strictEqual(parseQueueTreeStatusJson('not json'), null);
        assert.strictEqual(parseQueueTreeStatusJson('null'), null);
        assert.strictEqual(parseQueueTreeStatusJson('{}'), null);
        assert.strictEqual(
            parseQueueTreeStatusJson(JSON.stringify({ text: 'x', you: [], agent: [], deletable: [] })),
            null,
        );
    });

    it('drops malformed rows instead of deleting a guessed range', () => {
        const status = parseQueueTreeStatusJson(JSON.stringify({
            text: 'x',
            you: [],
            agent: [],
            needs_review: [],
            deletable: [
                { line: 0, end_line: 1, glyph: '✓', text: 'zero line' },
                { line: 3, end_line: 2, glyph: '✓', text: 'inverted' },
                { line: 3, glyph: '✓', text: 'no end' },
                { line: 3, end_line: 3, glyph: '✓', text: 'ok' },
            ],
        }))!;
        assert.deepStrictEqual(status.deletable.map((row) => row.text), ['ok']);
    });

    it('offers one delete action per deletable row the buffer still holds', () => {
        const status = parseQueueTreeStatusJson(STATUS)!;
        const items = buildTreeStatusDeleteItems(status, 8);
        assert.deepStrictEqual(items.map((item) => item.label), [
            '$(trash) Delete lines 2–3: ✓ do [#alpha]',
            '$(trash) Delete line 4: ✓ step',
        ]);
        assert.strictEqual(items[0]?.description, 'delete the subtree');
        assert.strictEqual(items[0]?.row.id, 'alpha');
    });

    it('deletes the inclusive 1-based range as whole 0-based lines', () => {
        const status = parseQueueTreeStatusJson(STATUS)!;
        assert.deepStrictEqual(treeStatusDeleteSpan(status.deletable[0]!, 8), { startLine: 1, endLine: 3 });
        assert.deepStrictEqual(treeStatusDeleteSpan(status.deletable[1]!, 8), { startLine: 3, endLine: 4 });
        assert.strictEqual(treeStatusDeleteSpan(status.deletable[2]!, 8), null);
        // Applying the span to the lines removes exactly line..=end_line.
        const lines = ['<!-- agent:queue -->', 'a', 'a1', 'b', 'c', 'd', 'e', '<!-- /agent:queue -->'];
        const span = treeStatusDeleteSpan(status.deletable[0]!, lines.length)!;
        lines.splice(span.startLine, span.endLine - span.startLine);
        assert.deepStrictEqual(lines, ['<!-- agent:queue -->', 'b', 'c', 'd', 'e', '<!-- /agent:queue -->']);
    });

    it('is a markdown command bound to Alt+Shift+S and reachable from the popup menu', () => {
        const command = packageJson.contributes.commands.find(
            (entry: { command: string }) => entry.command === 'agentDoc.treeStatus',
        );
        assert.strictEqual(command?.title, 'Agent Doc: Tree Status');
        const binding = packageJson.contributes.keybindings.find(
            (entry: { command: string }) => entry.command === 'agentDoc.treeStatus',
        );
        assert.strictEqual(binding?.key, 'alt+shift+s');
        assert.strictEqual(binding?.when, 'editorTextFocus && editorLangId == markdown');
        const paletteGate = packageJson.contributes.menus.commandPalette.find(
            (entry: { command: string }) => entry.command === 'agentDoc.treeStatus',
        );
        assert.strictEqual(paletteGate?.when, 'editorLangId == markdown');
        const item = buildPrimaryPopupMenuItems().find((entry) => entry.id === 'treeStatus');
        assert.strictEqual(item?.label, '[15] $(list-tree) Tree Status');
    });
});
