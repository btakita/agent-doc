import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import {
    consumeClaimedPatch,
    contentSha256Hex,
    createEditorApplyProof,
    docHash,
    isEditorApplyProofCurrent,
    isFullContentExpectedBufferCurrent,
    isPatchAlreadyApplied,
    resolveAgentDocRootForFile,
} from './patchGuard';

function makeTempDir(): string {
    return fs.mkdtempSync(path.join(os.tmpdir(), 'agent-doc-patch-guard-'));
}

describe('patchGuard', () => {
    it('resolves the nearest .agent-doc root for a file', () => {
        const root = makeTempDir();
        const nested = path.join(root, 'src', 'child');
        fs.mkdirSync(path.join(root, '.agent-doc'), { recursive: true });
        fs.mkdirSync(nested, { recursive: true });
        const filePath = path.join(nested, 'doc.md');
        fs.writeFileSync(filePath, 'content');

        try {
            assert.strictEqual(resolveAgentDocRootForFile(filePath), root);
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
        }
    });

    it('detects when a newer snapshot makes a patch file stale', async () => {
        const root = makeTempDir();
        const doc = path.join(root, 'doc.md');
        const patchFile = path.join(root, '.agent-doc', 'patches', `${docHash(doc)}.json`);
        const snapshotFile = path.join(root, '.agent-doc', 'snapshots', `${docHash(doc)}.md`);
        fs.mkdirSync(path.dirname(patchFile), { recursive: true });
        fs.mkdirSync(path.dirname(snapshotFile), { recursive: true });
        fs.writeFileSync(doc, 'content');
        fs.writeFileSync(patchFile, '{}');
        await new Promise(resolve => setTimeout(resolve, 20));
        fs.writeFileSync(snapshotFile, 'snapshot');

        try {
            assert.strictEqual(isPatchAlreadyApplied(doc, patchFile), true);
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
        }
    });

    it('keeps claimed patch sentinels durable for repeated watcher passes', () => {
        const root = makeTempDir();
        const doc = path.join(root, 'doc.md');
        const sentinel = path.join(root, '.agent-doc', 'claimed-patches', 'patch-123');
        fs.mkdirSync(path.dirname(sentinel), { recursive: true });
        fs.writeFileSync(doc, 'content');
        fs.writeFileSync(sentinel, '');

        try {
            assert.strictEqual(consumeClaimedPatch('patch-123', doc), true);
            assert.strictEqual(fs.existsSync(sentinel), true);
            assert.strictEqual(consumeClaimedPatch('patch-123', doc), true);
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
        }
    });

    it('keeps active-typing patch timeouts as retry states', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const guardIdx = source.indexOf("awaitIdleBeforeDocumentMutation(patch.file, 'file patch', uri.fsPath)");
        const applyIdx = source.indexOf('const applied = await this.applyPatch(patch, uri.fsPath)');

        assert.ok(guardIdx >= 0, 'patch watcher should guard visible writes with typing idle');
        assert.ok(applyIdx > guardIdx, 'patch watcher should guard before applyPatch');
        assert.ok(source.includes('this.schedulePatchRetry(patchFilePath)'));
        assert.ok(source.includes('typing debounce timed out before reposition'));
    });

    it('rejects stale editor apply proofs when content or version changed', () => {
        const proof = createEditorApplyProof('before', 7);

        assert.strictEqual(isEditorApplyProofCurrent(proof, 'before', 7), true);
        assert.strictEqual(isEditorApplyProofCurrent(proof, 'after', 7), false);
        assert.strictEqual(isEditorApplyProofCurrent(proof, 'before', 8), false);
    });

    it('rejects full-content source buffer drift by hash and byte length', () => {
        const expectedHash = contentSha256Hex('before');
        const expectedLen = Buffer.byteLength('before', 'utf8');

        assert.strictEqual(isFullContentExpectedBufferCurrent('before', expectedHash, expectedLen), true);
        assert.strictEqual(isFullContentExpectedBufferCurrent('before\nlive prompt', expectedHash, expectedLen), false);
        assert.strictEqual(isFullContentExpectedBufferCurrent('before', expectedHash, expectedLen + 1), false);
    });

    it('rejects full-content visible writes and still guards component writes', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const fullContentDeleteIdx = source.indexOf('full content IPC is disabled, deleting stale/foreign');
        const fullContentRejectIdx = source.indexOf('full content IPC is disabled for ${patch.file}; rejecting patch');
        const fullContentVisibleEditIdx = source.indexOf('edit.replace(fileUri, fullRange, patch.fullContent)');
        const fullContentProofIdx = source.indexOf("this.verifyApplyProof(document, proof, patch.file, 'full content'");
        const componentProofIdx = source.indexOf("this.verifyApplyProof(document, proof, patch.file, 'component patch'");
        const componentEditIdx = source.indexOf('edit.replace(fileUri, fullRange, content)');

        assert.ok(fullContentDeleteIdx >= 0);
        assert.ok(fullContentRejectIdx >= 0);
        assert.strictEqual(fullContentVisibleEditIdx, -1);
        assert.strictEqual(fullContentProofIdx, -1);
        assert.ok(componentProofIdx >= 0 && componentProofIdx < componentEditIdx);
    });
});
