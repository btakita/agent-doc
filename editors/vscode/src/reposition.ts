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

export function repositionBoundaryToEnd(doc: string, component: string): string | null {
    const openPattern = new RegExp(`<!-- agent:${escapeRegex(component)}(\\s[^>]*)? -->`);
    const closeTag = `<!-- /agent:${component} -->`;

    const openMatch = openPattern.exec(doc);
    if (!openMatch) return null;

    const closeIdx = doc.indexOf(closeTag, openMatch.index + openMatch[0].length);
    if (closeIdx < 0) return null;

    const contentStart = openMatch.index + openMatch[0].length;
    let content = doc.substring(contentStart, closeIdx);

    // If no boundary markers exist, nothing to reposition
    if (!/<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->/.test(content)) return null;

    content = content.replace(/<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->\n?/g, '');

    const id = Array.from({ length: 8 }, () =>
        Math.floor(Math.random() * 16).toString(16)
    ).join('');

    const trimmed = stripTransientHeadMarkers(content).trimEnd();
    const newContent = `${trimmed}\n<!-- agent:boundary:${id} -->\n`;

    return doc.substring(0, contentStart) + newContent + doc.substring(closeIdx);
}
