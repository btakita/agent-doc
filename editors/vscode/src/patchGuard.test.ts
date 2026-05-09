import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { consumeClaimedPatch, docHash, isPatchAlreadyApplied, resolveAgentDocRootForFile } from './patchGuard';

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
});
