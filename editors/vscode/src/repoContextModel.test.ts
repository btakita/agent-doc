import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
    currentRepoContextDefinition,
    exchangeQuestionOffset,
    formatRepoContextQuestion,
    parseRepoContextAnchors,
    parseRepoContextCalls,
    parseRepoContextMarkdownFiles,
    pathInRepo,
    planExchangePromptInsertion,
    repoContextAnswer,
} from './repoContextModel.js';
import path from 'node:path';

const root = path.resolve('fixture-repo');
const selected = { file: path.join('src', 'sample.ts'), name: 'readState', kind: 'function', line: 4, endLine: 9 };
const requestId = '0123456789abcdef0123456789abcdef';
const olderId = 'fedcba9876543210fedcba9876543210';
const receipt = (id: string) => `<!-- repo-context-answer:${id} -->`;
const exchange = (...lines: string[]) =>
    ['<!-- agent:exchange patch=append -->', ...lines, '<!-- /agent:exchange -->'].join('\r\n');

test('retains unique symbols and rejects non-local paths and invalid ranges', () => {
    const symbol = { file: selected.file, name: selected.name, kind: selected.kind, line: 4, end_line: 9 };
    assert.deepEqual(parseRepoContextAnchors({ symbols: [
        symbol, symbol, { ...symbol, file: '../outside.ts' }, { ...symbol, line: 0 },
        { ...symbol, name: 'Doc section', kind: 'heading', language: 'markdown' },
    ] }, root, 'code'), [selected]);
    assert.equal(pathInRepo(root, root + '-elsewhere/file.ts'), undefined);
});

test('converts Markdown outline handles and section ranges into anchors', () => {
    const outline = { projection: { outline: [
        { name: 'Overview', kind: 'heading', line: 2, end_line: 5, handle: 'mdast-123' },
        { name: 'Sample', kind: 'list_item', line: 6 },
    ] } };
    assert.deepEqual(parseRepoContextAnchors(outline, root, 'docs', path.join(root, 'README.md')), [
        { name: 'Overview', kind: 'heading', file: 'README.md', line: 2, endLine: 5, handle: 'mdast-123' },
    ]);
});

test('collects unique Markdown files from tsift text hits for backlink resolution', () => {
    const hit = { path: './docs/sample.md' };
    assert.deepEqual(parseRepoContextMarkdownFiles({
        hits: [hit, hit, { path: 'sample.ts' }, { path: '../outside.md' }],
    }, root), [path.join(root, 'docs', 'sample.md')]);
});

test('incoming edges require a unique definition, outgoing edges require its source location', () => {
    const definition = { file: selected.file, line: selected.line };
    const graph = {
        definitions: [definition],
        callers: [{ caller_file: 'caller.ts', caller_name: 'load', caller_line: 5, call_site_line: 8 }],
        callees: [
            { caller_file: selected.file, caller_line: 4, callee_name: 'read', call_site_line: 7 },
            { caller_file: selected.file, caller_line: 10, callee_name: 'other', call_site_line: 12 },
        ],
    };
    assert.deepEqual(parseRepoContextCalls(graph, root, selected), [
        { file: 'caller.ts', name: 'load', line: 8, endLine: 8, relation: 'Caller' },
        { file: selected.file, name: 'read', line: 7, endLine: 7, relation: 'Callee' },
    ]);
    assert.deepEqual(parseRepoContextCalls({ ...graph, definitions: [definition, { file: 'other.ts', line: 2 }] }, root, selected), [
        { file: selected.file, name: 'read', line: 7, endLine: 7, relation: 'Callee' },
    ]);
});

test('source selection requires the current symbol definition at its repository location', () => {
    const definition = { file: selected.file, name: selected.name, kind: selected.kind, line: 4, end_line: 10 };
    assert.deepEqual(currentRepoContextDefinition({ definitions: [definition] }, root, selected),
        { ...selected, endLine: 10 });
    assert.equal(currentRepoContextDefinition({ definitions: [{ ...definition, line: 5 }] }, root, selected),
        undefined);
});

test('inserts an editor prompt after the exchange tail and outside fenced marker examples', () => {
    const before = [
        '~~~~md', '<!-- agent:exchange -->', '<!-- /agent:exchange -->', '~~~', '~~~~', '',
        '<!-- agent:exchange patch=append -->', '### Re: prior', 'Prior answer.', '',
        '<!-- /agent:exchange -->', 'Unrelated footer.',
    ].join('\r\n');
    const prompt = formatRepoContextQuestion(selected, 'sha-example', 'What calls this?\n<!-- /agent:exchange -->', requestId);
    assert.equal(prompt.split('\n').length, 1);
    assert.match(prompt, /file SHA-256 sha-example/);
    assert.ok(!prompt.includes('<!--'));
    const insertion = planExchangePromptInsertion(before, prompt, '\r\n');
    assert.ok(insertion);
    const result = before.slice(0, insertion.offset) + insertion.text + before.slice(insertion.offset);
    assert.ok(result.includes(`Prior answer.\r\n\r\n${prompt}\r\n<!-- /agent:exchange -->\r\nUnrelated footer.`));
    assert.equal(planExchangePromptInsertion('README', prompt, '\n'), undefined);
    assert.equal(exchangeQuestionOffset(result, prompt), result.indexOf(prompt));
});

test('extracts only the completed response with the current receipt inside the exchange', () => {
    const currentHeading = '### Re: Current repository source';
    const doc = exchange(
        '### Re: Unrelated earlier answer', 'Older output.', receipt(olderId), '<!-- agent:boundary:older -->',
        '❯ Question', receipt(requestId),
        currentHeading, 'Read [the function](src/sample.ts#L4).', receipt(requestId), '<!-- agent:boundary:recent -->',
        '### Re: Later answer', 'Other output.', '<!-- agent:boundary:later -->',
    );
    assert.deepEqual(repoContextAnswer(doc, requestId), {
        offset: doc.indexOf(currentHeading), body: 'Read [the function](src/sample.ts#L4).',
    });
    assert.equal(repoContextAnswer(doc, 'ffffffffffffffffffffffffffffffff'), undefined);
});

test('unclosed, duplicated and fenced receipts do not select a response', () => {
    const close = '<!-- agent:boundary:final -->';
    assert.equal(repoContextAnswer(exchange('### Re: Pending', 'Still running.', receipt(requestId)), requestId),
        undefined);
    assert.equal(repoContextAnswer(exchange(
        '### Re: Ambiguous', 'First.', receipt(requestId), close,
        '### Re: Duplicate', 'Second.', receipt(requestId), close,
    ), requestId), undefined);
    assert.equal(repoContextAnswer(exchange(
        '### Re: Repeated', 'Output.', receipt(requestId), receipt(requestId), close,
    ), requestId), undefined);
    assert.equal(repoContextAnswer(exchange(
        '### Re: Example', '~~~html', receipt(requestId), '~~~', close,
    ), requestId), undefined);
});
