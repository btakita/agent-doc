import { describe, it } from 'node:test';
import assert from 'node:assert';
import { isPureRepositionSignal } from './patchPlan';

describe('isPureRepositionSignal', () => {
    it('treats a bare reposition payload as pure reposition', () => {
        assert.strictEqual(
            isPureRepositionSignal({
                reposition_boundary: true,
                patches: [],
                unmatched: '',
            }),
            true,
        );
    });

    it('keeps normalize-only repair payloads on the full patch path', () => {
        assert.strictEqual(
            isPureRepositionSignal({
                reposition_boundary: true,
                patches: [],
                unmatched: '',
                normalize_prefix_lines: ['do #tailpatch. spec-test-build-install-commit-push'],
            }),
            false,
        );
    });
});
