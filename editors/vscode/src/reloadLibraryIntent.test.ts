import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';
import { EditorIntent } from './editorIntent.js';
import { fileURLToPath } from 'node:url';

// ESM has no `__dirname`; derive it from the module URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

describe('typed reload_library intent', () => {
    const srcDir = path.join(__dirname, '..', 'src');

    it('uses the shared cross-language name', () => {
        assert.equal(EditorIntent.ReloadLibrary, 'reload_library');
    });

    it('VS Code reloads and reattaches only from the targeted socket intent', () => {
        const extension = fs.readFileSync(path.join(srcDir, 'extension.ts'), 'utf-8');
        const start = extension.indexOf('case EditorIntent.ReloadLibrary:');
        assert.ok(start >= 0);
        const handler = extension.slice(start, extension.indexOf('default:', start));
        assert.ok(handler.includes('native.forceReloadLib(projectRoot)'));
        assert.ok(handler.includes('this.crdtReplicas?.attachDocument('));
        assert.ok(handler.includes('nativeReloadGate.begin()'));
        assert.ok(handler.includes('await reload.completion'));
        assert.ok(handler.includes('requestSurfaceObservation()'));
        assert.ok(extension.includes('if (!this.targetsSocketMessage(message))'));
        assert.ok(!extension.includes('createFileSystemWatcher('));
        assert.ok(!extension.includes('ReloadBroadcast'));

        const native = fs.readFileSync(path.join(srcDir, 'native.ts'), 'utf-8');
        assert.ok(native.includes('export function forceReloadLib('));
        assert.ok(native.includes("NATIVE_HOT_RELOAD_CAPABILITY = 'native_hot_reload_generation_v1'"));
        assert.ok(!native.includes('reloadBroadcastFile'));
    });

    it('VS Code gates native actions and attaches Compact Exchange to its live replica', () => {
        const extension = fs.readFileSync(path.join(srcDir, 'extension.ts'), 'utf-8');
        const compactStart = extension.indexOf('async function compactExchangeAction()');
        const compactEnd = extension.indexOf('async function runWithJunieAction()', compactStart);
        const compact = extension.slice(compactStart, compactEnd);
        assert.ok(compact.includes('nativeReloadGate.awaitReady('));
        assert.ok(compact.includes('patchWatcher?.ensureOpenReplica('));

        const syncStart = extension.indexOf('async function syncLayoutInternal(');
        const syncEnd = extension.indexOf('// Feature 4: Editor Surface Reporting', syncStart);
        const sync = extension.slice(syncStart, syncEnd);
        assert.ok(sync.includes('nativeReloadGate.awaitReady('));
        assert.ok(sync.indexOf('nativeReloadGate.awaitReady(') < sync.indexOf('native.syncTmuxLayoutJson('));
    });

    it('JetBrains reloads and refreshes replicas only from the typed intent', () => {
        const jetbrainsDir = path.join(
            srcDir,
            '..',
            '..',
            'jetbrains',
            'src',
            'main',
            'kotlin',
            'com',
            'github',
            'btakita',
            'agentdoc',
        );
        const watcher = fs.readFileSync(path.join(jetbrainsDir, 'PatchWatcher.kt'), 'utf-8');
        assert.ok(watcher.includes('ReloadLibrary("reload_library")'));
        const start = watcher.indexOf('EditorIntent.ReloadLibrary.token ->');
        assert.ok(start >= 0);
        const handler = watcher.slice(start, start + 500);
        assert.ok(handler.includes('NativeReloadCoordinator.requestReload(libVersion)'));
        assert.ok(!watcher.includes('newWatchService()'));
        assert.ok(!watcher.includes('reloadBroadcastFile'));

        const native = fs.readFileSync(path.join(jetbrainsDir, 'NativeLib.kt'), 'utf-8');
        assert.ok(native.includes('fun hotReload('));
        assert.ok(!native.includes('reloadBroadcastFile'));

        const coordinator = fs.readFileSync(
            path.join(jetbrainsDir, 'NativeReloadCoordinator.kt'),
            'utf-8',
        );
        assert.ok(coordinator.includes('CrdtReplicaManager.quiesceAllForNativeReload()'));
        assert.ok(coordinator.includes('PatchWatcher.quiesceAllForNativeReload()'));
        assert.ok(coordinator.includes('AgentDocLib.hotReload(libVersion)'));
        assert.ok(coordinator.includes('CrdtReplicaManager.restartAfterNativeReload(replicaProjects)'));
    });
});
