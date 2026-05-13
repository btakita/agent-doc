import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildPromptQuickPickItems, normalizePromptEntries } from './promptPolling';

describe('promptPolling', () => {
    it('normalizes flat prompt --all JSON including selected', () => {
        const entries = normalizePromptEntries([
            {
                session_id: 'abc',
                file: 'tasks/demo.md',
                cwd: '/work/demo',
                active: true,
                question: 'Permission required',
                options: [
                    { index: 1, label: 'Allow once' },
                    { index: 2, label: 'Reject' },
                ],
                selected: 1,
            },
        ]);

        assert.strictEqual(entries.length, 1);
        assert.strictEqual(entries[0].key, '/work/demo:tasks/demo.md:Permission required');
        assert.strictEqual(entries[0].cwd, '/work/demo');
        assert.strictEqual(entries[0].info.selected, 1);
        assert.deepStrictEqual(entries[0].info.options?.map(option => option.label), [
            'Allow once',
            'Reject',
        ]);
    });

    it('builds answer indices from option position instead of display index', () => {
        const items = buildPromptQuickPickItems(
            [
                { index: 4, label: 'Allow once' },
                { index: 7, label: 'Reject' },
            ],
            1,
        );

        assert.deepStrictEqual(
            items.map(item => ({
                label: item.label,
                optionIndex: item.optionIndex,
                answerIndex: item.answerIndex,
                picked: item.picked,
            })),
            [
                { label: '[4] Allow once', optionIndex: 4, answerIndex: 1, picked: false },
                { label: '[7] Reject', optionIndex: 7, answerIndex: 2, picked: true },
            ],
        );
    });
});
