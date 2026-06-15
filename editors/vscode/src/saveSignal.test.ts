import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as path from 'path';
import { parseSaveDocumentSignal, ackContentSidecarPath } from './saveSignal';

describe('parseSaveDocumentSignal', () => {
    it('parses a well-formed signal with file + patch_id', () => {
        const signal = parseSaveDocumentSignal(
            JSON.stringify({ file: '/repo/plan.md', patch_id: 'save-123' }),
        );
        assert.deepStrictEqual(signal, { file: '/repo/plan.md', patchId: 'save-123' });
    });

    it('parses a signal with a file but no patch_id', () => {
        const signal = parseSaveDocumentSignal(JSON.stringify({ file: '/repo/plan.md' }));
        assert.deepStrictEqual(signal, { file: '/repo/plan.md', patchId: undefined });
    });

    it('treats an empty patch_id as absent', () => {
        const signal = parseSaveDocumentSignal(
            JSON.stringify({ file: '/repo/plan.md', patch_id: '' }),
        );
        assert.deepStrictEqual(signal, { file: '/repo/plan.md', patchId: undefined });
    });

    it('returns null for malformed JSON', () => {
        assert.strictEqual(parseSaveDocumentSignal('not json'), null);
    });

    it('returns null when the file path is missing', () => {
        assert.strictEqual(parseSaveDocumentSignal(JSON.stringify({ patch_id: 'x' })), null);
    });

    it('returns null when the file path is empty', () => {
        assert.strictEqual(parseSaveDocumentSignal(JSON.stringify({ file: '' })), null);
    });
});

describe('ackContentSidecarPath', () => {
    it('derives the sibling ack-content sidecar from the patches dir', () => {
        const patchesDir = path.join('/repo', '.agent-doc', 'patches');
        assert.strictEqual(
            ackContentSidecarPath(patchesDir, 'save-123'),
            path.join('/repo', '.agent-doc', 'ack-content', 'save-123.md'),
        );
    });
});
