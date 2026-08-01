import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as fs from 'fs';
import * as path from 'path';
import { EditorIntent } from './editorIntent.js';
import {
    contentSha256Hex,
    createEditorApplyProof,
    isEditorApplyProofCurrent,
    isFullContentExpectedBufferCurrent,
} from './patchGuard.js';
import { fileURLToPath } from 'node:url';

// ESM has no `__dirname`; derive it from the module URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

describe('patchGuard', () => {
    it('rejects a stale editor generation', () => {
        const proof = createEditorApplyProof('before', 7);
        assert.strictEqual(isEditorApplyProofCurrent(proof, 'before', 8), false);
        assert.strictEqual(isEditorApplyProofCurrent(proof, 'changed', 7), false);
        assert.strictEqual(isEditorApplyProofCurrent(proof, 'before', 7), true);
    });

    it('validates expected full-buffer hash and byte length', () => {
        const content = 'visible editor text';
        assert.strictEqual(
            isFullContentExpectedBufferCurrent(
                content,
                contentSha256Hex(content),
                Buffer.byteLength(content, 'utf8'),
            ),
            true,
        );
        assert.strictEqual(isFullContentExpectedBufferCurrent(content, contentSha256Hex('old')), false);
    });

    it('uses a PID-scoped endpoint and contains no live-document sidecar transport', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf8');
        const guardSource = fs.readFileSync(path.join(__dirname, '..', 'src', 'patchGuard.ts'), 'utf8');
        assert.ok(source.includes('`ipc-${process.pid}.sock`'));
        assert.ok(source.includes('case EditorIntent.ApplyCanonical:'));
        assert.strictEqual(source.includes('case EditorIntent.SaveDocument:'), false);
        for (const forbidden of [
            '.agent-doc/patches',
            'claimed-patches',
            'save-document.signal',
            'save_document',
            'crdt-replica-events',
        ]) {
            assert.strictEqual(source.includes(forbidden), false, forbidden);
            assert.strictEqual(guardSource.includes(forbidden), false, forbidden);
        }
    });

    it('keeps editor intent names identical across Rust, JetBrains, and VS Code', () => {
        const root = path.resolve(__dirname, '..', '..', '..');
        const rust = fs.readFileSync(path.join(root, 'agent-doc-ipc-protocol', 'src', 'lib.rs'), 'utf8');
        const jetbrains = fs.readFileSync(
            path.join(root, 'editors', 'jetbrains', 'src', 'main', 'kotlin', 'com', 'github', 'btakita', 'agentdoc', 'PatchWatcher.kt'),
            'utf8',
        );
        for (const token of Object.values(EditorIntent)) {
            assert.ok(rust.includes(`\"${token}\"`), `Rust missing ${token}`);
            assert.ok(jetbrains.includes(`(\"${token}\")`), `JetBrains missing ${token}`);
        }
    });
});
