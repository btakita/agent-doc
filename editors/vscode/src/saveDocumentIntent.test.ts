import * as assert from 'assert';
import { describe, it } from 'node:test';
import { processSaveDocumentIntent, type SaveDocumentHandle } from './saveDocumentIntent.js';

describe('processSaveDocumentIntent', () => {
    it('saves an open document before publishing and acknowledging its saved content', async () => {
        const events: string[] = [];
        let text = 'before save';
        const document: SaveDocumentHandle = {
            async save() {
                events.push('save');
                text = 'after save';
                return true;
            },
            getText: () => text,
        };

        const result = await processSaveDocumentIntent('/repo/session.md', {
            fileExists: () => true,
            findOpenDocument: () => document,
            publishSavedContent: (_filePath, content) => {
                events.push(`publish:${content}`);
                return true;
            },
            observeSavedContent: () => events.push('observe'),
            recordOutcome: (_filePath, status) => events.push(`record:${status}`),
            reportFailure: () => events.push('failure'),
        });

        assert.strictEqual(result, 1);
        assert.deepStrictEqual(events, [
            'save',
            'publish:after save',
            'observe',
            'record:saved',
        ]);
    });

    it('reports every fail-closed precondition and receipt outcome', async () => {
        const statuses: string[] = [];
        const baseEffects = {
            fileExists: () => true,
            findOpenDocument: () => undefined,
            publishSavedContent: () => true,
            observeSavedContent: () => undefined,
            recordOutcome: (_filePath: string, status: string) => statuses.push(status),
            reportFailure: () => undefined,
        };

        assert.strictEqual(await processSaveDocumentIntent(undefined, baseEffects), 0);
        assert.strictEqual(await processSaveDocumentIntent('/repo/session.md', baseEffects), 0);
        assert.deepStrictEqual(statuses, ['missing_file', 'missing_document']);

        statuses.length = 0;
        const document: SaveDocumentHandle = {
            save: async () => true,
            getText: () => 'saved text',
        };
        assert.strictEqual(await processSaveDocumentIntent('/repo/session.md', {
            ...baseEffects,
            findOpenDocument: () => document,
            publishSavedContent: () => false,
        }), 0);
        assert.deepStrictEqual(statuses, ['failed']);
    });
});
