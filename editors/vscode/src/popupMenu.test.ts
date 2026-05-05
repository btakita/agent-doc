import { describe, it } from 'node:test';
import assert from 'node:assert';
import { buildOverflowPopupMenuItems, buildPrimaryPopupMenuItems } from './popupMenu';

describe('popupMenu', () => {
    it('keeps compact exchange and supervisor restart in the primary numbered menu', () => {
        const primary = buildPrimaryPopupMenuItems();
        assert(primary.some(item => item.id === 'compactExchange'));
        assert(primary.some(item => item.id === 'restartSupervisor'));
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
