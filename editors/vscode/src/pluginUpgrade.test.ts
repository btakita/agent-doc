import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    AGENT_DOC_EXTENSION_ID,
    installedExtension,
    needsWindowReload,
    obsoleteDirectories,
    staleExtensionVerdict,
    upgradePrompt,
} from './pluginUpgrade.js';

function entry(version: string, id = AGENT_DOC_EXTENSION_ID, dir = `${id}-${version}`) {
    return {
        identifier: { id, uuid: 'uuid' },
        version,
        location: { $mid: 1, path: `/home/me/.vscode/extensions/${dir}`, scheme: 'file' },
        relativeLocation: dir,
        metadata: { installedTimestamp: 1 },
    };
}

describe('installedExtension', () => {
    it('reads the agent-doc generation from extensions.json', () => {
        const manifest = JSON.stringify([entry('1.0.0', 'other.ext'), entry('0.2.84')]);
        assert.deepStrictEqual(installedExtension(manifest, AGENT_DOC_EXTENSION_ID), {
            version: '0.2.84',
            location: '/home/me/.vscode/extensions/btakita.agent-doc-0.2.84',
        });
    });

    it('matches the id case-insensitively and resolves several entries to the newest', () => {
        const manifest = JSON.stringify([entry('0.2.9', 'BTakita.Agent-Doc'), entry('0.2.84')]);
        assert.strictEqual(installedExtension(manifest, AGENT_DOC_EXTENSION_ID)?.version, '0.2.84');
    });

    it('skips a directory .obsolete marks for deletion', () => {
        const manifest = JSON.stringify([entry('0.2.83'), entry('0.2.84')]);
        const obsolete = JSON.stringify({ 'btakita.agent-doc-0.2.84': true, 'unrelated-1.0.0': true });
        assert.strictEqual(installedExtension(manifest, AGENT_DOC_EXTENSION_ID, obsolete)?.version, '0.2.83');
        assert.deepStrictEqual(
            [...obsoleteDirectories(obsolete)].sort(),
            ['btakita.agent-doc-0.2.84', 'unrelated-1.0.0'],
        );
    });

    it('answers nothing for a missing, malformed, or foreign manifest', () => {
        for (const manifest of [null, '', 'not json', '{}', JSON.stringify([entry('1.0.0', 'other.ext')])]) {
            assert.strictEqual(installedExtension(manifest, AGENT_DOC_EXTENSION_ID), null);
        }
        assert.strictEqual(
            installedExtension(JSON.stringify([{ ...entry('0.2.84'), version: 'next' }]), AGENT_DOC_EXTENSION_ID),
            null,
        );
        assert.strictEqual(obsoleteDirectories('not json').size, 0);
    });
});

describe('staleExtensionVerdict', () => {
    const installed = { version: '0.2.84', location: '/x/btakita.agent-doc-0.2.84' };

    it('needs a reload for any installed generation other than the running one', () => {
        assert.deepStrictEqual(staleExtensionVerdict('0.2.83', installed), {
            running: '0.2.83',
            installed: '0.2.84',
            location: '/x/btakita.agent-doc-0.2.84',
        });
        assert.strictEqual(
            staleExtensionVerdict('0.2.85', installed)?.installed,
            '0.2.84',
            'a downgrade also needs the reload',
        );
    });

    it('is silent when the running generation is installed or unknown', () => {
        assert.strictEqual(staleExtensionVerdict('0.2.84', installed), null);
        assert.strictEqual(staleExtensionVerdict('unknown', installed), null);
        assert.strictEqual(staleExtensionVerdict(undefined, installed), null);
        assert.strictEqual(staleExtensionVerdict('0.2.83', null), null);
    });
});

describe('upgradePrompt', () => {
    const verdict = { running: '0.2.83', installed: '0.2.84', location: null };

    it('offers Restart Extensions locally', () => {
        const prompt = upgradePrompt(verdict, { remoteName: undefined, webUi: false });
        assert.strictEqual(prompt.remedy, 'restart_extensions');
        assert.match(prompt.message, /v0\.2\.84 is installed, but this window is still running v0\.2\.83/);
        assert.deepStrictEqual(prompt.actions.map((action) => [action.label, action.command, action.args]), [
            ['Restart Extensions', 'extension.open', [AGENT_DOC_EXTENSION_ID]],
            ['Reload Window', 'workbench.action.reloadWindow', []],
        ]);
    });

    it('offers only Reload Window on a remote or web host', () => {
        const remote = upgradePrompt(verdict, { remoteName: 'ssh-remote', webUi: false });
        assert.strictEqual(remote.remedy, 'reload_window');
        assert.match(remote.message, /on the ssh-remote host/);
        assert.deepStrictEqual(remote.actions.map((action) => action.command), ['workbench.action.reloadWindow']);
        const web = upgradePrompt(verdict, { remoteName: undefined, webUi: true });
        assert.strictEqual(web.remedy, 'reload_window');
        assert.ok(needsWindowReload('dev-container', false));
        assert.ok(!needsWindowReload(undefined, false));
        assert.ok(!needsWindowReload('', false));
    });
});
