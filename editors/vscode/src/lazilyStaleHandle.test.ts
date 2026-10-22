import { describe, it } from 'node:test';
import assert from 'node:assert';
import { createContext } from '@lazily-hub/lazily-js/reactive';

/**
 * Consumer proof for lazily-spec's `dispose_stale_handle` contract.
 *
 * The extension bundles lazily-js, so its lockfile must retain the generation-aware
 * handle implementation: a handle from an earlier allocation may share an id with
 * a successor, but it never owns that successor's teardown authority.
 */
describe('lazily-js stale-handle conformance', () => {
    it('keeps a recycled Source alive when its stale predecessor is disposed again', () => {
        const context = createContext();
        const stale = context.source('old');
        context.disposeCell(stale);

        const current = context.source('current');
        assert.equal(current.id, stale.id, 'the fixture exercises same-kind id recycling');

        context.disposeCell(stale);
        assert.equal(context.get(current), 'current');
        context.set(current, 'updated');
        assert.equal(context.get(current), 'updated');
    });

    it('keeps a recycled Effect active when its stale predecessor is disposed again', () => {
        const context = createContext();
        const input = context.source(0);
        const cleanups: string[] = [];
        const stale = context.effect(() => () => cleanups.push('stale'));
        context.disposeEffect(stale);

        const current = context.effect((compute) => {
            compute.get(input);
            return () => cleanups.push('current');
        });
        assert.equal(current.id, stale.id, 'the fixture exercises same-kind id recycling');

        context.disposeEffect(stale);
        assert.equal(context.isEffectActive(stale), false);
        assert.equal(context.isEffectActive(current), true);
        assert.deepEqual(cleanups, ['stale']);

        context.set(input, 1);
        assert.deepEqual(cleanups, ['stale', 'current']);
    });
});
