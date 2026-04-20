import { describe, it } from 'node:test';
import assert from 'node:assert';
import { repositionBoundaryToEnd } from './reposition';

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
});
