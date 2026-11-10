import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    appendPatchAlreadyPresent,
    calculateMinimalReplacement,
    isFullDocumentReplacement,
    isPureRepositionSignal,
    lineIndexAt,
    mapOffsetThroughReplacement,
    mapVisibleLineThroughReplacement,
} from './patchPlan.js';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'node:url';

// ESM has no `__dirname`; derive it from the module URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

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

    it('detects full-document replacements for visible write guards', () => {
        const fullReplacement = calculateMinimalReplacement('abcdef', 'UVWXYZ');
        const middleReplacement = calculateMinimalReplacement('alpha beta gamma', 'alpha BETA gamma');
        const emptyInsertion = calculateMinimalReplacement('', 'new content');

        assert.ok(fullReplacement);
        assert.strictEqual(isFullDocumentReplacement('abcdef', fullReplacement), true);
        assert.ok(middleReplacement);
        assert.strictEqual(isFullDocumentReplacement('alpha beta gamma', middleReplacement), false);
        assert.ok(emptyInsertion);
        assert.strictEqual(isFullDocumentReplacement('', emptyInsertion), false);
    });

    it('keeps a caret and viewport outside an agent write instead of jumping to it', () => {
        const before = [
            '<!-- agent:status -->',
            'ok',
            '<!-- /agent:status -->',
            '',
            '<!-- agent:queue -->',
            'operator is reading this',
            '<!-- /agent:queue -->',
            '',
        ].join('\n');
        const after = before.replace('ok', 'ok\nupdated');
        const edit = calculateMinimalReplacement(before, after);
        assert.ok(edit);
        const caret = before.indexOf('operator is reading this');
        const mapped = mapOffsetThroughReplacement(
            caret,
            edit.start,
            edit.start + edit.deleteLength,
            edit.text.length,
        );
        assert.strictEqual(after.slice(mapped, mapped + 'operator is reading this'.length), 'operator is reading this');
        const visibleLine = lineIndexAt(before, caret);
        const kept = mapVisibleLineThroughReplacement(visibleLine, before, edit);
        assert.notStrictEqual(kept, lineIndexAt(before, edit.start));
        assert.ok(kept > lineIndexAt(before, edit.start));
        assert.strictEqual(mapVisibleLineThroughReplacement(0, before, edit), 0);
    });

    it('applyMinimalTextEdit restores the caret and viewport and does not focus the document', () => {
        const source = fs.readFileSync(path.join(__dirname, '..', 'src', 'extension.ts'), 'utf8');
        const start = source.indexOf('private async applyMinimalTextEdit(');
        const end = source.indexOf('private async applyCrdtReplicaText(', start);
        assert.ok(start >= 0 && end > start);
        const body = source.slice(start, end);
        const edit = body.indexOf('workspace.applyEdit(edit)');
        const restoreCaret = body.indexOf('mapOffsetThroughReplacement(');
        const restoreScroll = body.indexOf('mapVisibleLineThroughReplacement(');
        assert.ok(edit >= 0);
        assert.ok(restoreCaret > edit);
        assert.ok(restoreScroll > edit);
        assert.strictEqual(body.includes('showTextDocument'), false);
        assert.strictEqual(body.includes('requestFocus'), false);
    });
});
