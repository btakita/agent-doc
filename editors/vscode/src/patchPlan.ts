export interface RepositionPatchShape {
    reposition_boundary?: boolean;
    patches?: unknown[];
    normalize_prefix_lines?: string[];
    unmatched?: string;
    frontmatter?: string;
    fullContent?: string;
}

export interface MinimalReplacement {
    start: number;
    deleteLength: number;
    text: string;
}

export function calculateMinimalReplacement(before: string, after: string): MinimalReplacement | null {
    if (before === after) {
        return null;
    }

    const minLen = Math.min(before.length, after.length);
    let prefixLen = 0;
    while (prefixLen < minLen && before.charCodeAt(prefixLen) === after.charCodeAt(prefixLen)) {
        prefixLen++;
    }

    let suffixLen = 0;
    while (
        suffixLen < before.length - prefixLen &&
        suffixLen < after.length - prefixLen &&
        before.charCodeAt(before.length - 1 - suffixLen) === after.charCodeAt(after.length - 1 - suffixLen)
    ) {
        suffixLen++;
    }

    return {
        start: prefixLen,
        deleteLength: before.length - prefixLen - suffixLen,
        text: after.substring(prefixLen, after.length - suffixLen),
    };
}

export function isFullDocumentReplacement(before: string, replacement: MinimalReplacement): boolean {
    return before.length > 0 && replacement.start === 0 && replacement.deleteLength === before.length;
}

export function isPureRepositionSignal(patch: RepositionPatchShape): boolean {
    if (!patch.reposition_boundary) {
        return false;
    }
    if ((patch.patches?.length ?? 0) > 0) {
        return false;
    }
    if ((patch.normalize_prefix_lines?.length ?? 0) > 0) {
        return false;
    }
    if ((patch.unmatched ?? '').trim() !== '') {
        return false;
    }
    if ((patch.frontmatter ?? '').trim() !== '') {
        return false;
    }
    if ((patch.fullContent ?? '') !== '') {
        return false;
    }
    return true;
}

export function appendPatchAlreadyPresent(doc: string, component: string, content: string): boolean {
    const range = findComponentRange(doc, component);
    if (!range) return false;
    const patch = normalizeAppendPatchContentForCompare(content);
    if (patch.length === 0) return false;
    const existing = normalizeAppendPatchContentForCompare(doc.substring(range[0], range[1]));
    return existing.includes(patch);
}

function normalizeAppendPatchContentForCompare(content: string): string {
    return stripTransientHeadMarkers(content)
        .split('\n')
        .filter(line => {
            const trimmed = line.trim();
            return !(trimmed.startsWith('<!-- agent:boundary:') && trimmed.endsWith(' -->'));
        })
        .join('\n')
        .trim();
}

function findComponentRange(doc: string, component: string): [number, number] | null {
    const openPattern = new RegExp(`<!-- agent:${escapeRegex(component)}(\\s[^>]*)? -->`, 'g');
    const closeTag = `<!-- /agent:${component} -->`;
    const codeRanges = findCodeBlockRanges(doc);

    let openMatch: RegExpExecArray | null = null;
    while ((openMatch = openPattern.exec(doc)) !== null) {
        const matchStart = openMatch.index;
        const insideCode = codeRanges.some(([start, end]) => matchStart >= start && matchStart < end);
        if (!insideCode) break;
    }
    if (!openMatch) return null;

    const contentStart = openMatch.index + openMatch[0].length;
    let searchFrom = contentStart;
    while (true) {
        const closeIdx = doc.indexOf(closeTag, searchFrom);
        if (closeIdx < 0) return null;
        const insideCode = codeRanges.some(([start, end]) => closeIdx >= start && closeIdx < end);
        if (!insideCode) return [contentStart, closeIdx];
        searchFrom = closeIdx + closeTag.length;
    }
}

function stripTransientHeadMarkers(content: string): string {
    const lines = content.split('\n');
    const result: string[] = [];
    let inFence = false;
    let fenceChar = '';
    let fenceLen = 0;

    for (const line of lines) {
        const trimmed = line.trimStart();
        const first = trimmed.charAt(0);
        const runLen = first ? leadingRunLength(trimmed, first) : 0;

        if (!inFence && (first === '`' || first === '~') && runLen >= 3) {
            inFence = true;
            fenceChar = first;
            fenceLen = runLen;
            result.push(line);
            continue;
        }
        if (inFence) {
            if (first === fenceChar && runLen >= fenceLen && trimmed.substring(runLen).trim() === '') {
                inFence = false;
            }
            result.push(line);
            continue;
        }

        if (/^\s*#{1,6}\s/.test(line) && line.endsWith(' (HEAD)')) {
            result.push(line.substring(0, line.length - ' (HEAD)'.length));
        } else {
            result.push(line);
        }
    }

    return result.join('\n');
}

function leadingRunLength(value: string, char: string): number {
    let count = 0;
    while (count < value.length && value.charAt(count) === char) count++;
    return count;
}

function findCodeBlockRanges(doc: string): Array<[number, number]> {
    const ranges: Array<[number, number]> = [];
    const fencePattern = /^[ \t]*```/gm;
    let insideFence = false;
    let fenceStart = 0;
    let match: RegExpExecArray | null;

    while ((match = fencePattern.exec(doc)) !== null) {
        if (!insideFence) {
            insideFence = true;
            fenceStart = match.index;
        } else {
            insideFence = false;
            const lineEnd = doc.indexOf('\n', match.index);
            ranges.push([fenceStart, lineEnd >= 0 ? lineEnd + 1 : doc.length]);
        }
    }
    if (insideFence) {
        ranges.push([fenceStart, doc.length]);
    }
    return ranges;
}

function escapeRegex(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
