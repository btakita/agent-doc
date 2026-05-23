import { describe, it } from 'node:test';
import assert from 'node:assert';
import { appendPatchAlreadyPresent, isPureRepositionSignal } from './patchPlan';

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
