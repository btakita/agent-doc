import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildOverflowPopupMenuItems, buildPrimaryPopupMenuItems } from './popupMenu.js';

describe('popupMenu', () => {
    it('keeps editor parity actions in the primary numbered menu', () => {
        const primary = buildPrimaryPopupMenuItems();
        const ids = primary.map(item => item.id);
        assert.deepStrictEqual(ids.slice(0, 13), [
            'submit',
            'initSession',
            'claim',
            'fixDocument',
            'compactExchange',
            'syncLayout',
            'loadTmuxWindow',
            'status',
            'clear',
            'restartSupervisor',
            'restartAgent',
            'interruptClear',
            'doctor',
        ]);
        assert.strictEqual(primary[8]?.label, '[9] $(clear-all) Clear Session Context');
        assert.strictEqual(ids.filter(id => id === 'clear').length, 1);
        assert(primary.some(item => item.id === 'compactExchange'));
        assert(primary.some(item => item.id === 'initSession'));
        assert(primary.some(item => item.id === 'restartSupervisor'));
        assert.deepStrictEqual(
            ids.slice(ids.indexOf('restartSupervisor'), ids.indexOf('restartSupervisor') + 2),
            ['restartSupervisor', 'restartAgent'],
        );
        assert(!primary.some(item => item.id === 'runWithJunie'));
        assert(!primary.some(item => item.id === 'forceClaim'));
        assert(!primary.some(item => item.id === 'stopAgent'));
    });

    it('keeps lower-frequency operator actions in the overflow menu', () => {
        assert.deepStrictEqual(
            buildOverflowPopupMenuItems().map(item => item.id),
            [
                'runWithJunie',
                'forceClaim',
                'stopAgent',
                'cancelTurn',
                'killSupervisor',
                'resyncFixSessions',
                'gcStaleSessions',
            ],
        );
    });
});
