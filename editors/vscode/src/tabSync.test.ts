import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';
import {
    buildEditorSurface,
    buildSyncCommandArgs,
    flattenVisibleColumns,
    formatSyncHint,
    intentFromReceipt,
    isPreservedLayoutOutput,
    normalizeVisibleColumns,
    syncHintFromReceipt,
} from './tabSync.js';
import { fileURLToPath } from 'node:url';

// ESM has no `__dirname`; derive it from the module URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

describe('editor surface reporting wiring', () => {
    const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');

    it('reports one observation per tab change instead of planning a command', () => {
        const start = source.indexOf('function reportCurrentSurface');
        assert.ok(start >= 0, 'extension.ts must define reportCurrentSurface');
        const body = source.slice(start, source.indexOf('function forgetObservedSurfaces', start));
        assert.ok(body.includes('native.editorSurfaceObserveJson({'));

        const tabChangedStart = source.indexOf('function onTabChanged');
        assert.ok(tabChangedStart >= 0, 'extension.ts must define onTabChanged');
        const tabChangedBody = source.slice(tabChangedStart, tabChangedStart + 400);
        assert.ok(tabChangedBody.includes('requestSurfaceObservation()'));
    });

    it('no longer chooses between focus and sync itself', () => {
        assert.ok(
            !source.includes('buildTabChangeCommand'),
            'the focus-vs-sync plan belongs to the surface graph, not the extension',
        );
        assert.ok(
            !source.includes('focusExistingPaneForActiveEditor'),
            'the immediate focus handoff is now a derived Focus intent',
        );
        assert.ok(
            !source.includes('registerTabSyncDeferredRetry'),
            'the preserved-layout retry ladder belongs to the surface graph',
        );
    });

    it('releases each observed surface graph on deactivate', () => {
        const start = source.indexOf('export function deactivate');
        assert.ok(start >= 0);
        assert.ok(source.slice(start).includes('forgetObservedSurfaces()'));
        assert.ok(source.includes('native.editorSurfaceForget(root)'));
    });

    it('reports absolute document paths so a derived focus can address them', () => {
        const start = source.indexOf('function captureCurrentSurface');
        assert.ok(start >= 0, 'extension.ts must define captureCurrentSurface');
        const body = source.slice(start, source.indexOf('function requestSurfaceObservation', start));
        assert.ok(body.includes('absolutizeColumns(root, collectVisibleMarkdownColumns(root))'));
        assert.ok(body.includes('activeFile: activeFsPath'));
    });
});

describe('buildEditorSurface', () => {
    it('reports the focused document, the visible set, and the split layout', () => {
        const surface = buildEditorSurface({
            activeFile: '/repo/tasks/b.md',
            visibleMd: ['/repo/tasks/a.md', '/repo/tasks/b.md'],
            visibleColumns: [['/repo/tasks/a.md'], ['/repo/tasks/b.md']],
        });

        assert.deepStrictEqual(surface, {
            focused: '/repo/tasks/b.md',
            visible: ['/repo/tasks/a.md', '/repo/tasks/b.md'],
            columns: [{ files: ['/repo/tasks/a.md'] }, { files: ['/repo/tasks/b.md'] }],
            force_reconcile: false,
        });
    });

    it('reports an unchanged surface rather than deciding it is a no-op', () => {
        const input = {
            activeFile: '/repo/tasks/a.md',
            visibleMd: ['/repo/tasks/a.md'],
            visibleColumns: [['/repo/tasks/a.md']],
        };

        // No `previous` parameter exists: dedup is the graph's, and an identical
        // observation costs nothing there.
        assert.deepStrictEqual(buildEditorSurface(input), buildEditorSurface(input));
        assert.notStrictEqual(buildEditorSurface(input), null);
    });

    it('reports no columns when the editor detected no layout', () => {
        const surface = buildEditorSurface({
            activeFile: '/repo/tasks/a.md',
            visibleMd: ['/repo/tasks/a.md', '/repo/tasks/b.md'],
        });

        assert.deepStrictEqual(surface?.columns, []);
        assert.deepStrictEqual(surface?.visible, ['/repo/tasks/a.md', '/repo/tasks/b.md']);
    });

    it('drops blank and duplicate entries and empty columns', () => {
        const surface = buildEditorSurface({
            activeFile: '/repo/tasks/a.md',
            visibleMd: [],
            visibleColumns: [['/repo/tasks/a.md', '', '/repo/tasks/a.md'], ['', '']],
        });

        assert.deepStrictEqual(surface?.columns, [{ files: ['/repo/tasks/a.md'] }]);
        assert.deepStrictEqual(surface?.visible, ['/repo/tasks/a.md']);
    });

    it('carries force_reconcile through in the shape the graph reads', () => {
        const forced = buildEditorSurface({
            activeFile: '/repo/a.md',
            visibleMd: ['/repo/a.md'],
            forceReconcile: true,
        });
        assert.strictEqual(forced?.force_reconcile, true);
    });

    it('returns null only when there is nothing visible to observe', () => {
        assert.strictEqual(
            buildEditorSurface({ activeFile: '/repo/a.md', visibleMd: [], visibleColumns: [] }),
            null,
        );
    });

    it('reports no layout_synced field for the controller to answer', () => {
        const surface = buildEditorSurface({ activeFile: '/repo/a.md', visibleMd: ['/repo/a.md'] });
        assert.ok(surface !== null);
        assert.ok(!('layout_synced' in surface!));
    });
});

describe('surface receipts', () => {
    it('turns a derived sync intent into the operator hint', () => {
        const receipt = JSON.stringify({
            intent: {
                kind: 'sync',
                columns: [{ files: ['/repo/a.md'] }, { files: ['/repo/b.md'] }],
                document: '/repo/b.md',
            },
            idle: false,
            outcome: '{}',
            error: null,
        });

        assert.strictEqual(
            syncHintFromReceipt(receipt),
            'Sync: --col /repo/a.md --col /repo/b.md [focus: /repo/b.md]',
        );
        assert.strictEqual(intentFromReceipt(receipt)?.kind, 'sync');
    });

    it('gives focus and idle intents no hint', () => {
        assert.strictEqual(
            syncHintFromReceipt(JSON.stringify({ intent: { kind: 'focus', document: '/repo/b.md' } })),
            null,
        );
        assert.strictEqual(syncHintFromReceipt(JSON.stringify({ intent: { kind: 'idle' } })), null);
    });

    it('reports an unusable receipt as no intent instead of throwing', () => {
        assert.strictEqual(intentFromReceipt(null), null);
        assert.strictEqual(intentFromReceipt(''), null);
        assert.strictEqual(intentFromReceipt('not json'), null);
        assert.strictEqual(intentFromReceipt('{}'), null);
        assert.strictEqual(syncHintFromReceipt('not json'), null);
    });

    it('formats a hint from columns directly', () => {
        assert.strictEqual(
            formatSyncHint([{ files: ['/repo/a.md', '/repo/c.md'] }], '/repo/a.md'),
            'Sync: --col /repo/a.md,/repo/c.md [focus: /repo/a.md]',
        );
    });
});

describe('manual sync layout arguments', () => {
    it('keeps split placeholders so non-markdown side panes do not collapse columns', () => {
        assert.deepStrictEqual(
            buildSyncCommandArgs([['a.md'], []], 'a.md', { exactVisible: true }),
            ['sync', '--col', 'a.md', '--col', '', '--focus', 'a.md', '--exact-visible', '--no-autostart'],
        );
    });

    it('falls back to the active file when no columns were detected', () => {
        assert.deepStrictEqual(
            buildSyncCommandArgs([], 'a.md', { noAutostart: false }),
            ['sync', '--col', 'a.md', '--focus', 'a.md'],
        );
    });

    it('normalizes and flattens visible columns', () => {
        assert.deepStrictEqual(
            normalizeVisibleColumns([['b.md', '', 'b.md'], ['a.md']]),
            [['b.md'], ['a.md']],
        );
        assert.deepStrictEqual(flattenVisibleColumns([['b.md'], ['a.md', '']]), ['a.md', 'b.md']);
    });

    it('detects preserved-layout output', () => {
        assert.strictEqual(
            isPreservedLayoutOutput(
                '[sync] safe passive sync preserved the current tmux layout because a pane is blocked',
            ),
            true,
        );
        assert.strictEqual(isPreservedLayoutOutput('[sync] reconciled'), false);
    });
});
