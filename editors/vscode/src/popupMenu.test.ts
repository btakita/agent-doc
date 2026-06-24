import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildOverflowPopupMenuItems, buildPrimaryPopupMenuItems } from './popupMenu';

describe('popupMenu', () => {
    it('keeps compact exchange and supervisor restart in the primary numbered menu', () => {
        const primary = buildPrimaryPopupMenuItems();
        const ids = primary.map(item => item.id);
        assert(primary.some(item => item.id === 'compactExchange'));
        assert(primary.some(item => item.id === 'restartSupervisor'));
        assert.deepStrictEqual(
            ids.slice(ids.indexOf('restartSupervisor'), ids.indexOf('restartSupervisor') + 2),
            ['restartSupervisor', 'restartAgent'],
        );
        assert(!primary.some(item => item.id === 'runWithJunie'));
        assert(!primary.some(item => item.id === 'forceClaim'));
    });

    it('keeps Junie and force claim in the overflow menu', () => {
        assert.deepStrictEqual(
            buildOverflowPopupMenuItems().map(item => item.id),
            ['runWithJunie', 'forceClaim'],
        );
    });
});
