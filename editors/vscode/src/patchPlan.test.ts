import { describe, it } from 'node:test';
import assert from 'node:assert';
import { appendPatchAlreadyPresent, calculateMinimalReplacement, isPureRepositionSignal } from './patchPlan';

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

    it('keeps full-content repair payloads off the pure reposition path', () => {
        assert.strictEqual(
            isPureRepositionSignal({
                reposition_boundary: true,
                patches: [],
                unmatched: '',
                fullContent: 'replacement',
            }),
            false,
        );
    });
});

describe('appendPatchAlreadyPresent', () => {
    it('detects a replayed exchange response despite a transient HEAD marker', () => {
        const doc = `<!-- agent:exchange patch=append -->
### Re: Duplicate — gpt-5 (HEAD)

Already applied.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
`;

        assert.strictEqual(
            appendPatchAlreadyPresent(
                doc,
                'exchange',
                '### Re: Duplicate — gpt-5\n\nAlready applied.\n',
            ),
            true,
        );
    });

    it('ignores boundary markers while comparing append patches', () => {
        const doc = `<!-- agent:exchange patch=append -->
### Re: Duplicate — gpt-5

Already applied.
<!-- agent:boundary:abc12345 -->
<!-- /agent:exchange -->
`;

        assert.strictEqual(
            appendPatchAlreadyPresent(
                doc,
                'exchange',
                '### Re: Duplicate — gpt-5\n\nAlready applied.\n<!-- agent:boundary:other -->\n',
            ),
            true,
        );
    });

    it('does not treat missing append content as present', () => {
        const doc = `<!-- agent:exchange patch=append -->
Different content.
<!-- /agent:exchange -->
`;

        assert.strictEqual(
            appendPatchAlreadyPresent(
                doc,
                'exchange',
                '### Re: Duplicate — gpt-5\n\nAlready applied.\n',
            ),
            false,
        );
    });
});

describe('calculateMinimalReplacement', () => {
    it('returns null when content is unchanged', () => {
        assert.strictEqual(calculateMinimalReplacement('same', 'same'), null);
    });

    it('collapses a middle replacement to the changed span', () => {
        assert.deepStrictEqual(
            calculateMinimalReplacement('alpha beta gamma', 'alpha BETA gamma'),
            { start: 6, deleteLength: 4, text: 'BETA' },
        );
    });

    it('handles insertion and deletion without replacing the whole document', () => {
        assert.deepStrictEqual(
            calculateMinimalReplacement('alpha gamma', 'alpha beta gamma'),
            { start: 6, deleteLength: 0, text: 'beta ' },
        );
        assert.deepStrictEqual(
            calculateMinimalReplacement('alpha beta gamma', 'alpha gamma'),
            { start: 6, deleteLength: 5, text: '' },
        );
    });
});
