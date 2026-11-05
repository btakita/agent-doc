/**
 * `#queuetreegutter`: VS Code presentation of the native queue tree
 * (`agent_doc_queue_tree_json`). The binary owns every node's state, glyph and
 * tooltip (FFI-first); this module only turns its 1-based lines into gutter
 * icons and fold ranges. Pure: no `vscode` import, so `node --test` covers it.
 */

/** One node of `agent_doc_queue_tree_json` (the fields the editor reads). */
export interface QueueTreeNode {
    /** 1-based document line of the row. */
    line: number;
    /** 1-based last line of the row's subtree (inclusive). */
    end_line: number;
    depth: number;
    state: string;
    glyph: string;
    text?: string;
    id?: string;
    closed: boolean;
    rollup?: { you: number; agent: number; done: number };
    /** Native tooltip; an older library without it falls back to the state label. */
    tooltip?: string;
}

/** A closed subtree's fold, as 0-based editor lines (`line + 1 ..= end_line` hidden). */
export interface QueueTreeFold {
    start: number;
    end: number;
    /** Stable identity of the folded row across edits that shift lines. */
    key: string;
}

/** Parse the native JSON into nodes, or null when it is not a queue tree. */
export function parseQueueTreeJson(raw: string): QueueTreeNode[] | null {
    try {
        const parsed = JSON.parse(raw);
        if (!parsed || !Array.isArray(parsed.nodes)) return null;
        return (parsed.nodes as any[]).filter(
            (node) =>
                node &&
                Number.isInteger(node.line) &&
                node.line >= 1 &&
                typeof node.glyph === 'string' &&
                typeof node.state === 'string',
        ).map((node) => ({
            ...node,
            end_line: Number.isInteger(node.end_line) ? node.end_line : node.line,
            closed: node.closed === true,
        })) as QueueTreeNode[];
    } catch {
        return null;
    }
}

/** The gutter hover text: the native tooltip, named as agent-doc's. */
export function queueTreeTooltip(node: QueueTreeNode): string {
    const words = typeof node.tooltip === 'string' && node.tooltip.length > 0
        ? node.tooltip
        : node.state.replace(/-/g, ' ');
    return `agent-doc queue: ${words}`;
}

/** The folds of every closed node with a subtree. */
export function queueTreeFolds(nodes: QueueTreeNode[]): QueueTreeFold[] {
    return nodes
        .filter((node) => node.closed && node.end_line > node.line)
        .map((node) => ({
            start: node.line - 1,
            end: node.end_line - 1,
            key: `${node.id ?? ''}\u0000${node.text ?? ''}\u0000${node.depth}`,
        }));
}

/**
 * Which folds to collapse now: each closed subtree folds once, when it first
 * appears closed, so an operator who expands it keeps it open. A fold holding a
 * cursor line is deferred rather than hiding the line being edited. Returns the
 * folds to collapse and the identity set to remember; a key that is no longer a
 * closed fold is forgotten, so re-closing the row folds it again.
 */
export function planQueueTreeAutoFolds(
    folds: QueueTreeFold[],
    alreadyFolded: ReadonlySet<string>,
    cursorLines: number[],
): { collapse: QueueTreeFold[]; remembered: Set<string> } {
    const remembered = new Set<string>();
    const collapse: QueueTreeFold[] = [];
    for (const fold of folds) {
        if (alreadyFolded.has(fold.key)) {
            remembered.add(fold.key);
            continue;
        }
        if (cursorLines.some((line) => line > fold.start && line <= fold.end)) continue;
        collapse.push(fold);
        remembered.add(fold.key);
    }
    return { collapse, remembered };
}

/** An SVG data URI that paints [glyph] as a gutter icon. */
export function glyphIconDataUri(glyph: string): string {
    const escaped = glyph
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
    const svg =
        "<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 16 16'>" +
        "<text x='8' y='12.5' font-size='12' font-family='sans-serif' text-anchor='middle' fill='#8a8a8a'>" +
        `${escaped}</text></svg>`;
    return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
