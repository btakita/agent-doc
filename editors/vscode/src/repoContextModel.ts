import * as path from 'node:path';
import { findCodeBlockRanges, findComponentRange } from './patchPlan.js';

export type RepoContextMode = 'code' | 'docs';

export interface SourceLocation {
    file: string;
    line: number;
    endLine: number;
}

export interface RepoContextAnchor extends SourceLocation {
    name: string;
    kind: string;
    handle?: string;
}

export interface RepoContextLink extends SourceLocation {
    name: string;
    relation: string;
}

function record(value: unknown): Record<string, unknown> {
    return value !== null && typeof value === 'object' ? value as Record<string, unknown> : {};
}

function entries(value: unknown): Record<string, unknown>[] {
    return Array.isArray(value) ? value.map(record) : [];
}

function lineNumber(value: unknown): number | undefined {
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 1 ? value : undefined;
}

export function pathInRepo(root: string, file: string): string | undefined {
    if (!file || file.includes('\0')) return undefined;
    const absolute = path.resolve(root, file);
    const relative = path.relative(root, absolute);
    return relative !== '' && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)
        ? absolute : undefined;
}

export function parseRepoContextAnchors(
    output: unknown,
    root: string,
    mode: RepoContextMode,
    outlineFile?: string,
): RepoContextAnchor[] {
    const data = record(output);
    const rows = outlineFile ? entries(record(data.projection).outline) : entries(data.symbols);
    const seen = new Set<string>();
    const anchors: RepoContextAnchor[] = [];
    for (const row of rows) {
        if (typeof row.name !== 'string' || !row.name || typeof row.kind !== 'string') continue;
        if (mode === 'docs' ? row.kind !== 'heading' : row.language === 'markdown') continue;
        const file = outlineFile ?? row.file;
        const absolute = typeof file === 'string' ? pathInRepo(root, file) : undefined;
        const line = lineNumber(row.line);
        const endLine = lineNumber(row.end_line) ?? line;
        if (!absolute || line === undefined || endLine === undefined || endLine < line) continue;
        const location = path.relative(root, absolute);
        const key = JSON.stringify([location, line, row.name]);
        if (seen.has(key)) continue;
        seen.add(key);
        anchors.push({
            file: location,
            line,
            endLine,
            name: row.name,
            kind: row.kind,
            ...(typeof row.handle === 'string' ? { handle: row.handle } : {}),
        });
    }
    return anchors;
}

export function parseRepoContextCalls(
    output: unknown,
    root: string,
    anchor: RepoContextAnchor,
): RepoContextLink[] {
    const { callers, callees, definitions } = record(output);
    const sameDefinition = (row: Record<string, unknown>) =>
        typeof row.file === 'string' && pathInRepo(root, row.file) === pathInRepo(root, anchor.file)
        && row.line === anchor.line;
    // tsift reports incoming edges by name. An overloaded name does not prove
    // which declaration an incoming edge targets.
    const definitionsForName = entries(definitions);
    const unambiguous = definitionsForName.length === 1 && sameDefinition(definitionsForName[0]);
    const results: RepoContextLink[] = [];
    const seen = new Set<string>();

    for (const relation of ['Caller', 'Callee'] as const) {
        if (relation === 'Caller' && !unambiguous) continue;
        for (const edge of entries(relation === 'Caller' ? callers : callees)) {
            const { caller_file: file, caller_line, call_site_line } = edge;
            if (relation === 'Callee' && !sameDefinition({ file, line: caller_line })) continue;
            const absolute = typeof file === 'string' ? pathInRepo(root, file) : undefined;
            const line = lineNumber(call_site_line);
            const name = relation === 'Caller' ? edge.caller_name : edge.callee_name;
            if (!absolute || line === undefined || typeof name !== 'string' || !name) continue;
            const location = path.relative(root, absolute);
            const key = JSON.stringify([relation, name, location, line]);
            if (seen.has(key)) continue;
            seen.add(key);
            results.push({ file: location, line, endLine: line, name, relation });
        }
    }
    return results;
}

export function currentRepoContextDefinition(
    output: unknown,
    root: string,
    anchor: RepoContextAnchor,
): RepoContextAnchor | undefined {
    const definitions = parseRepoContextAnchors({ symbols: record(output).definitions }, root, 'code');
    return definitions.find(item => item.file === anchor.file && item.line === anchor.line
        && item.kind === anchor.kind && item.name === anchor.name);
}

export function parseRepoContextMarkdownFiles(output: unknown, root: string): string[] {
    const files = new Set<string>();
    for (const hit of entries(record(output).hits)) {
        if (typeof hit.path !== 'string' || !/\.(md|mdx|markdown)$/i.test(hit.path)) continue;
        const file = pathInRepo(root, hit.path);
        if (file) files.add(file);
    }
    return [...files];
}

export function formatRepoContextQuestion(
    anchor: RepoContextAnchor,
    revision: string,
    question: string,
    requestId: string,
): string {
    // Keep the operator's question and repository labels in a single exchange
    // prompt. Markdown comment delimiters would be document control syntax.
    const inline = (text: string): string => text.replace(/\s+/gu, ' ').trim()
        .split('<!--').join('&lt;!--').split('-->').join('--&gt;');
    const range = anchor.line === anchor.endLine ? `${anchor.line}` : `${anchor.line}-${anchor.endLine}`;
    const handle = anchor.handle ? `, source handle ${inline(anchor.handle)}` : '';
    return `❯ Document Context Explorer prompt about ${inline(anchor.file)}:${range} (${inline(anchor.kind)} ${inline(anchor.name)}${handle}, file SHA-256 ${revision}): ${inline(question)} Please cite the source locations in your answer and include a standalone HTML comment whose text is repo-context-answer:${requestId} in the matching response.`;
}

export function planExchangePromptInsertion(
    doc: string,
    prompt: string,
    newline: string,
): { offset: number; text: string } | undefined {
    const component = findComponentRange(doc, 'exchange');
    if (!component) return undefined;
    const [start, end] = component;
    const body = doc.slice(start, end);
    const padding = body.endsWith(newline + newline) ? '' : body.endsWith(newline) ? newline : newline + newline;
    return { offset: end, text: padding + prompt + newline };
}

export function exchangeQuestionOffset(doc: string, question: string): number | undefined {
    const range = findComponentRange(doc, 'exchange');
    if (!range) return undefined;
    const exchange = doc.slice(...range);
    const fenced = findCodeBlockRanges(exchange);
    let offset = 0;
    let found: number | undefined;
    for (const line of exchange.split('\n')) {
        if (line.trimEnd() === question && !fenced.some(([begin, end]) => offset >= begin && offset < end)) {
            if (found !== undefined) return undefined;
            found = range[0] + offset;
        }
        offset += line.length + 1;
    }
    return found;
}

export function repoContextAnswer(
    doc: string,
    requestId: string,
): { offset: number; body: string } | undefined {
    if (!/^[a-f0-9]{32}$/.test(requestId)) return undefined;
    const range = findComponentRange(doc, 'exchange');
    if (!range) return undefined;
    const exchange = doc.slice(...range);
    const fenced = findCodeBlockRanges(exchange);
    const separators = /^### Re:[ \t]+[^\r\n]*|^❯[ \t]+[^\r\n]*|^<!-- agent:boundary:[a-z0-9:-]+ -->[ \t]*(?=\r?$)|^<!--[ \t]*repo-context-answer:([a-f0-9]{32})[ \t]*-->[ \t]*(?=\r?$)/gm;
    let heading: RegExpExecArray | undefined;
    let receipt: RegExpExecArray | undefined;
    let answer: { offset: number; body: string } | undefined;
    let match: RegExpExecArray | null;
    while ((match = separators.exec(exchange)) !== null) {
        const index = match.index;
        if (fenced.some(([begin, end]) => index >= begin && index < end)) continue;
        if (match[1] !== undefined) {
            if (match[1] !== requestId) continue;
            if (heading && receipt) return undefined;
            if (heading) receipt = match;
            continue;
        }
        if (heading && receipt) {
            if (answer) return undefined;
            const body = (
                exchange.slice(heading.index + heading[0].length, receipt.index)
                + exchange.slice(receipt.index + receipt[0].length, index)
            ).trim();
            if (!body) return undefined;
            answer = { offset: range[0] + heading.index, body };
        }
        heading = match[0].startsWith('### Re:') ? match : undefined;
        receipt = undefined;
    }
    // A receipt on a still-open tail does not yet prove a complete response.
    return receipt ? undefined : answer;
}
