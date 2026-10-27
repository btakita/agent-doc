/**
 * Editor-group layout evidence and its derivation (`#vscodeupgradelayout`,
 * `editors/SPEC.md` § 13).
 *
 * The extension reports what its editor groups are — the grid from
 * `vscode.getEditorLayout`, the tab-group count, each visible editor's
 * document, the focused group — and the native `editor_group_layout`
 * derivation of `agent_doc_editor_derive_json` turns that into split columns,
 * orientation, and claim position. The native library hot-reloads under a
 * running extension host, so a layout rule change needs no VSIX (and so no
 * Restart Extensions locally, or Reload Window on a remote host).
 *
 * {@link deriveGroupLayoutFloor} is the frozen TypeScript port of the native
 * rules, used only when the loaded library cannot answer. Both answer every case
 * of `test-fixtures/editor-derivations/editor_group_layout.json`; new rules land
 * in `agent-doc-editor-surface/src/group_layout.rs`, never here.
 */

export const GROUP_LAYOUT_DERIVATION = 'editor_group_layout';

/** Mirrors `GROUP_COLUMN_X_TOLERANCE` in `group_layout.rs`. */
const GROUP_COLUMN_X_TOLERANCE = 0.01;

/** One node of `vscode.getEditorLayout`'s grid (`GroupLayoutArgument`). */
export interface EditorGroupLayoutNode {
    size?: number | null;
    groups?: EditorGroupLayoutNode[] | null;
}

/** `vscode.getEditorLayout`'s answer: `0` left to right, `1` top to bottom. */
export interface EditorGroupLayout {
    orientation?: number | null;
    groups?: EditorGroupLayoutNode[] | null;
}

export interface GroupEditorEvidence {
    view_column: number | null;
    /** A project markdown document in published path form, else `null`. */
    file: string | null;
}

/** Wire shape of Rust `GroupLayoutEvidence`. */
export interface GroupLayoutEvidence {
    layout: EditorGroupLayout | null;
    group_count: number | null;
    editors: GroupEditorEvidence[];
    focused_group: number | null;
}

/** Wire shape of Rust `GroupLayoutResolution`. */
export interface GroupLayoutResolution {
    columns: { files: string[] }[];
    column_order: 'editor' | 'unknown' | 'retained';
    source: 'measured' | 'ordinal';
    orientation: 'h' | 'v' | null;
    position: 'left' | 'right' | 'top' | 'bottom' | null;
    reason: string | null;
    observed_line: string;
}

interface PlacedGroup {
    x: number;
    y: number;
    rootChild: number;
}

function validSize(size: number | null | undefined): number | null {
    return typeof size === 'number' && Number.isFinite(size) && size > 0 ? size : null;
}

function childWeights(children: EditorGroupLayoutNode[]): number[] {
    let presentSum = 0;
    let presentCount = 0;
    for (const child of children) {
        const size = validSize(child.size);
        if (size !== null) {
            presentSum += size;
            presentCount += 1;
        }
    }
    const fallback = presentCount === 0 ? 1 : presentSum / presentCount;
    return children.map((child) => validSize(child.size) ?? fallback);
}

function childrenOf(node: { groups?: EditorGroupLayoutNode[] | null }): EditorGroupLayoutNode[] {
    return Array.isArray(node.groups) ? node.groups : [];
}

function placeGroups(
    children: EditorGroupLayoutNode[],
    horizontal: boolean,
    x: number,
    y: number,
    width: number,
    height: number,
    rootChild: number | null,
    placed: PlacedGroup[],
): void {
    const weights = childWeights(children);
    let total = 0;
    for (const weight of weights) total += weight;
    let offset = 0;
    children.forEach((child, index) => {
        const weight = weights[index];
        const childX = horizontal ? x + width * (offset / total) : x;
        const childY = horizontal ? y : y + height * (offset / total);
        const childWidth = horizontal ? width * (weight / total) : width;
        const childHeight = horizontal ? height : height * (weight / total);
        offset += weight;
        const branch = rootChild ?? index;
        const grandchildren = childrenOf(child);
        if (grandchildren.length === 0) {
            placed.push({ x: childX, y: childY, rootChild: branch });
        } else {
            placeGroups(grandchildren, !horizontal, childX, childY, childWidth, childHeight, branch, placed);
        }
    });
}

function measureGroups(layout: EditorGroupLayout): PlacedGroup[] {
    const placed: PlacedGroup[] = [];
    placeGroups(childrenOf(layout), layout.orientation !== 1, 0, 0, 1, 1, null, placed);
    return placed;
}

function isViewColumn(value: number | null | undefined): value is number {
    return typeof value === 'number' && Number.isInteger(value);
}

function filesByViewColumn(editors: GroupEditorEvidence[], count: number): string[][] {
    const columns: string[][] = Array.from({ length: count }, () => []);
    for (const editor of editors) {
        if (!isViewColumn(editor.view_column)) continue;
        if (editor.view_column < 1 || editor.view_column > count) continue;
        const file = editor.file;
        if (typeof file !== 'string' || file.length === 0) continue;
        const column = columns[editor.view_column - 1];
        if (!column.includes(file)) column.push(file);
    }
    return columns;
}

function maxViewColumn(editors: GroupEditorEvidence[]): number {
    let max = 0;
    for (const editor of editors) {
        if (isViewColumn(editor.view_column) && editor.view_column >= 1) {
            max = Math.max(max, editor.view_column);
        }
    }
    return max;
}

function measuredColumns(placed: PlacedGroup[], files: string[][]): { files: string[] }[] {
    const order = placed.map((_, index) => index);
    order.sort((left, right) => (placed[left].x - placed[right].x) || (placed[left].y - placed[right].y));
    const grouped: number[][] = [];
    for (const index of order) {
        const last = grouped[grouped.length - 1];
        if (last && Math.abs(placed[last[0]].x - placed[index].x) <= GROUP_COLUMN_X_TOLERANCE) {
            last.push(index);
        } else {
            grouped.push([index]);
        }
    }
    return grouped.map((column) => {
        column.sort((left, right) => placed[left].y - placed[right].y);
        const columnFiles: string[] = [];
        for (const index of column) {
            for (const file of files[index]) {
                if (!columnFiles.includes(file)) columnFiles.push(file);
            }
        }
        return { files: columnFiles };
    });
}

function ordinalPosition(evidence: GroupLayoutEvidence): GroupLayoutResolution['position'] {
    if ((evidence.group_count ?? 0) < 2) return null;
    const focused = evidence.focused_group;
    if (focused === 0) return 'left';
    if (typeof focused === 'number' && focused >= 1) return 'right';
    return null;
}

function observedLine(
    evidence: GroupLayoutEvidence,
    leaves: number | null,
    resolution: Omit<GroupLayoutResolution, 'observed_line'>,
): string {
    const rendered = resolution.columns
        .map((column) => (column.files.length === 0 ? '<empty>' : column.files.join(',')))
        .join(' | ');
    const groupCount = typeof evidence.group_count === 'number' ? String(evidence.group_count) : 'unknown';
    let line = `[layout-detect] vscode groups=${groupCount} layout_groups=${leaves === null ? 'none' : leaves}`
        + ` source=${resolution.source} orientation=${resolution.orientation ?? 'none'}`
        + ` columns=${resolution.columns.length} [${rendered}] order=${resolution.column_order}`;
    if (resolution.reason) line += ` reason=${resolution.reason}`;
    return line;
}

/** Frozen port of `resolve_group_layout` (`group_layout.rs`). */
export function deriveGroupLayoutFloor(evidence: GroupLayoutEvidence): GroupLayoutResolution {
    const editors = Array.isArray(evidence.editors) ? evidence.editors : [];
    const maxColumn = maxViewColumn(editors);
    const placed = evidence.layout ? measureGroups(evidence.layout) : null;
    const leaves = placed ? placed.length : null;
    const groupCount = evidence.group_count;
    let geometryReason: string | null;
    if (placed === null || placed.length === 0) {
        geometryReason = 'layout_unavailable';
    } else if (typeof groupCount === 'number' && groupCount >= 0 && placed.length === groupCount) {
        geometryReason = maxColumn > placed.length ? 'layout_misses_visible_column' : null;
    } else {
        geometryReason = 'layout_group_count_mismatch';
    }

    let resolution: Omit<GroupLayoutResolution, 'observed_line'>;
    if (geometryReason === null && placed !== null) {
        const files = filesByViewColumn(editors, placed.length);
        const horizontal = evidence.layout?.orientation !== 1;
        let position: GroupLayoutResolution['position'] = null;
        const focused = evidence.focused_group;
        if (placed.length >= 2 && typeof focused === 'number' && focused >= 0 && focused < placed.length) {
            const first = placed[focused].rootChild === 0;
            position = horizontal ? (first ? 'left' : 'right') : (first ? 'top' : 'bottom');
        }
        resolution = {
            columns: measuredColumns(placed, files),
            column_order: 'editor',
            source: 'measured',
            orientation: placed.length >= 2 ? (horizontal ? 'h' : 'v') : null,
            position,
            reason: null,
        };
    } else {
        const reason = geometryReason ?? 'layout_unavailable';
        resolution = {
            columns: filesByViewColumn(editors, maxColumn).map((files) => ({ files })),
            column_order: reason === 'layout_unavailable' ? 'editor' : 'unknown',
            source: 'ordinal',
            orientation: null,
            position: ordinalPosition(evidence),
            reason,
        };
    }
    return { ...resolution, observed_line: observedLine(evidence, leaves, resolution) };
}

function isResolution(value: unknown): value is GroupLayoutResolution {
    const candidate = value as GroupLayoutResolution | null;
    return !!candidate
        && Array.isArray(candidate.columns)
        && candidate.columns.every((column) => Array.isArray(column?.files))
        && typeof candidate.column_order === 'string'
        && typeof candidate.source === 'string'
        && typeof candidate.observed_line === 'string';
}

/**
 * Ask the native derivation, falling back to the frozen floor when the loaded
 * library cannot answer (absent, older than this derivation, or an error).
 * `deriveNative` takes the request JSON and returns the response JSON or `null`.
 */
export function deriveGroupLayout(
    evidence: GroupLayoutEvidence,
    deriveNative?: (requestJson: string) => string | null,
): { resolution: GroupLayoutResolution; native: boolean } {
    if (deriveNative) {
        try {
            const raw = deriveNative(JSON.stringify({ derivation: GROUP_LAYOUT_DERIVATION, input: evidence }));
            if (raw) {
                const answer = JSON.parse(raw);
                if (answer?.derivation === GROUP_LAYOUT_DERIVATION && isResolution(answer.output)) {
                    const output = answer.output as GroupLayoutResolution;
                    return {
                        resolution: {
                            ...output,
                            orientation: output.orientation ?? null,
                            position: output.position ?? null,
                            reason: output.reason ?? null,
                        },
                        native: true,
                    };
                }
            }
        } catch {
            // fall through to the frozen floor
        }
    }
    return { resolution: deriveGroupLayoutFloor(evidence), native: false };
}

/** The resolution's columns as `string[][]`, the shape the callers publish. */
export function resolutionColumns(resolution: GroupLayoutResolution): string[][] {
    return resolution.columns.map((column) => [...column.files]);
}
