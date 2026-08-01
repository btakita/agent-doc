import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// ESM has no `__dirname`; derive it from the module URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

describe('editor UI thread budget', () => {
    it('VS Code text-change listener defers full-buffer and native-heavy work', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const start = source.indexOf('this.typingListener = vscode.workspace.onDidChangeTextDocument');
        assert.ok(start >= 0, 'typing listener should exist');
        const end = source.indexOf('this.outputChannel.appendLine(`PatchWatcher: Lazily endpoint active', start);
        assert.ok(end > start, 'typing listener should precede watcher startup log');
        const listener = source.slice(start, end);

        assert.ok(listener.includes('this.scheduleLazilyCurrentObservation(e.document, eventProjectRoot);'));
    assert.ok(listener.includes('this.scheduleCrdtLocalChangeDelta(fsPath, changes, admission);'));
    assert.ok(listener.includes('this.scheduleEditorOpReport(fsPath, e.contentChanges, eventProjectRoot);'));
    assert.ok(listener.includes('const operatorEdit = !remoteCrdtApply && (e.document.isDirty || e.reason !== undefined);'));
    assert.ok(listener.includes('const admission = this.crdtReplicas?.captureLocalChange(fsPath, operatorEdit);'));
        assert.strictEqual(listener.includes('e.document.getText()'), false);
        assert.strictEqual(listener.includes('native.documentChanged('), false);
        assert.strictEqual(listener.includes('lazilyCurrentObserved'), false);
        assert.strictEqual(listener.includes('native.recordEditorOp('), false);
        assert.strictEqual(listener.includes('reportEditorChange('), false);
        assert.strictEqual(listener.includes('handleLocalChangeDelta('), false);
    });

    it('VS Code coalesces full-buffer reporting through lazily KeepLatest debounce', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        assert.ok(source.includes("import { DebounceCore } from '@lazily-hub/lazily-js/rateshape';"));
        assert.ok(source.includes('new DebounceCore<string>(LAZILY_CURRENT_OBSERVATION_DELAY_MS)'));
        assert.ok(source.includes('state.debounce.input(monotonicMillis(), fsPath)'));
        assert.ok(source.includes('state.debounce.tick(monotonicMillis())'));
        assert.ok(source.includes('this.lazilyCurrentObservations.get(fsPath) !== state'));
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

it('VS Code receives CRDT events and renders the controller-owned in-memory turn projection', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        assert.strictEqual(source.includes("'.agent-doc', 'crdt-replica-events'"), false);
        assert.ok(source.includes('case EditorIntent.DeliverCrdtRemote:'));
        assert.ok(source.includes('this.crdtReplicas?.requestRemoteDrain(filePath);'));
        assert.ok(source.includes('configureTurnStatusWatcher()'));
        assert.ok(source.includes('TURN_STATUS_CACHE_OBSERVE_INTERVAL_MS'));
        assert.ok(source.includes('TURN_STATUS_AUTHORITY_SETTLE_MS'));
    assert.ok(source.includes('currentControllerTurnAuthority'));
    assert.ok(source.includes("command: 'document_turn_projection'"));
    assert.strictEqual(source.includes('native.currentDocumentAuthorityJson(projectRoot)'), false);
        assert.ok(source.includes('Project Controller disconnected'));
        assert.ok(source.includes('function refreshTurnStatusNow('));
        assert.ok(source.includes("refreshTurnStatus('active-editor', true)"));
        assert.strictEqual(source.includes("'.agent-doc', 'turn-scope'"), false);
        assert.strictEqual(source.includes('turnProjectionForFile('), false);
    assert.strictEqual(source.includes('TURN_STATUS_SLOW_BACKOFF_MS'), false);
    assert.strictEqual(source.includes('const turnStatusInterval = setInterval'), false);
    const turnStatusSection = source.slice(
        source.indexOf('// Controller-owned turn-state coordination.'),
        source.indexOf('// Visual highlighting'),
    );
    assert.ok(turnStatusSection.includes('requestProjectController('));
    assert.ok(turnStatusSection.includes("command: 'document_turn_projection'"));
    assert.strictEqual(turnStatusSection.includes("command: 'state_subscribe'"), false);
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
        const end = source.indexOf('private projectRoot()', start);
        assert.ok(end > start, 'CRDT scheduler should precede the next endpoint helper');
        const scheduler = source.slice(start, end);

        assert.ok(scheduler.includes('setTimeout(() => {'));
    assert.ok(scheduler.includes('handleLocalChangeDelta(fsPath, changes, admission)'));
    });

    it('VS Code visual highlighter defers non-markdown documents before scheduling refresh', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        // Parity with JetBrains VisualHighlighterManager: non-markdown documents
        // must short-circuit before any timer/map work so per-keystroke churn is
        // never paid for other file types.
        const listenerMarker = 'vscode.workspace.onDidChangeTextDocument((event) => this.scheduleRefresh(event.document))';
        assert.ok(source.includes(listenerMarker), 'highlighter onDidChangeTextDocument must route through scheduleRefresh');

        const start = source.indexOf('private scheduleRefresh(document: vscode.TextDocument): void {');
        assert.ok(start >= 0, 'highlighter scheduleRefresh should exist');
        const end = source.indexOf('private refreshAll()', start);
        assert.ok(end > start, 'highlighter scheduleRefresh should precede refreshAll');
        const method = source.slice(start, end);

        const guard = method.indexOf("document.languageId !== 'markdown'");
        const firstTimerWork = method.indexOf('this.refreshTimers');
        assert.ok(guard >= 0, 'scheduleRefresh must guard on markdown languageId');
        assert.ok(firstTimerWork > guard, 'markdown guard must precede any refresh-timer work');
        assert.ok(method.includes('setTimeout'));
    });

    it('VS Code observes Lazily current continuously without a file signal', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        assert.strictEqual(source.includes('observe-lazily-current.signal'), false);
        assert.ok(source.includes('this.scheduleLazilyCurrentObservation(document, projectRoot);'));
        assert.ok(source.includes('case EditorIntent.ObserveLazilyCurrent:'));

        const publishStart = source.indexOf('private observeLazilyCurrentNow');
        assert.ok(publishStart >= 0, 'immediate live-buffer publisher should exist');
        const publishEnd = source.indexOf('private scheduleEditorOpReport', publishStart);
        assert.ok(publishEnd > publishStart, 'publisher should precede editor-op scheduler');
        const publisher = source.slice(publishStart, publishEnd);
        assert.ok(publisher.includes('document.getText()'));
        assert.ok(publisher.includes('native.lazilyCurrentObserved(fsPath, text, projectRoot, EDITOR_ID, noUnsavedOperatorEdits);'));
        assert.ok(publisher.includes('this.crdtReplicas?.attachDocument(fsPath, text, true)'));
        assert.strictEqual(publisher.includes('workspace.applyEdit'), false);
        assert.strictEqual(publisher.includes('.save('), false);
    });

    it('VS Code records every save_document outcome through the shared native surface ABI', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const intent = fs.readFileSync(path.join(__dirname, '..', 'src', 'saveDocumentIntent.ts'), 'utf-8');
        const start = source.indexOf('case EditorIntent.SaveDocument:');
        assert.ok(start >= 0, 'save_document socket handler should exist');
        const end = source.indexOf('case EditorIntent.RefreshVcs:', start);
        assert.ok(end > start, 'save_document handler should precede refresh_vcs');
        const handler = source.slice(start, end);

        assert.ok(handler.includes('return processSaveDocumentIntent(filePath, {'));
        assert.ok(handler.includes('native.recordEditorSurfaceEvent('));
        for (const status of ['missing_file', 'missing_document', 'saved', 'failed']) {
            assert.ok(intent.includes(`'${status}'`), `save_document should record ${status}`);
        }
        assert.ok(handler.includes('publishSavedContent:'));
        assert.ok(handler.includes('observeSavedContent:'));
    });

    it('VS Code refreshes only the repository containing the requested file', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf-8');
        const start = source.indexOf('async function refreshVcsForFile(filePath: string)');
        assert.ok(start >= 0, 'path-scoped VCS refresh helper should exist');
        const end = source.indexOf('// #qnodemerge4wire', start);
        assert.ok(end > start, 'path-scoped VCS refresh helper should precede editor-op state');
        const helper = source.slice(start, end);

        assert.ok(helper.includes("vscode.extensions.getExtension<GitExtensionExports>('vscode.git')"));
        assert.ok(helper.includes('getRepository(vscode.Uri.file(filePath))'));
        assert.ok(helper.includes('await repository?.status()'));
        assert.strictEqual(source.includes("executeCommand('git.refresh')"), false);

        const handlerStart = source.indexOf('case EditorIntent.RefreshVcs:');
        const handlerEnd = source.indexOf('case EditorIntent.ReloadLibrary:', handlerStart);
        assert.ok(handlerEnd > handlerStart, 'refresh_vcs handler should precede reload_library');
        const handler = source.slice(handlerStart, handlerEnd);
        assert.ok(handler.includes('if (filePath) await refreshVcsForFile(filePath);'));
    });

    it('VS Code reliable-sync liveness seeds restored tabs exactly once', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'reliableSyncLiveness.ts'), 'utf-8');
        assert.ok(source.includes('for (const document of vscode.workspace.textDocuments) reportOpen(document);'));
        assert.ok(source.includes('if (this.docs.get(documentHash)?.orSet.present() === true) return null;'));
        assert.ok(source.includes('if (!opsJson) return;'));
        assert.ok(source.includes('Register: {'));
        assert.ok(source.includes('editor_id: editorId'));
        assert.ok(source.includes('resolveProjectPath(filePath, workspaceRoot)?.projectRoot ?? workspaceRoot'));
        assert.ok(source.includes('projectRoots.set(documentHash, root)'));
        assert.ok(source.includes('projectRoots.get(documentHash)'));
    });
});
