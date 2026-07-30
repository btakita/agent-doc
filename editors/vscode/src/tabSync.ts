/**
 * Editor-surface reporting (`#jbsurfaceswap` / `#jbpluginlazilyeffects`).
 *
 * The extension no longer plans a tab change. It reports what it sees — focused
 * document, visible markdown set, column layout — and the reactive graph behind
 * `agent_doc_editor_surface_observe_json` folds that against what tmux was last
 * reconciled against, derives focus-vs-sync, and runs the Project Controller
 * command as an `Effect`. So the previous-state field, the focus/sync decision,
 * the preserved-layout retry ladder, and the timeout backoff all left this file;
 * what stayed is the layout normalization the editor alone can do, plus the
 * manual `Sync Layout` command's argument builder.
 */

/** One column of the reported split layout. Wire shape of Rust `SurfaceColumn`. */
export interface SurfaceColumn {
    files: string[];
}

/** What the editor looks like right now. Wire shape of Rust `EditorSurface`. */
export interface EditorSurface {
    focused: string;
    visible: string[];
    open: string[];
    columns: SurfaceColumn[];
    force_reconcile: boolean;
}

/** What one observation implied, as reported back by the graph. */
export interface SurfaceIntent {
    kind: 'idle' | 'focus' | 'sync';
    document?: string;
    columns?: SurfaceColumn[];
}

const SAFE_PASSIVE_LAYOUT_PRESERVED_MARKER =
    '[sync] safe passive sync preserved the current tmux layout because';

export function isPreservedLayoutOutput(output: string): boolean {
    return output.includes(SAFE_PASSIVE_LAYOUT_PRESERVED_MARKER);
}

function normalizeVisibleMd(visibleMd: string[]): string[] {
    return [...new Set(visibleMd)].sort();
}

/**
 * Stable editor-work priority: the focused document, adjacent tabs supplied by
 * the adapter, visible documents, then the remaining open documents.
 */
export function prioritizeDocuments(
    focused: string,
    nearbyTabs: string[],
    visible: string[],
    open: string[],
): string[] {
    return [...new Set([focused, ...nearbyTabs, ...visible, ...open].filter(Boolean))];
}

export function normalizeVisibleColumns(visibleColumns: string[][]): string[][] {
    return visibleColumns.map((column) => [...new Set(column.filter((file) => file.length > 0))]);
}

export function flattenVisibleColumns(visibleColumns: string[][]): string[] {
    return normalizeVisibleMd(normalizeVisibleColumns(visibleColumns).flat());
}

export function buildSyncCommandArgs(
    visibleColumns: string[][],
    activeFile: string,
    options?: { noAutostart?: boolean; exactVisible?: boolean },
): string[] {
    const columns = normalizeVisibleColumns(visibleColumns);
    const normalizedColumns = columns.length > 0 ? columns : [[activeFile]];
    const args = ['sync'];
    for (const column of normalizedColumns) {
        args.push('--col', column.join(','));
    }
    args.push('--focus', activeFile);
    if (options?.exactVisible) {
        args.push('--exact-visible');
    }
    if (options?.noAutostart !== false) {
        args.push('--no-autostart');
    }
    return args;
}

export interface EditorSurfaceInput {
    activeFile: string;
    visibleMd: string[];
    openMd?: string[];
    visibleColumns?: string[][];
    forceReconcile?: boolean;
}

/**
 * Build the observation to report.
 *
 * Returns `null` only when there is nothing to observe (no visible markdown) —
 * never because "nothing changed". Deciding that an observation implies no
 * action is the graph's job, and an observation identical to the last one costs
 * nothing there, which is why this needs no previous-state parameter.
 *
 * An undetected layout reports **no** columns rather than a synthesized single
 * column, so the graph can tell "the editor has one column" apart from "the
 * editor could not see its layout" and skip the drift comparison in the latter.
 */
export function buildEditorSurface(input: EditorSurfaceInput): EditorSurface | null {
    const columns = normalizeVisibleColumns(input.visibleColumns ?? []).filter(
        (column) => column.length > 0,
    );
    const visible = normalizeVisibleMd(
        input.visibleMd.length > 0 ? input.visibleMd : columns.flat(),
    );
    if (visible.length === 0) {
        return null;
    }
    return {
        focused: input.activeFile,
        visible,
        open: prioritizeDocuments(input.activeFile, input.openMd ?? [], visible, []),
        columns: columns.map((files) => ({ files })),
        force_reconcile: input.forceReconcile === true,
    };
}

/** The intent a receipt reports, or `null` when the receipt is unusable. */
export function intentFromReceipt(receiptJson: string | null | undefined): SurfaceIntent | null {
    if (!receiptJson) return null;
    try {
        const receipt = JSON.parse(receiptJson);
        const intent = receipt?.intent;
        if (!intent || typeof intent.kind !== 'string') return null;
        return intent as SurfaceIntent;
    } catch {
        return null;
    }
}

export function formatSyncHint(columns: SurfaceColumn[], focus: string): string {
    return `Sync: ${columns.map((column) => `--col ${column.files.join(',')}`).join(' ')} [focus: ${focus}]`;
}

/**
 * The user-visible hint for an observation receipt, or `null` when the derived
 * intent was idle or a pure focus move (which needs no hint — the operator just
 * moved between documents they can already see).
 */
export function syncHintFromReceipt(receiptJson: string | null | undefined): string | null {
    const intent = intentFromReceipt(receiptJson);
    if (!intent || intent.kind !== 'sync' || !intent.document) return null;
    return formatSyncHint(intent.columns ?? [], intent.document);
}
