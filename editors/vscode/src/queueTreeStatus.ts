/**
 * `#treestatusvscode`: VS Code presentation of the native tree-status query
 * (`agent_doc_queue_tree_status_json`, `#treestatusquery`). The binary owns
 * the whole answer: which rows are Next: you / Next: agent / Deletable / Needs
 * agent review, how each is worded, and the `text` rendering the action shows
 * verbatim. This module only validates the JSON and turns each deletable row's
 * 1-based inclusive `line..=end_line` into a delete action. Pure: no `vscode`
 * import, so `node --test` covers it.
 */

/** One row of a tree-status answer (the fields the editor reads). */
export interface QueueTreeStatusRow {
    /** 1-based document line of the row. */
    line: number;
    /** 1-based last line of the row's subtree (inclusive). */
    end_line: number;
    state: string;
    glyph: string;
    text: string;
    id?: string;
    action: string;
}

/** `agent_doc_queue_tree_status_json`: the answer plus its CLI rendering. */
export interface QueueTreeStatus {
    /** The CLI's `agent-doc queue tree-status` rendering, shown verbatim. */
    text: string;
    queue?: string;
    line?: number;
    end_line?: number;
    whole_queue?: boolean;
    you: QueueTreeStatusRow[];
    agent: QueueTreeStatusRow[];
    deletable: QueueTreeStatusRow[];
    needs_review: QueueTreeStatusRow[];
}

/** A delete action for one deletable subtree. */
export interface TreeStatusDeleteItem {
    label: string;
    description: string;
    row: QueueTreeStatusRow;
}

/** 0-based, end-exclusive line span to delete (`[startLine, endLine)`). */
export interface TreeStatusDeleteSpan {
    startLine: number;
    endLine: number;
}

function parseRows(value: unknown): QueueTreeStatusRow[] | null {
    if (!Array.isArray(value)) return null;
    return value
        .filter(
            (row: any) =>
                row &&
                Number.isInteger(row.line) &&
                row.line >= 1 &&
                Number.isInteger(row.end_line) &&
                row.end_line >= row.line &&
                typeof row.glyph === 'string' &&
                typeof row.text === 'string',
        )
        .map((row: any) => ({
            line: row.line,
            end_line: row.end_line,
            state: typeof row.state === 'string' ? row.state : '',
            glyph: row.glyph,
            text: row.text,
            ...(typeof row.id === 'string' ? { id: row.id } : {}),
            action: typeof row.action === 'string' ? row.action : '',
        }));
}

/** Parse the native JSON, or null when it is not a tree-status answer. */
export function parseQueueTreeStatusJson(raw: string): QueueTreeStatus | null {
    let parsed: any;
    try {
        parsed = JSON.parse(raw);
    } catch {
        return null;
    }
    if (!parsed || typeof parsed.text !== 'string') return null;
    const you = parseRows(parsed.you);
    const agent = parseRows(parsed.agent);
    const deletable = parseRows(parsed.deletable);
    const needsReview = parseRows(parsed.needs_review);
    if (!you || !agent || !deletable || !needsReview) return null;
    return {
        text: parsed.text,
        ...(typeof parsed.queue === 'string' ? { queue: parsed.queue } : {}),
        ...(Number.isInteger(parsed.line) ? { line: parsed.line } : {}),
        ...(Number.isInteger(parsed.end_line) ? { end_line: parsed.end_line } : {}),
        ...(typeof parsed.whole_queue === 'boolean' ? { whole_queue: parsed.whole_queue } : {}),
        you,
        agent,
        deletable,
        needs_review: needsReview,
    };
}

function lineLabel(row: QueueTreeStatusRow): string {
    return row.end_line > row.line ? `lines ${row.line}–${row.end_line}` : `line ${row.line}`;
}

/**
 * One delete action per deletable row the document still holds. The binary
 * lists only outermost deletable subtrees, so the spans never overlap.
 */
export function buildTreeStatusDeleteItems(
    status: QueueTreeStatus,
    lineCount: number,
): TreeStatusDeleteItem[] {
    return status.deletable
        .filter((row) => row.end_line <= lineCount)
        .map((row) => ({
            label: `$(trash) Delete ${lineLabel(row)}: ${row.glyph} ${row.text}`,
            description: row.action,
            row,
        }));
}

/**
 * The 0-based, end-exclusive span that removes `line..=end_line` whole
 * (including the last line's newline), or null when the row is outside a
 * document of `lineCount` lines.
 */
export function treeStatusDeleteSpan(row: QueueTreeStatusRow, lineCount: number): TreeStatusDeleteSpan | null {
    if (row.line < 1 || row.end_line < row.line || row.end_line > lineCount) return null;
    return { startLine: row.line - 1, endLine: row.end_line };
}
