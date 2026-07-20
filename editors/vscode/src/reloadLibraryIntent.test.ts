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
        const handler = extension.slice(start, start + 500);
        assert.ok(handler.includes('native.forceReloadLib(projectRoot)'));
        assert.ok(handler.includes('this.crdtReplicas?.attachDocument('));
        assert.ok(extension.includes('if (!this.targetsSocketMessage(message))'));
        assert.ok(!extension.includes('createFileSystemWatcher('));
        assert.ok(!extension.includes('ReloadBroadcast'));

        const native = fs.readFileSync(path.join(srcDir, 'native.ts'), 'utf-8');
        assert.ok(native.includes('export function forceReloadLib('));
        assert.ok(!native.includes('reloadBroadcastFile'));
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
        assert.ok(handler.includes('AgentDocLib.forceReload()'));
        assert.ok(handler.includes('CrdtReplicaManager.forceRefreshOpenDocumentReplicas('));
        assert.ok(!watcher.includes('newWatchService()'));
        assert.ok(!watcher.includes('reloadBroadcastFile'));

        const native = fs.readFileSync(path.join(jetbrainsDir, 'NativeLib.kt'), 'utf-8');
        assert.ok(native.includes('fun forceReload('));
        assert.ok(!native.includes('reloadBroadcastFile'));
    });
});
