import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';

// #cdylib-reload-broadcast: source-level assertions that both editor plugins
// wire the global reload-broadcast file to a forced native cdylib reload. This
// mirrors the JetBrains watcher for shared-foundation parity: an install (or
// `agent-doc admin reload-lib`) writes the broadcast, and each plugin watches it
// and forces its existing native-reload path instead of waiting for the next
// lazy FFI call.
describe('cdylib reload broadcast wiring', () => {
    const srcDir = path.join(__dirname, '..', 'src');

    it('VS Code native.ts resolves the broadcast file and force-reloads the cdylib', () => {
        const source = fs.readFileSync(path.join(srcDir, 'native.ts'), 'utf-8');
        assert.ok(
            source.includes("RELOAD_BROADCAST_FILENAME = 'agent-doc-reload-broadcast.json'"),
            'native.ts must define the well-known broadcast filename',
        );
        assert.ok(
            source.includes('export function reloadBroadcastFile('),
            'native.ts must export reloadBroadcastFile()',
        );
        assert.ok(
            source.includes('export function forceReloadLib('),
            'native.ts must export forceReloadLib()',
        );
        // forceReload must actually re-load via koffi, not just no-op.
        const start = source.indexOf('export function forceReloadLib(');
        const body = source.slice(start, start + 800);
        assert.ok(body.includes('koffi.load('), 'forceReloadLib must call koffi.load');
    });

    it('VS Code extension.ts watches the broadcast file and forces a reload on change', () => {
        const source = fs.readFileSync(path.join(srcDir, 'extension.ts'), 'utf-8');
        assert.ok(
            source.includes('startLibReloadBroadcastWatcher('),
            'extension.ts must start the broadcast watcher',
        );
        const start = source.indexOf('private onLibReloadBroadcastEvent(');
        assert.ok(start >= 0, 'extension.ts must define onLibReloadBroadcastEvent');
        const end = source.indexOf('dispose(): void {', start);
        assert.ok(end > start, 'event handler should precede dispose');
        const handler = source.slice(start, end);
        assert.ok(handler.includes('native.reloadBroadcastFile('), 'handler must resolve the broadcast file');
        assert.ok(handler.includes('native.forceReloadLib('), 'handler must force the native reload on change');
        assert.ok(
            source.includes('this.libReloadBroadcastWatcher?.dispose()'),
            'dispose must dispose the broadcast watcher',
        );
    });

    it('JetBrains AgentDocLib exposes a forceReload + broadcast-file resolver', () => {
        const nativeLib = fs.readFileSync(
            path.join(
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
                'NativeLib.kt',
            ),
            'utf-8',
        );
        assert.ok(
            nativeLib.includes('RELOAD_BROADCAST_FILENAME = "agent-doc-reload-broadcast.json"'),
            'NativeLib.kt must define the well-known broadcast filename',
        );
        assert.ok(nativeLib.includes('fun reloadBroadcastFile('), 'NativeLib.kt must expose reloadBroadcastFile()');
        assert.ok(nativeLib.includes('fun forceReload('), 'NativeLib.kt must expose forceReload()');
        assert.ok(nativeLib.includes('Native.load('), 'forceReload must actually reload the native lib');
    });

    it('JetBrains PatchWatcher watches the broadcast file and handles the reload_lib socket message', () => {
        const patchWatcher = fs.readFileSync(
            path.join(
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
                'PatchWatcher.kt',
            ),
            'utf-8',
        );
        assert.ok(
            patchWatcher.includes('startLibReloadBroadcastWatcher('),
            'PatchWatcher.kt must start the broadcast watcher',
        );
        assert.ok(
            patchWatcher.includes('newWatchService()'),
            'PatchWatcher.kt must use WatchService for the broadcast file',
        );
        assert.ok(
            patchWatcher.includes('AgentDocLib.reloadBroadcastFile('),
            'PatchWatcher.kt watcher must resolve the broadcast file',
        );
        assert.ok(
            patchWatcher.includes('AgentDocLib.forceReload()'),
            'PatchWatcher.kt must force the native reload',
        );
        // The reload_lib socket message must map to the same forced reload.
        const start = patchWatcher.indexOf('"reload_lib" ->');
        assert.ok(start >= 0, 'PatchWatcher.kt must handle the reload_lib socket message');
        const handler = patchWatcher.slice(start, start + 600);
        assert.ok(handler.includes('AgentDocLib.forceReload()'), 'reload_lib handler must force the reload');
    });
});
