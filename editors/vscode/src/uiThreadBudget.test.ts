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
        const end = source.indexOf('private async applyReplaceDelivery', start);
        assert.ok(end > start, 'delta-only local-change handler should precede remote apply helpers');
        const handler = source.slice(start, end);

        assert.ok(handler.includes('applyReplicaTextChange(oldText, change)'));
        assert.ok(handler.includes('this.shadows.set(filePath, newText);'));
        assert.strictEqual(handler.includes('document.getText()'), false);
    });

    it('VS Code CRDT remote delivery drains from explicit events, not an interval', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'crdtReplica.ts'), 'utf-8');
        assert.strictEqual(source.includes('setInterval'), false);
        assert.strictEqual(source.includes('pollRemoteUpdates'), false);
        assert.ok(source.includes('requestRemoteDrain(filePath?: string): void'));
        assert.ok(source.includes('private async drainRequestedRemoteUpdates()'));
    });

    it('VS Code watches CRDT events and reads turn state from Project Controller lazily projection', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        assert.ok(source.includes("'.agent-doc', 'crdt-replica-events'"));
        assert.ok(source.includes('private onCrdtReplicaEvent('));
        assert.ok(source.includes('this.crdtReplicas?.requestRemoteDrain(event.file);'));
        assert.ok(source.includes('configureTurnStatusWatcher()'));
        assert.ok(source.includes('TURN_STATUS_MIN_REFRESH_INTERVAL_MS'));
        assert.ok(source.includes("command: 'state_subscribe'"));
        assert.ok(source.includes('turnProjectionFromProjectController'));
        assert.ok(source.includes('Project Controller disconnected'));
        assert.ok(source.includes('async function refreshTurnStatusNow('));
        assert.ok(source.includes("refreshTurnStatus('active-editor', true)"));
        assert.strictEqual(source.includes("'.agent-doc', 'turn-scope'"), false);
        assert.strictEqual(source.includes('turnProjectionForFile('), false);
        assert.strictEqual(source.includes('TURN_STATUS_SLOW_BACKOFF_MS'), false);
        assert.strictEqual(source.includes('const turnStatusInterval = setInterval'), false);
    });

    it('VS Code Run Agent Doc dispatches through the project controller editor_route RPC', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        assert.ok(source.includes('function controllerSocketPath(projectRoot: string): string'));
        assert.ok(source.includes("'.agent-doc', 'controller.sock'"));
        assert.ok(source.includes('async function ensureProjectControllerRunning'));
        assert.ok(source.includes("['controller', 'status', '--project-root', projectRoot, '--ensure']"));
        assert.ok(source.includes('async function runEditorRouteViaProjectController('));
        assert.ok(source.includes("command: 'editor_route'"));
        assert.ok(source.includes('buildEditorRoutePayload(rel, routeKey, layoutArgs'));

        const start = source.indexOf('async function executeRunForDocument');
        assert.ok(start >= 0, 'executeRunForDocument should exist');
        const end = source.indexOf('// ---------------------------------------------------------------------------', start + 1);
        assert.ok(end > start, 'executeRunForDocument should precede next section marker');
        const runBody = source.slice(start, end);
        assert.ok(runBody.includes('await runEditorRouteViaProjectController(cwd, rel, filePath, routeKey, abortController.signal);'));
        assert.strictEqual(runBody.includes('buildRunRouteCommandArgs'), false);
        assert.strictEqual(runBody.includes("runCli(['route'"), false);
        assert.strictEqual(source.includes('function buildRunRouteCommandArgs'), false);
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

    it('VS Code publish-live-buffer signal is read-only and off the typing listener', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        assert.ok(source.includes("'publish-live-buffer.signal'"));
        assert.ok(source.includes('this.onPublishLiveBufferSignal(patchesDir)'));

        const handlerStart = source.indexOf('private async processPublishLiveBufferSignal');
        assert.ok(handlerStart >= 0, 'publish-live-buffer signal handler should exist');
        const handlerEnd = source.indexOf('private writeEditorContentProjection', handlerStart);
        assert.ok(handlerEnd > handlerStart, 'handler should precede content projection helper');
        const handler = source.slice(handlerStart, handlerEnd);
        assert.ok(handler.includes('this.publishLiveBufferNow(document, projectRoot);'));
        assert.ok(handler.includes('PUBLISH_LIVE_BUFFER_SIGNAL_MAX_AGE_MS'));
        assert.ok(handler.includes('signalMtimeMs'));
        assert.ok(handler.includes('stale signal ignored'));
        assert.strictEqual(handler.includes('workspace.applyEdit'), false);
        assert.strictEqual(handler.includes('.save('), false);

        const publishStart = source.indexOf('private publishLiveBufferNow');
        assert.ok(publishStart >= 0, 'immediate live-buffer publisher should exist');
        const publishEnd = source.indexOf('private scheduleEditorOpReport', publishStart);
        assert.ok(publishEnd > publishStart, 'publisher should precede editor-op scheduler');
        const publisher = source.slice(publishStart, publishEnd);
        assert.ok(publisher.includes('document.getText()'));
        assert.ok(publisher.includes('native.documentChangedDigestContent(fsPath, text, projectRoot, EDITOR_ID, noUnsavedOperatorEdits);'));
        assert.ok(publisher.includes('this.crdtReplicas?.attachDocument(fsPath, text, true)'));
        assert.strictEqual(publisher.includes('workspace.applyEdit'), false);
        assert.strictEqual(publisher.includes('.save('), false);
    });
});
