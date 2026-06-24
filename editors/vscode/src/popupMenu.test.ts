import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildOverflowPopupMenuItems, buildPrimaryPopupMenuItems } from './popupMenu';

describe('popupMenu', () => {
    it('keeps editor parity actions in the primary numbered menu', () => {
        const primary = buildPrimaryPopupMenuItems();
        const ids = primary.map(item => item.id);
        assert.deepStrictEqual(ids.slice(0, 12), [
            'submit',
            'claim',
            'fixDocument',
            'compactExchange',
            'syncLayout',
            'loadTmuxWindow',
            'status',
            'restartSupervisor',
            'restartAgent',
            'clear',
            'interruptClear',
            'doctor',
        ]);
        assert(primary.some(item => item.id === 'compactExchange'));
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
