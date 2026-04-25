import { describe, it } from 'node:test';
import assert from 'node:assert';
import { annotateExchangeHeadingsAgainstBaseline, repositionBoundaryToEnd, repositionBoundaryToEndPreserveHead } from './reposition';

describe('repositionBoundaryToEnd', () => {
    it('repositions boundary from middle to end', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            '### Response — opus-4-6',
            'Some content here.',
            '<!-- agent:boundary:abc12345 -->',
            'do something',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEnd(doc, 'exchange');
        assert.ok(result, 'should return repositioned content');
        assert.ok(result.includes('do something\n<!-- agent:boundary:'));
        assert.ok(result.endsWith('<!-- /agent:exchange -->'));
        assert.ok(!result.includes('abc12345'));
    });

    it('returns null when no boundary exists', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            'Some content.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEnd(doc, 'exchange');
        // No boundary markers exist — the function strips them all and re-adds,
        // but with no markers to strip, the content is unchanged
        assert.strictEqual(result, null);
    });

    it('removes old boundary and inserts fresh one at end', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            'First response.',
            '<!-- agent:boundary:aaa11111 -->',
            'Second response.',
            '<!-- agent:boundary:bbb22222 -->',
            'User input here.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEnd(doc, 'exchange');
        assert.ok(result, 'should return repositioned content');
        assert.ok(!result.includes('aaa11111'), 'old boundary 1 removed');
        assert.ok(!result.includes('bbb22222'), 'old boundary 2 removed');
        assert.ok(result.includes('First response.'));
        assert.ok(result.includes('Second response.'));
        assert.ok(result.includes('User input here.'));
        const boundaryCount = (result.match(/<!-- agent:boundary:[a-f0-9]+ -->/g) || []).length;
        assert.strictEqual(boundaryCount, 1, 'exactly one boundary');
    });

    it('returns null for nonexistent component', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            'Content.',
            '<!-- agent:boundary:abc12345 -->',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEnd(doc, 'nonexistent');
        assert.strictEqual(result, null);
    });

    it('handles component with attributes', () => {
        const doc = [
            '<!-- agent:exchange patch=append max_lines=50 -->',
            'Content here.',
            '<!-- agent:boundary:abc12345 -->',
            'New user input.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEnd(doc, 'exchange');
        assert.ok(result, 'should handle open tag with extra attributes');
        assert.ok(result.includes('New user input.\n<!-- agent:boundary:'));
    });

    it('strips transient HEAD markers while collapsing stale boundaries', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            '### Re: test — opus-4-6 (HEAD)',
            'Response.',
            '<!-- agent:boundary:aaa11111 -->',
            'User prompt.',
            '<!-- agent:boundary:bbb22222 -->',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEnd(doc, 'exchange');
        assert.ok(result, 'should return repositioned content');
        assert.ok(result.includes('### Re: test — opus-4-6\n'));
        assert.strictEqual((result.match(/\(HEAD\)/g) || []).length, 0, 'no HEAD markers remain');
        assert.strictEqual(
            (result.match(/<!-- agent:boundary:[a-f0-9]+ -->/g) || []).length,
            1,
            'exactly one boundary marker',
        );
        assert.ok(result.includes('User prompt.\n<!-- agent:boundary:'));
        assert.ok(!result.includes('aaa11111'));
        assert.ok(!result.includes('bbb22222'));
    });

    it('reuses the requested boundary id when provided', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            'Response.',
            '<!-- agent:boundary:aaa11111 -->',
            'User prompt.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEnd(doc, 'exchange', 'keep-this-id');
        assert.ok(result, 'should return repositioned content');
        assert.ok(result.includes('<!-- agent:boundary:keep-this-id -->'));
        assert.ok(!result.includes('aaa11111'));
        assert.strictEqual(
            (result.match(/<!-- agent:boundary:[a-z0-9-]+ -->/g) || []).length,
            1,
            'exactly one boundary marker',
        );
    });
});

describe('repositionBoundaryToEndPreserveHead', () => {
    it('keeps (HEAD) annotations while repositioning boundary', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            '### Re: test — opus-4-6 (HEAD)',
            'Response.',
            '<!-- agent:boundary:aaa11111 -->',
            'User prompt.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEndPreserveHead(doc, 'exchange');
        assert.ok(result, 'should return repositioned content');
        assert.ok(result.includes('### Re: test — opus-4-6 (HEAD)\n'), '(HEAD) annotation preserved');
        assert.ok(result.includes('User prompt.\n<!-- agent:boundary:'));
        assert.ok(!result.includes('aaa11111'), 'old boundary removed');
        assert.strictEqual(
            (result.match(/<!-- agent:boundary:[a-f0-9]+ -->/g) || []).length,
            1,
            'exactly one boundary marker',
        );
    });

    it('reuses the requested boundary id and keeps (HEAD)', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            '### Re: test — opus-4-6 (HEAD)',
            'Response.',
            '<!-- agent:boundary:aaa11111 -->',
            'User prompt.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = repositionBoundaryToEndPreserveHead(doc, 'exchange', 'keep-this-id');
        assert.ok(result, 'should return repositioned content');
        assert.ok(result.includes('(HEAD)'), '(HEAD) preserved');
        assert.ok(result.includes('<!-- agent:boundary:keep-this-id -->'));
        assert.ok(!result.includes('aaa11111'));
    });

    it('clean variant strips HEAD but preserve variant keeps it', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            '### Re: test — opus-4-6 (HEAD)',
            'Response.',
            '<!-- agent:boundary:aaa11111 -->',
            'User prompt.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const clean = repositionBoundaryToEnd(doc, 'exchange');
        const preserve = repositionBoundaryToEndPreserveHead(doc, 'exchange');
        assert.ok(clean, 'clean should return content');
        assert.ok(preserve, 'preserve should return content');
        assert.ok(!clean!.includes('(HEAD)'), 'clean strips (HEAD)');
        assert.ok(preserve!.includes('(HEAD)'), 'preserve keeps (HEAD)');
    });
});

describe('annotateExchangeHeadingsAgainstBaseline', () => {
    it('marks only newly patched response headings as transient HEAD', () => {
        const baseline = [
            '<!-- agent:exchange patch=append -->',
            '### Re: earlier — gpt-5',
            '',
            'Existing answer.',
            '<!-- /agent:exchange -->',
        ].join('\n');
        const doc = [
            '<!-- agent:exchange patch=append -->',
            '### Re: earlier — gpt-5',
            '',
            'Existing answer.',
            '### Re: latest — gpt-5',
            '',
            'Fresh answer.',
            '<!-- agent:boundary:abc12345 -->',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = annotateExchangeHeadingsAgainstBaseline(doc, 'exchange', baseline);
        assert.ok(result, 'should return annotated content');
        assert.ok(result!.includes('### Re: earlier — gpt-5\n'));
        assert.ok(result!.includes('### Re: latest — gpt-5 (HEAD)\n'));
        assert.strictEqual((result!.match(/\(HEAD\)/g) || []).length, 1);
    });

    it('does not add HEAD markers when exchange content is unchanged', () => {
        const doc = [
            '<!-- agent:exchange patch=append -->',
            '### Re: earlier — gpt-5',
            '',
            'Existing answer.',
            '<!-- /agent:exchange -->',
        ].join('\n');

        const result = annotateExchangeHeadingsAgainstBaseline(doc, 'exchange', doc);
        assert.strictEqual(result, doc);
        assert.strictEqual((result!.match(/\(HEAD\)/g) || []).length, 0);
    });
});
