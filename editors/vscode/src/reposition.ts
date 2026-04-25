function escapeRegex(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function stripTransientHeadMarkers(content: string): string {
    const lines = content.split('\n');
    let inFence = false;
    let fenceChar = '';
    let fenceLen = 0;

    return lines.map((line) => {
        const trimmed = line.trimStart();
        const first = trimmed[0] ?? '';
        const runLen = first ? trimmed.match(new RegExp(`^\\${first}+`))?.[0].length ?? 0 : 0;

        if (!inFence && (first === '`' || first === '~') && runLen >= 3) {
            inFence = true;
            fenceChar = first;
            fenceLen = runLen;
            return line;
        }
        if (inFence) {
            if (first === fenceChar && runLen >= fenceLen && trimmed.slice(runLen).trim() === '') {
                inFence = false;
            }
            return line;
        }

        if (/^\s*#{1,6}\s/.test(line) && line.endsWith(' (HEAD)')) {
            return line.slice(0, -' (HEAD)'.length);
        }
        return line;
    }).join('\n');
}

function findComponentRange(doc: string, component: string): { contentStart: number; closeIdx: number } | null {
    const openPattern = new RegExp(`<!-- agent:${escapeRegex(component)}(\\s[^>]*)? -->`, 'g');
    const closeTag = `<!-- /agent:${component} -->`;
    const codeRanges = findCodeBlockRanges(doc);

    let openMatch: RegExpExecArray | null = null;
    while ((openMatch = openPattern.exec(doc)) !== null) {
        if (!codeRanges.some(([start, end]) => openMatch!.index >= start && openMatch!.index < end)) {
            break;
        }
    }
    if (!openMatch) return null;

    const contentStart = openMatch.index + openMatch[0].length;
    let closeIdx = -1;
    let searchFrom = contentStart;
    while (true) {
        closeIdx = doc.indexOf(closeTag, searchFrom);
        if (closeIdx < 0) return null;
        if (!codeRanges.some(([start, end]) => closeIdx >= start && closeIdx < end)) {
            break;
        }
        searchFrom = closeIdx + closeTag.length;
    }

    return { contentStart, closeIdx };
}

function findCodeBlockRanges(doc: string): Array<[number, number]> {
    const ranges: Array<[number, number]> = [];
    const fencePattern = /^[ \t]*```/gm;
    let insideFence = false;
    let fenceStart = 0;
    let match: RegExpExecArray | null;

    while ((match = fencePattern.exec(doc)) !== null) {
        if (!insideFence) {
            fenceStart = match.index;
            insideFence = true;
        } else {
            const lineEnd = doc.indexOf('\n', match.index + match[0].length);
            const blockEnd = lineEnd >= 0 ? lineEnd + 1 : doc.length;
            ranges.push([fenceStart, blockEnd]);
            insideFence = false;
        }
    }

    return ranges;
}

function collectReHeadings(content: string): Set<string> {
    const codeRanges = findCodeBlockRanges(content);
    const headings = new Set<string>();
    let offset = 0;

    for (const line of content.split('\n')) {
        const inCode = codeRanges.some(([start, end]) => offset >= start && offset < end);
        if (!inCode) {
            const trimmed = line.trimStart();
            const hashCount = trimmed.match(/^#+/)?.[0].length ?? 0;
            const afterHash = trimmed.slice(hashCount);
            if (hashCount > 0 && hashCount <= 6 && afterHash.startsWith(' ') && afterHash.trimStart().startsWith('Re:')) {
                headings.add(line.trimStart().trimEnd().replace(/ \(HEAD\)$/, ''));
            }
        }
        offset += line.length + 1;
    }

    return headings;
}

function annotateReHeadingsWithHead(content: string, baseline: Set<string>): string {
    const codeRanges = findCodeBlockRanges(content);
    const lines = content.split('\n');
    const reIndices: number[] = [];
    let offset = 0;

    for (let idx = 0; idx < lines.length; idx += 1) {
        const line = lines[idx];
        const inCode = codeRanges.some(([start, end]) => offset >= start && offset < end);
        offset += line.length + 1;
        if (inCode) continue;

        const trimmed = line.trimStart();
        const hashCount = trimmed.match(/^#+/)?.[0].length ?? 0;
        const afterHash = trimmed.slice(hashCount);
        if (hashCount === 0 || hashCount > 6 || !afterHash.startsWith(' ') || !afterHash.trimStart().startsWith('Re:')) {
            continue;
        }

        lines[idx] = line.trimEnd().replace(/ \(HEAD\)$/, '');
        reIndices.push(idx);
    }

    const markIndices = reIndices.filter((idx) => !baseline.has(lines[idx].trimStart().trimEnd()));
    const finalIndices = markIndices.length > 0 ? markIndices : reIndices.slice(-1);
    for (const idx of finalIndices) {
        lines[idx] = `${lines[idx]} (HEAD)`;
    }

    return lines.join('\n');
}

export function annotateExchangeHeadingsAgainstBaseline(
    doc: string,
    component: string,
    baselineDoc: string,
): string | null {
    const currentRange = findComponentRange(doc, component);
    if (!currentRange) return null;
    const baselineRange = findComponentRange(baselineDoc, component);

    const currentContent = doc.substring(currentRange.contentStart, currentRange.closeIdx);
    const baselineContent = baselineRange
        ? baselineDoc.substring(baselineRange.contentStart, baselineRange.closeIdx)
        : '';
    const normalizeForCompare = (value: string) =>
        stripTransientHeadMarkers(value)
            .split('\n')
            .filter((line) => !line.trim().startsWith('<!-- agent:boundary:'))
            .join('\n');
    if (normalizeForCompare(currentContent) === normalizeForCompare(baselineContent)) {
        return doc;
    }

    const annotated = annotateReHeadingsWithHead(currentContent, collectReHeadings(baselineContent));
    if (annotated === currentContent) {
        return doc;
    }

    return doc.substring(0, currentRange.contentStart) + annotated + doc.substring(currentRange.closeIdx);
}

export function repositionBoundaryToEnd(doc: string, component: string, boundaryId?: string): string | null {
    const range = findComponentRange(doc, component);
    if (!range) return null;

    let content = doc.substring(range.contentStart, range.closeIdx);

    // If no boundary markers exist, nothing to reposition
    if (!/<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->/.test(content)) return null;

    content = content.replace(/<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->\n?/g, '');

    const id = boundaryId ?? Array.from({ length: 8 }, () =>
        Math.floor(Math.random() * 16).toString(16)
    ).join('');

    const trimmed = stripTransientHeadMarkers(content).trimEnd();
    const newContent = `${trimmed}\n<!-- agent:boundary:${id} -->\n`;

    return doc.substring(0, range.contentStart) + newContent + doc.substring(range.closeIdx);
}
