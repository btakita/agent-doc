function escapeRegex(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
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

    const trimmed = content.trimEnd();
    const newContent = `${trimmed}\n<!-- agent:boundary:${id} -->\n`;

    return doc.substring(0, contentStart) + newContent + doc.substring(closeIdx);
}
