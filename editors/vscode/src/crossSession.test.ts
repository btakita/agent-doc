import { describe, it } from 'node:test';
import assert from 'node:assert';
import { parseCrossSessionReject } from './crossSession.js';

describe('parseCrossSessionReject', () => {
    it('parses the marker from merged claim output', () => {
        const output = [
            '[claim] cross-session-reject pane_id=%43 pane_session=5 configured=0',
            "Error: pane %43 is in tmux session '5' but project session is '0'; switch to the configured session or pass --force",
        ].join('\n');
        assert.deepStrictEqual(parseCrossSessionReject(output), {
            paneId: '%43',
            paneSession: '5',
            configured: '0',
        });
    });

    it('does not assume field order', () => {
        const reject = parseCrossSessionReject(
            '[claim] cross-session-reject configured=main pane_session=work pane_id=%7',
        );
        assert.deepStrictEqual(reject, { paneId: '%7', paneSession: 'work', configured: 'main' });
    });

    it('returns undefined without a marker', () => {
        assert.strictEqual(parseCrossSessionReject('Claimed plan.md'), undefined);
        assert.strictEqual(parseCrossSessionReject('Error: some other failure'), undefined);
    });

    it('returns undefined when a field is missing', () => {
        assert.strictEqual(
            parseCrossSessionReject('[claim] cross-session-reject pane_id=%1 pane_session=2'),
            undefined,
        );
    });
});
