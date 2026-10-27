import { describe, it } from 'node:test';
import assert from 'node:assert';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
    GROUP_LAYOUT_DERIVATION,
    deriveGroupLayout,
    deriveGroupLayoutFloor,
    type GroupLayoutEvidence,
} from './groupLayout.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

interface ParityCase {
    name: string;
    input: GroupLayoutEvidence;
    expected: Record<string, unknown>;
    /** A deliberate, documented divergence of the frozen floor from native. */
    floor_expected?: Record<string, unknown>;
}

function parityCases(): ParityCase[] {
    const fixturePath = path.join(
        __dirname,
        '..',
        'test-fixtures',
        'editor-derivations',
        'editor_group_layout.json',
    );
    const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf-8'));
    assert.strictEqual(fixture.derivation, GROUP_LAYOUT_DERIVATION);
    return fixture.cases as ParityCase[];
}

describe('editor_group_layout frozen floor', () => {
    it('answers every shared parity case the native derivation answers', () => {
        const cases = parityCases();
        assert.ok(cases.length >= 15, 'the parity file lost cases');
        for (const parity of cases) {
            const expected = parity.floor_expected ?? parity.expected;
            const answer = deriveGroupLayoutFloor(parity.input);
            assert.deepStrictEqual(
                answer.columns.map((column) => column.files),
                expected.columns,
                `${parity.name}: columns`,
            );
            for (const field of [
                'column_order',
                'source',
                'orientation',
                'position',
                'reason',
                'observed_line',
            ] as const) {
                assert.deepStrictEqual(answer[field], expected[field], `${parity.name}: ${field}`);
            }
        }
    });

    it('prefers the native answer and keeps the floor for a library that cannot answer', () => {
        const evidence = parityCases()[0].input;
        const nativeAnswer = {
            derivation: GROUP_LAYOUT_DERIVATION,
            output: {
                columns: [{ files: ['native.md'] }],
                column_order: 'editor',
                source: 'measured',
                observed_line: 'native',
            },
        };
        let request: unknown;
        const fromNative = deriveGroupLayout(evidence, (json) => {
            request = JSON.parse(json);
            return JSON.stringify(nativeAnswer);
        });
        assert.deepStrictEqual(request, { derivation: GROUP_LAYOUT_DERIVATION, input: evidence });
        assert.strictEqual(fromNative.native, true);
        assert.deepStrictEqual(fromNative.resolution.columns, [{ files: ['native.md'] }]);
        assert.strictEqual(fromNative.resolution.position, null);

        const floor = deriveGroupLayoutFloor(evidence);
        for (const unanswered of [
            () => null,
            () => 'not json',
            () => JSON.stringify({ derivation: 'capabilities', output: {} }),
            () => { throw new Error('unknown editor derivation'); },
        ]) {
            const answer = deriveGroupLayout(evidence, unanswered);
            assert.strictEqual(answer.native, false);
            assert.deepStrictEqual(answer.resolution, floor);
        }
        assert.deepStrictEqual(deriveGroupLayout(evidence).resolution, floor);
    });
});
