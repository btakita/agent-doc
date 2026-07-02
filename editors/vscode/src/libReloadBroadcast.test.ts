import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';

// #cdylib-reload-broadcast: source-level assertions that both editor plugins
// wire the global reload-broadcast file to a forced native cdylib reload. This
// mirrors the JetBrains poller for shared-foundation parity: an install (or
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

    it('VS Code extension.ts polls the broadcast file and forces a reload on change', () => {
        const source = fs.readFileSync(path.join(srcDir, 'extension.ts'), 'utf-8');
        assert.ok(
            source.includes('startLibReloadBroadcastPoll('),
            'extension.ts must start the broadcast poll',
        );
        const start = source.indexOf('private pollLibReloadBroadcastOnce(');
        assert.ok(start >= 0, 'extension.ts must define pollLibReloadBroadcastOnce');
        const end = source.indexOf('dispose(): void {', start);
        assert.ok(end > start, 'poll method should precede dispose');
        const poll = source.slice(start, end);
        assert.ok(poll.includes('native.reloadBroadcastFile('), 'poll must resolve the broadcast file');
        assert.ok(poll.includes('native.forceReloadLib('), 'poll must force the native reload on change');
        // The poll must be disposed to avoid a leaked interval.
        assert.ok(
            source.includes('clearInterval(this.libReloadBroadcastTimer)'),
            'dispose must clear the broadcast poll interval',
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

    it('JetBrains PatchWatcher polls the broadcast file and handles the reload_lib socket message', () => {
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
            patchWatcher.includes('scheduleLibReloadBroadcastPoll('),
            'PatchWatcher.kt must schedule the broadcast poll',
        );
        assert.ok(
            patchWatcher.includes('AgentDocLib.reloadBroadcastFile('),
            'PatchWatcher.kt poll must resolve the broadcast file',
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
