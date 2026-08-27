import { describe, it } from 'node:test';
import assert from 'node:assert';
import { NativeReloadGate } from './nativeReloadGate.js';

describe('NativeReloadGate', () => {
    it('coalesces reload intents onto one completion event', async () => {
        const gate = new NativeReloadGate();
        const owner = gate.begin();
        const follower = gate.begin();

        assert.equal(owner.owner, true);
        assert.equal(follower.owner, false);
        assert.equal(owner.completion, follower.completion);
        assert.equal(await gate.awaitReady(1), false);

        follower.complete(true);
        assert.equal(await gate.awaitReady(1), false);
        owner.complete(false);
        assert.equal(await follower.completion, false);
        assert.equal(await gate.awaitReady(1), true);
    });

    it('releases waiters when the owning handoff completes', async () => {
        const gate = new NativeReloadGate();
        const owner = gate.begin();
        const waiter = gate.awaitReady(1_000);

        owner.complete(true);

        assert.equal(await waiter, true);
        assert.equal(await owner.completion, true);
    });
});
