import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildPrimaryPopupMenuItems, type PopupMenuActionId } from './popupMenu.js';

describe('popupMenu', () => {
    it('keeps editor parity actions in the primary numbered menu', () => {
        const primary = buildPrimaryPopupMenuItems();
        const ids = primary.map(item => item.id);
        assert.deepStrictEqual(ids.slice(0, 14), [
            'submit',
            'initSession',
            'claim',
            'fixDocument',
            'compactExchange',
            'syncLayout',
            'loadTmuxWindow',
            'status',
            'clear',
            'claimArtifact',
            'restartSupervisor',
            'restartAgent',
            'interruptClear',
            'doctor',
        ]);
        assert.strictEqual(primary[8]?.label, '[9] $(clear-all) Clear Session Context');
        assert.strictEqual(primary[9]?.label, '[10] $(link) Claim Artifact Session');
        assert.strictEqual(ids.filter(id => id === 'clear').length, 1);
        assert.strictEqual(ids.filter(id => id === 'claimArtifact').length, 1);
        assert(!primary.some(item => item.label.includes('More Actions')));
        assert(primary.some(item => item.id === 'compactExchange'));
        assert(primary.some(item => item.id === 'initSession'));
        assert(primary.some(item => item.id === 'restartSupervisor'));
        assert.deepStrictEqual(
            ids.slice(ids.indexOf('restartSupervisor'), ids.indexOf('restartSupervisor') + 2),
            ['restartSupervisor', 'restartAgent'],
        );
        const alsoOnFirstScreen: PopupMenuActionId[] = [
            'startAgentDoc',
            'runWithJunie',
            'forceClaim',
            'stopAgent',
            'cancelTurn',
            'killSupervisor',
            'resyncFixSessions',
            'gcStaleSessions',
            'about',
        ];
        for (const id of alsoOnFirstScreen) {
            assert(ids.includes(id), id);
        }
    });
});
