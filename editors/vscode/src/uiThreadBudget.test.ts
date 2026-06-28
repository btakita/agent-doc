import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';

describe('editor UI thread budget', () => {
    it('VS Code text-change listener defers full-buffer and native-heavy work', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const start = source.indexOf('this.typingListener = vscode.workspace.onDidChangeTextDocument');
        assert.ok(start >= 0, 'typing listener should exist');
        const end = source.indexOf('this.outputChannel.appendLine(`PatchWatcher: watching', start);
        assert.ok(end > start, 'typing listener should precede watcher startup log');
        const listener = source.slice(start, end);

        assert.ok(listener.includes('this.scheduleNativeDocumentChanged(fsPath, eventProjectRoot);'));
        assert.ok(listener.includes('this.scheduleLiveBufferReport(e.document, eventProjectRoot);'));
        assert.ok(listener.includes('this.scheduleCrdtLocalChangeDelta(fsPath, changes);'));
        assert.ok(listener.includes('this.scheduleEditorOpReport(fsPath, e.contentChanges, eventProjectRoot);'));
        assert.strictEqual(listener.includes('e.document.getText()'), false);
        assert.strictEqual(listener.includes('native.documentChanged('), false);
        assert.strictEqual(listener.includes('documentChangedDigestContent'), false);
        assert.strictEqual(listener.includes('native.recordEditorOp('), false);
        assert.strictEqual(listener.includes('reportEditorChange('), false);
        assert.strictEqual(listener.includes('handleLocalChangeDelta('), false);
    });

    it('VS Code CRDT local-change hot path updates shadows from deltas', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'crdtReplica.ts'), 'utf-8');
        const start = source.indexOf('async handleLocalChangeDelta');
        assert.ok(start >= 0, 'delta-only local-change handler should exist');
        const end = source.indexOf('async pollRemoteUpdates', start);
        assert.ok(end > start, 'delta-only local-change handler should precede polling');
        const handler = source.slice(start, end);

        assert.ok(handler.includes('applyReplicaTextChange(oldText, change)'));
        assert.ok(handler.includes('this.shadows.set(filePath, newText);'));
        assert.strictEqual(handler.includes('document.getText()'), false);
    });

    it('VS Code schedules CRDT local forwarding off the text-change listener', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const start = source.indexOf('private scheduleCrdtLocalChangeDelta');
        assert.ok(start >= 0, 'CRDT local-change scheduler should exist');
        const end = source.indexOf('private async onPatchFileCreated', start);
        assert.ok(end > start, 'CRDT scheduler should precede patch processing');
        const scheduler = source.slice(start, end);

        assert.ok(scheduler.includes('setTimeout(() => {'));
        assert.ok(scheduler.includes('handleLocalChangeDelta(fsPath, changes)'));
    });
});
