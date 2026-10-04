import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';
import { DASHBOARD_COMMAND_ARGS, dashboardPath } from './dashboard.js';
import { buildPrimaryPopupMenuItems } from './popupMenu.js';

const packageJson = JSON.parse(
    fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'package.json'), 'utf8'),
);

describe('dashboard', () => {
    it('writes the controller-owned default projection', () => {
        assert.deepStrictEqual([...DASHBOARD_COMMAND_ARGS], ['dashboard', '--write']);
        assert.strictEqual(
            dashboardPath('/w/agent-loop'),
            path.join('/w/agent-loop', '.agent-doc', 'dashboard.md'),
        );
    });

    it('is a palette command that is not markdown gated', () => {
        const command = packageJson.contributes.commands.find(
            (entry: { command: string }) => entry.command === 'agentDoc.dashboard',
        );
        assert.strictEqual(command?.title, 'Agent Doc: Dashboard');
        const paletteGate = packageJson.contributes.menus.commandPalette.find(
            (entry: { command: string }) => entry.command === 'agentDoc.dashboard',
        );
        assert.strictEqual(paletteGate, undefined, 'dashboard must be available from any editor');
    });

    it('is reachable from the Agent Doc popup menu', () => {
        assert(buildPrimaryPopupMenuItems().some(item => item.id === 'dashboard'));
    });
});
