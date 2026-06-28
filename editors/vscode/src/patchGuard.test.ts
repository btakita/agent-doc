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

    it('requires ack-content proof before file patch apply can delete the patch file', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const applyIdx = source.indexOf('const applied = await this.applyPatch(patch, uri.fsPath)');
        const deleteIdx = source.indexOf('fs.unlinkSync(uri.fsPath)', source.indexOf('if (applied) {'));
        const minimalApplyIdx = source.indexOf('const ok = await this.applyMinimalTextEdit(document, content);');
        const ackIdx = source.indexOf('return this.writeAckContent(patch.patch_id, document.getText(), patchesDir);');
        const applyPatchStart = source.indexOf('private async applyPatch(');
        const applyPatchEnd = source.indexOf('private async applyMinimalTextEdit(', applyPatchStart);
        const applyPatchBody = source.slice(applyPatchStart, applyPatchEnd);

        assert.ok(applyIdx >= 0);
        assert.ok(deleteIdx > applyIdx);
        assert.ok(minimalApplyIdx >= 0);
        assert.ok(ackIdx > minimalApplyIdx);
        assert.strictEqual(applyPatchBody.includes('document.save()'), false);
        assert.ok(source.includes('private writeAckContent('));
        assert.ok(source.includes('): boolean {'));
        assert.ok(source.includes('return this.writeAckContent(patch.patch_id, document.getText(), patchesDir);'));
        assert.ok(source.includes('ackContentSidecarPath(patchesDir, patchId)'));
    });

    it('publishes plugin owner before VS Code patch ack', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const nativeSource = fs.readFileSync(path.join(__dirname, '..', 'src', 'native.ts'), 'utf-8');
        const branch = source.slice(
            source.indexOf('private async onPatchFileCreated('),
            source.indexOf('private async applyPatch(', source.indexOf('private async onPatchFileCreated(')),
        );

        const ownerIdx = branch.indexOf('this.ownsDocument(patch.file, projectRoot)');
        const queuedIdx = branch.indexOf('native.recordEditorPatchQueued');
        const applyIdx = branch.indexOf('const applied = await this.applyPatch(patch, uri.fsPath)');
        const ackIdx = branch.indexOf('native.recordEditorAckObserved');

        assert.ok(ownerIdx >= 0 && ownerIdx < queuedIdx, 'VS Code must acquire/publish plugin-owner proof before queueing');
        assert.ok(ownerIdx >= 0 && ownerIdx < applyIdx, 'VS Code must acquire/publish plugin-owner proof before applying');
        assert.ok(ownerIdx >= 0 && ownerIdx < ackIdx, 'VS Code must acquire/publish plugin-owner proof before ACKing');
        assert.ok(source.includes('private ownsDocument('));
        assert.ok(source.includes('native.pluginOwnerTryAcquire(filePath, EDITOR_ID, process.pid, projectRoot)'));
        assert.ok(source.includes('native.pluginOwnerRelease(filePath, EDITOR_ID, this.projectRoot())'));
        assert.ok(nativeSource.includes('agent_doc_plugin_owner_try_acquire'));
        assert.ok(nativeSource.includes('export function pluginOwnerTryAcquire('));
    });

    it('does not autosave command paths', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const sessionCommandStart = source.indexOf('async function runSessionCommandForActiveFile(');
        const sessionCommandEnd = source.indexOf('function showSessionOutput', sessionCommandStart);
        const killStart = source.indexOf('async function killSupervisorAction()');
        const killEnd = source.indexOf('function resolveCleanupCwd()', killStart);
        const interruptClearStart = source.indexOf('async function interruptClearSessionContextAction()');
        const interruptClearEnd = source.indexOf('// ---------------------------------------------------------------------------\n// Feature 2: Claim', interruptClearStart);

        assert.strictEqual(source.includes('document.save()'), false);
        assert.strictEqual(/\.save\(/.test(source), false);
        assert.ok(source.includes('async function ensureDocumentCleanForCommand('));
        assert.ok(source.includes("ensureDocumentCleanForCommand(filePath, 'Run')"));
        assert.ok(source.includes("ensureDocumentCleanForCommand(editor.document.uri.fsPath, 'Fix document')"));
        assert.ok(source.includes("ensureDocumentCleanForCommand(editor.document.uri.fsPath, 'Compact exchange')"));
        assert.ok(source.includes("ensureDocumentCleanForCommand(editor.document.uri.fsPath, 'Run with Junie')"));
        assert.ok(source.includes("ensureDocumentCleanForCommand(filePath, 'Clear Session Context')"));
        assert.ok(sessionCommandStart >= 0 && sessionCommandEnd > sessionCommandStart);
        assert.ok(killStart >= 0 && killEnd > killStart);
        assert.ok(interruptClearStart >= 0 && interruptClearEnd > interruptClearStart);
        assert.strictEqual(source.slice(sessionCommandStart, sessionCommandEnd).includes('document.save()'), false);
        assert.strictEqual(source.slice(killStart, killEnd).includes('document.save()'), false);
        assert.strictEqual(source.slice(interruptClearStart, interruptClearEnd).includes('document.save()'), false);
    });

    it('keeps legacy save and reconnect repair paths disabled', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');

        assert.ok(source.includes('save_document IPC is disabled'));
        assert.ok(source.includes('reread_disk repair is disabled'));
        assert.strictEqual(source.includes('saveDocumentToDisk'), false);
        assert.strictEqual(source.includes('applyReconnectReread'), false);
        assert.strictEqual(source.includes('reread disk into stale buffer'), false);
        assert.strictEqual(/\.save\(/.test(source), false);
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
        const componentEditIdx = source.indexOf('this.applyMinimalTextEdit(document, content)');
        const fullDocumentEditGuardIdx = source.indexOf('isFullDocumentReplacement(before, replacement)');
        const rangeCreationIdx = source.indexOf('const range = new vscode.Range(', source.indexOf('private async applyMinimalTextEdit('));

        assert.ok(fullContentDeleteIdx >= 0);
        assert.ok(fullContentRejectIdx >= 0);
        assert.strictEqual(fullContentVisibleEditIdx, -1);
        assert.strictEqual(fullContentProofIdx, -1);
        assert.ok(componentProofIdx >= 0 && componentProofIdx < componentEditIdx);
        assert.ok(fullDocumentEditGuardIdx >= 0 && fullDocumentEditGuardIdx < rangeCreationIdx);
        assert.strictEqual(source.includes('document.lineCount'), false);
    });

    it('keeps cycle 1779845677327 full-content fixture off visible write paths', () => {
        const cycleFixture = {
            file: '/repo/tasks/agent-doc/agent-doc-bugs2.md',
            patch_id: 'cycle-1779845677327',
            patches: [],
            fullContent: `<!-- agent:exchange -->\n❯ do [#liveipcrace]\n<!-- /agent:exchange -->\n\n###\n\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again.\n#spec-test-build-install-commit-push\n---\ndispatch #spec-test-build-install-commit-push\n-->\n`,
        };
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const fileFullContentGuardIdx = source.indexOf("if ((patch.fullContent ?? '') !== '')");
        const fileApplyIdx = source.indexOf('const applied = await this.applyPatch(patch, uri.fsPath)');
        const socketFullContentGuardIdx = source.indexOf("if (patch.fullContent != null && patch.fullContent !== '')");
        const componentEditIdx = source.indexOf('this.applyMinimalTextEdit(document, content)');

        assert.ok(cycleFixture.fullContent.includes('#spec-test-build-install-commit-push'));
        assert.ok(cycleFixture.fullContent.includes('dispatch #spec-test-build-install-commit-push'));
        assert.ok(fileFullContentGuardIdx >= 0 && fileFullContentGuardIdx < fileApplyIdx);
        assert.ok(socketFullContentGuardIdx >= 0 && socketFullContentGuardIdx < componentEditIdx);
        assert.strictEqual(source.indexOf('fullRange, patch.fullContent'), -1);
        assert.strictEqual(source.indexOf('edit.replace(fileUri, fullRange, content)'), -1);
        assert.ok(source.includes('full-document WorkspaceEdit replacement is disabled'));
    });

    it('honors explicit component op overrides for convergence patches', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');

        assert.ok(source.includes('content = this.applyComponentPatch(content, p.component, p.content, p.op);'));
        assert.ok(source.includes('const overrideMode = this.componentPatchModeOverride(modeOverride);'));
        assert.ok(source.includes('if (overrideMode == null && openMatch[1])'));
        assert.ok(source.includes("normalized === 'append' || normalized === 'prepend' || normalized === 'replace'"));
    });

    it('uses per-editor identity for live-buffer reports and targeted patch filtering', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const nativeSource = fs.readFileSync(path.join(__dirname, '..', 'src', 'native.ts'), 'utf-8');

        assert.ok(source.includes('const EDITOR_ID = `vscode-${process.pid}-${crypto.randomUUID()}`'));
        assert.ok(source.includes('this.targetsThisEditor(patch)'));
        assert.ok(source.includes('patch.editor_id && patch.editor_id !== EDITOR_ID'));
        assert.ok(source.includes('patch.origin_editor_id === EDITOR_ID'));
        assert.ok(source.includes('native.documentClosedForEditor(document.uri.fsPath'));
        assert.ok(nativeSource.includes('agent_doc_document_changed_digest_content_for_editor'));
        assert.ok(nativeSource.includes('agent_doc_document_changed_digest_content_for_editor_v2'));
        assert.ok(nativeSource.includes('operator_text_authority_v1'));
        assert.ok(nativeSource.includes('agent_doc_document_closed_for_editor'));
    });
});
