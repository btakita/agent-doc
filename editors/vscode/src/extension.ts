import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import * as fs from 'fs';
import * as crypto from 'crypto';
import { execFile } from 'child_process';
import * as native from './native';
import * as stateMirror from './stateMirror';
import { createEditorApplyProof, consumeClaimedPatch, isEditorApplyProofCurrent, isPatchAlreadyApplied } from './patchGuard';
import { appendPatchAlreadyPresent, calculateMinimalReplacement, isFullDocumentReplacement, isPureRepositionSignal } from './patchPlan';
import { parseCrossSessionReject, CrossSessionReject } from './crossSession';
import { parseSaveDocumentSignal, ackContentSidecarPath } from './saveSignal';
import { annotateExchangeHeadingsAgainstBaseline, repositionBoundaryToEnd, repositionBoundaryToEndPreserveHead } from './reposition';
import {
    buildBusySessionRestartBlockedMessage,
    buildBusySessionClearBlockedMessage,
    buildForcedRestartSupervisorCommandArgs,
    buildRouteFailurePresentation,
    buildSessionCommandArgs,
    buildSessionStatusPresentation,
    buildSessionSuccessHint,
    buildStartingSessionRestartBlockedMessage,
    parseBusySessionRestartRefusal,
    parseBusySessionClearRefusal,
    parseStartingSessionRestartRefusal,
    sessionStatusShowsIdleDirectPane,
    type SessionCommandName,
} from './sessionUi';
import {
    buildOverflowPopupMenuItems,
    buildPrimaryPopupMenuItems,
} from './popupMenu';
import {
    buildPromptQuickPickItems,
    normalizePromptEntries,
    type PromptAllEntry,
} from './promptPolling';
import {
    analyzeTabSyncCommandResult,
    buildImmediateFocusCommandArgs,
    buildSyncCommandArgs,
    buildTabChangeCommand,
    flattenVisibleColumns,
    isPreservedLayoutOutput,
    normalizeVisibleColumns,
    shouldReplayQueuedTabChange,
    shouldScheduleDeferredTabSyncRetry,
    type TabSyncState,
} from './tabSync';
import {
    EditorCommandCompletion,
    EditorCommandDecision,
    EditorCommandKind,
    EditorCommandRegistry,
} from './editorCommandState';
import { CrdtReplicaManager, type ReplicaTextChange } from './crdtReplica';

// ---------------------------------------------------------------------------
// CLI Resolution (Feature 9)
// ---------------------------------------------------------------------------

let resolvedAgentDoc: string | null = null;
const SYNC_CLI_TIMEOUT_MS = 30_000;
const FOCUS_CLI_TIMEOUT_MS = 750;
const ROUTE_CANCEL_WAIT_MS = 5_000;
const ROUTE_WAIT_FOR_READY_SECONDS = '120';
const EDITOR_ID = `vscode-${process.pid}-${crypto.randomUUID()}`;
const LIVE_BUFFER_REPORT_DELAY_MS = 75;

// #qnodemerge4wire Phase 4: per-document text shadow (the previous full text).
// VS Code's onDidChangeTextDocument carries only rangeLength (UTF-16) for the
// deleted span, not the old fragment text, so we keep the prior text to compute
// the deleted UTF-8 byte length and the byte offset of a change.
const editorOpShadows = new Map<string, string>();

interface PendingEditorOpReport {
    fsPath: string;
    oldText: string;
    change: ReplicaTextChange;
    projectRoot?: string;
}

function applyTextDocumentChange(
    oldText: string,
    change: vscode.TextDocumentContentChangeEvent | ReplicaTextChange,
): string | null {
    const start = Math.max(0, Math.min(change.rangeOffset, oldText.length));
    const end = Math.max(start, Math.min(start + change.rangeLength, oldText.length));
    return oldText.slice(0, start) + change.text + oldText.slice(end);
}

function seedEditorOpShadow(fsPath: string, text: string): void {
    editorOpShadows.set(fsPath, text);
}

function clearEditorOpShadow(fsPath: string): void {
    editorOpShadows.delete(fsPath);
}

/**
 * #qnodemerge4wire Phase 4: report a markdown document change as byte-offset
 * editor op(s) for CRDT-aligned merge. Converts VS Code's UTF-16 offsets to
 * UTF-8 bytes against the document shadow (the pre-change text). Only single
 * content-change events are captured (the common keystroke/paste/delete case);
 * multi-change events are skipped (the merge's replay gate would reject
 * misaligned ops anyway — safe diff-guess fallback). Native recording is queued
 * off the text-change listener path. Best-effort; never throws into typing.
 */
function captureEditorChangeReport(
    fsPath: string,
    changes: readonly vscode.TextDocumentContentChangeEvent[],
    projectRoot: string | undefined,
): PendingEditorOpReport | null {
    try {
        const oldText = editorOpShadows.get(fsPath);
        // No prior shadow (first edit after open) or a multi-change event: skip
        // capture this edit and let the merge fall back to the diff-guess.
        if (oldText === undefined || changes.length !== 1) return null;

        const change = changes[0];
        const nextText = applyTextDocumentChange(oldText, change);
        if (nextText == null) return null;
        editorOpShadows.set(fsPath, nextText);
        return {
            fsPath,
            oldText,
            projectRoot,
            change: {
                rangeOffset: change.rangeOffset,
                rangeLength: change.rangeLength,
                text: change.text,
            },
        };
    } catch (e: any) {
        console.warn(`[agent-doc] captureEditorChangeReport skipped: ${e?.message ?? e}`);
        return null;
    }
}

function reportEditorChange(report: PendingEditorOpReport): void {
    try {
        const { fsPath, oldText, change, projectRoot } = report;
        // rangeOffset/rangeLength are UTF-16 units in the OLD doc — convert against
        // the shadow (old text) to the UTF-8 byte offset + deleted byte length.
        const { byteOffset, deleteBytes } = native.utf16RangeToUtf8Bytes(
            oldText,
            change.rangeOffset,
            change.rangeLength,
        );

        const baseHash = native.documentBaseHash(fsPath, projectRoot);
        if (!baseHash) return;

        // A replacement is delete(old bytes) then insert(new text) at the same offset.
        if (change.rangeLength > 0) {
            native.recordEditorOp(fsPath, baseHash, 'delete', byteOffset, null, deleteBytes, projectRoot);
        }
        if (change.text.length > 0) {
            native.recordEditorOp(fsPath, baseHash, 'insert', byteOffset, change.text, 0, projectRoot);
        }
    } catch (e: any) {
        console.warn(`[agent-doc] reportEditorChange skipped: ${e?.message ?? e}`);
    }
}

class CliCancelledError extends Error {
    constructor() {
        super('cancelled');
        this.name = 'CliCancelledError';
    }
}

function resolveAgentDoc(): string {
    if (resolvedAgentDoc) return resolvedAgentDoc;
    const home = os.homedir();
    const candidates = [
        path.join(home, 'bin', 'agent-doc'),
        path.join(home, '.local', 'bin', 'agent-doc'),
        path.join(home, '.cargo', 'bin', 'agent-doc'),
        '/usr/local/bin/agent-doc',
    ];
    for (const p of candidates) {
        try {
            fs.accessSync(p, fs.constants.X_OK);
            resolvedAgentDoc = p;
            return p;
        } catch {
            // not found, continue
        }
    }
    resolvedAgentDoc = 'agent-doc';
    return 'agent-doc';
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function isMarkdown(editor: vscode.TextEditor | undefined): boolean {
    return editor?.document.languageId === 'markdown';
}

function getWorkspaceRoot(uri: vscode.Uri): string | undefined {
    return vscode.workspace.getWorkspaceFolder(uri)?.uri.fsPath;
}

function relativePath(root: string, filePath: string): string {
    return path.relative(root, filePath);
}

/**
 * Resolve the agent-doc project root for a file.
 *
 * Walks up from the file's parent looking for the nearest ancestor directory
 * containing `.agent-doc/`. When the file lives inside a submodule that is
 * itself an agent-doc project (e.g. `src/session-share/`), the submodule root
 * is returned so route / claim commands run in the correct working directory
 * and the file path they receive is relative to that root.
 *
 * Falls back to the workspace folder root when no ancestor has `.agent-doc/`.
 *
 * Returns `{ cwd, relativePath }` — ready to pass to `runCli(args, cwd)` with
 * the file argument as `relativePath`.
 */
function resolveProject(
    workspaceRoot: string,
    filePath: string,
): { cwd: string; relativePath: string } {
    // Try FFI first (shared canonical implementation).
    const ffi = native.resolveProjectPath(filePath, workspaceRoot);
    if (ffi) return { cwd: ffi.projectRoot, relativePath: ffi.relativePath };

    // JS fallback: walk up looking for `.agent-doc/`.
    let dir = path.dirname(filePath);
    const fsRoot = path.parse(dir).root;
    while (dir && dir !== fsRoot) {
        if (fs.existsSync(path.join(dir, '.agent-doc'))) {
            return { cwd: dir, relativePath: path.relative(dir, filePath) };
        }
        const parent = path.dirname(dir);
        if (parent === dir) break;
        dir = parent;
    }
    return { cwd: workspaceRoot, relativePath: path.relative(workspaceRoot, filePath) };
}

interface RunCliOptions {
    timeoutMs?: number;
    signal?: AbortSignal;
}

function isCliCancelled(err: unknown): boolean {
    return err instanceof CliCancelledError;
}

/** Run an agent-doc CLI command. Returns stdout on success. */
function runCli(args: string[], cwd: string, options?: RunCliOptions): Promise<string> {
    const bin = resolveAgentDoc();
    return new Promise((resolve, reject) => {
        if (options?.signal?.aborted) {
            reject(new CliCancelledError());
            return;
        }

        let settled = false;
        let child: ReturnType<typeof execFile> | undefined;
        const abortHandler = () => {
            child?.kill('SIGTERM');
        };

        child = execFile(bin, args, {
            cwd,
            maxBuffer: 1024 * 1024,
            timeout: options?.timeoutMs,
            killSignal: 'SIGTERM',
        }, (err, stdout, stderr) => {
            if (settled) return;
            settled = true;
            options?.signal?.removeEventListener('abort', abortHandler);
            if (options?.signal?.aborted) {
                reject(new CliCancelledError());
                return;
            }
            if (err) {
                if ((err as any).killed && options?.timeoutMs) {
                    reject(new Error(`timed out after ${Math.ceil(options.timeoutMs / 1000)}s\n${stdout.trim()}`.trim()));
                    return;
                }
                reject(new Error(stderr?.trim() || err.message));
            } else {
                resolve(stdout.trim());
            }
        });
        options?.signal?.addEventListener('abort', abortHandler, { once: true });
    });
}

// ---------------------------------------------------------------------------
// Slash Command Completion (Feature 10)
// ---------------------------------------------------------------------------

interface CommandInfo {
    name: string;
    args: string;
    description: string;
}

let cachedCommands: CommandInfo[] | null = null;

async function loadCommands(): Promise<CommandInfo[]> {
    if (cachedCommands) return cachedCommands;
    const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!root) return [];
    try {
        const output = await runCli(['commands'], root);
        cachedCommands = JSON.parse(output) as CommandInfo[];
        return cachedCommands;
    } catch {
        return [];
    }
}

class SlashCommandCompletionProvider implements vscode.CompletionItemProvider {
    async provideCompletionItems(
        document: vscode.TextDocument,
        position: vscode.Position,
    ): Promise<vscode.CompletionItem[]> {
        const lineText = document.lineAt(position.line).text;
        const textBeforeCursor = lineText.substring(0, position.character).trimStart();
        if (!textBeforeCursor.startsWith('/')) return [];

        const commands = await loadCommands();
        const prefix = textBeforeCursor.split(' ')[0] || '/';

        return commands
            .filter(cmd => cmd.name.startsWith(prefix))
            .map(cmd => {
                const item = new vscode.CompletionItem(cmd.name, vscode.CompletionItemKind.Function);
                item.detail = cmd.args ? `${cmd.name} ${cmd.args}` : cmd.name;
                item.documentation = cmd.description;
                item.insertText = cmd.name;
                item.filterText = cmd.name;
                // Bold top-level commands (no spaces in name beyond the initial /)
                item.sortText = cmd.name.includes(' ') ? `1${cmd.name}` : `0${cmd.name}`;
                return item;
            });
    }
}

// ---------------------------------------------------------------------------
// Notifications (Feature 7)
// ---------------------------------------------------------------------------

const statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
let statusBarTimeout: ReturnType<typeof setTimeout> | undefined;
const sessionOutputChannel = vscode.window.createOutputChannel('Agent Doc Session');
const routeFailureOutputChannel = vscode.window.createOutputChannel('Agent Doc Route Failures');

function showHint(message: string): void {
    statusBarItem.text = `$(check) ${message}`;
    statusBarItem.show();
    if (statusBarTimeout) clearTimeout(statusBarTimeout);
    statusBarTimeout = setTimeout(() => statusBarItem.hide(), 2000);
}

function showError(message: string): void {
    vscode.window.showErrorMessage(`Agent Doc: ${message}`);
}

// ---------------------------------------------------------------------------
// Visual highlighting
// ---------------------------------------------------------------------------

class SyntaxDecorationController implements vscode.Disposable {
    private readonly disposables: vscode.Disposable[] = [];
    private readonly refreshTimers = new Map<string, ReturnType<typeof setTimeout>>();
    private readonly componentBodyDecoration = vscode.window.createTextEditorDecorationType({
        backgroundColor: new vscode.ThemeColor('editor.rangeHighlightBackground'),
    });
    private readonly componentDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('terminal.ansiCyan'),
        fontWeight: '600',
    });
    private readonly patchDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('terminal.ansiYellow'),
        fontWeight: '600',
    });
    private readonly boundaryDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('editorInfo.foreground'),
        fontStyle: 'italic',
    });
    private readonly scratchDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('descriptionForeground'),
        fontStyle: 'italic',
    });
    private readonly scratchBodyDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('descriptionForeground'),
        backgroundColor: new vscode.ThemeColor('editor.rangeHighlightBackground'),
        fontStyle: 'italic',
    });
    private readonly promptDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('terminal.ansiGreen'),
        fontWeight: '600',
    });
    private readonly responseHeadingDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('terminal.ansiBlue'),
        fontWeight: '600',
    });
    private readonly trackedIdDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('terminal.ansiYellow'),
        border: '1px solid',
        borderColor: new vscode.ThemeColor('terminal.ansiYellow'),
        borderRadius: '3px',
        backgroundColor: new vscode.ThemeColor('editor.wordHighlightBackground'),
    });
    private readonly labelTagDecoration = vscode.window.createTextEditorDecorationType({
        color: new vscode.ThemeColor('terminal.ansiYellow'),
        border: '1px solid',
        borderColor: new vscode.ThemeColor('terminal.ansiYellow'),
        borderRadius: '3px',
        backgroundColor: new vscode.ThemeColor('editor.wordHighlightBackground'),
        fontWeight: '600',
    });
    // #editor-bold-markdown-rendering: render markdown emphasis inline.
    private readonly boldDecoration = vscode.window.createTextEditorDecorationType({
        fontWeight: 'bold',
    });
    private readonly italicDecoration = vscode.window.createTextEditorDecorationType({
        fontStyle: 'italic',
    });

    constructor() {
        this.disposables.push(
            this.componentBodyDecoration,
            this.componentDecoration,
            this.patchDecoration,
            this.boundaryDecoration,
            this.scratchDecoration,
            this.scratchBodyDecoration,
            this.promptDecoration,
            this.responseHeadingDecoration,
            this.trackedIdDecoration,
            this.labelTagDecoration,
            this.boldDecoration,
            this.italicDecoration,
        );
        this.disposables.push(
            vscode.window.onDidChangeVisibleTextEditors((editors) => {
                for (const editor of editors) this.refreshEditor(editor);
            }),
            vscode.window.onDidChangeActiveTextEditor((editor) => {
                if (editor) this.refreshEditor(editor);
            }),
            vscode.workspace.onDidOpenTextDocument((document) => this.scheduleRefresh(document)),
            vscode.workspace.onDidChangeTextDocument((event) => this.scheduleRefresh(event.document)),
            vscode.workspace.onDidCloseTextDocument((document) => {
                const timer = this.refreshTimers.get(document.uri.toString());
                if (timer) {
                    clearTimeout(timer);
                    this.refreshTimers.delete(document.uri.toString());
                }
            }),
        );
        this.refreshAll();
    }

    private scheduleRefresh(document: vscode.TextDocument): void {
        if (document.languageId !== 'markdown') return;
        const key = document.uri.toString();
        const existing = this.refreshTimers.get(key);
        if (existing) clearTimeout(existing);
        const timer = setTimeout(() => {
            this.refreshTimers.delete(key);
            for (const editor of vscode.window.visibleTextEditors) {
                if (editor.document.uri.toString() === key) {
                    this.refreshEditor(editor);
                }
            }
        }, 120);
        this.refreshTimers.set(key, timer);
    }

    private refreshAll(): void {
        for (const editor of vscode.window.visibleTextEditors) {
            this.refreshEditor(editor);
        }
    }

    private refreshEditor(editor: vscode.TextEditor): void {
        if (!isMarkdown(editor)) {
            this.clearEditor(editor);
            return;
        }

        const root = getWorkspaceRoot(editor.document.uri);
        const tokens = native.visualTokens(editor.document.getText(), root);
        const ranges = {
            componentBody: [] as vscode.Range[],
            component: [] as vscode.Range[],
            patch: [] as vscode.Range[],
            boundary: [] as vscode.Range[],
            scratch: [] as vscode.Range[],
            scratchBody: [] as vscode.Range[],
            prompt: [] as vscode.Range[],
            responseHeading: [] as vscode.Range[],
            trackedId: [] as vscode.Range[],
            labelTag: [] as vscode.Range[],
            bold: [] as vscode.Range[],
            italic: [] as vscode.Range[],
        };

        for (const token of tokens) {
            const range = new vscode.Range(
                editor.document.positionAt(token.start),
                editor.document.positionAt(token.end),
            );
            switch (token.kind) {
                case 'component_body':
                    ranges.componentBody.push(range);
                    break;
                case 'component_open':
                case 'component_close':
                    ranges.component.push(range);
                    break;
                case 'patch_open':
                case 'patch_close':
                    ranges.patch.push(range);
                    break;
                case 'boundary':
                    ranges.boundary.push(range);
                    break;
                case 'scratch_comment':
                    ranges.scratch.push(range);
                    break;
                case 'scratch_comment_body':
                    ranges.scratchBody.push(range);
                    break;
                case 'prompt':
                    ranges.prompt.push(range);
                    break;
                case 'response_heading':
                    ranges.responseHeading.push(range);
                    break;
                case 'tracked_id':
                    ranges.trackedId.push(range);
                    break;
                case 'label_tag':
                    ranges.labelTag.push(range);
                    break;
                case 'bold':
                    ranges.bold.push(range);
                    break;
                case 'italic':
                    ranges.italic.push(range);
                    break;
            }
        }

        editor.setDecorations(this.componentBodyDecoration, ranges.componentBody);
        editor.setDecorations(this.componentDecoration, ranges.component);
        editor.setDecorations(this.patchDecoration, ranges.patch);
        editor.setDecorations(this.boundaryDecoration, ranges.boundary);
        editor.setDecorations(this.scratchDecoration, ranges.scratch);
        editor.setDecorations(this.scratchBodyDecoration, ranges.scratchBody);
        editor.setDecorations(this.promptDecoration, ranges.prompt);
        editor.setDecorations(this.responseHeadingDecoration, ranges.responseHeading);
        editor.setDecorations(this.trackedIdDecoration, ranges.trackedId);
        editor.setDecorations(this.labelTagDecoration, ranges.labelTag);
        editor.setDecorations(this.boldDecoration, ranges.bold);
        editor.setDecorations(this.italicDecoration, ranges.italic);
    }

    private clearEditor(editor: vscode.TextEditor): void {
        editor.setDecorations(this.componentBodyDecoration, []);
        editor.setDecorations(this.componentDecoration, []);
        editor.setDecorations(this.patchDecoration, []);
        editor.setDecorations(this.boundaryDecoration, []);
        editor.setDecorations(this.scratchDecoration, []);
        editor.setDecorations(this.scratchBodyDecoration, []);
        editor.setDecorations(this.promptDecoration, []);
        editor.setDecorations(this.responseHeadingDecoration, []);
        editor.setDecorations(this.trackedIdDecoration, []);
        editor.setDecorations(this.labelTagDecoration, []);
        editor.setDecorations(this.boldDecoration, []);
        editor.setDecorations(this.italicDecoration, []);
    }

    dispose(): void {
        for (const timer of this.refreshTimers.values()) {
            clearTimeout(timer);
        }
        this.refreshTimers.clear();
        for (const editor of vscode.window.visibleTextEditors) {
            this.clearEditor(editor);
        }
        for (const disposable of this.disposables) {
            disposable.dispose();
        }
    }
}

// ---------------------------------------------------------------------------
// Concurrency guard
// ---------------------------------------------------------------------------

let commandRunning = false;

// ---------------------------------------------------------------------------
// Split / Layout Detection (Features 2, 3)
// ---------------------------------------------------------------------------

interface SplitInfo {
    orientation: 'h' | 'v' | undefined;
    position: string | undefined;
}

function detectSplit(editor: vscode.TextEditor): SplitInfo {
    const groups = vscode.window.tabGroups.all;
    if (groups.length < 2) {
        return { orientation: undefined, position: undefined };
    }

    // Find which group the editor belongs to
    const editorUri = editor.document.uri.toString();
    let editorGroupIndex = -1;
    for (let i = 0; i < groups.length; i++) {
        for (const tab of groups[i].tabs) {
            if (tab.input instanceof vscode.TabInputText && tab.input.uri.toString() === editorUri) {
                editorGroupIndex = i;
                break;
            }
        }
        if (editorGroupIndex >= 0) break;
    }

    // VS Code doesn't directly expose orientation, but viewColumn gives position.
    // viewColumn 1,2,3... for side-by-side; for top/bottom we heuristic-check.
    // Side-by-side is the most common split in VS Code.
    const orientation: 'h' | 'v' = 'h'; // Default assumption: horizontal split
    let position: string | undefined;

    if (editorGroupIndex === 0) {
        position = 'left';
    } else if (editorGroupIndex >= 1) {
        position = 'right';
    }

    return { orientation, position };
}

function collectVisibleMarkdownColumns(root: string): string[][] {
    const columns = new Map<number, string[]>();
    let maxColumn = 0;

    for (const editor of vscode.window.visibleTextEditors) {
        const viewColumn = editor.viewColumn;
        if (viewColumn === undefined) continue;
        maxColumn = Math.max(maxColumn, viewColumn);

        const column = columns.get(viewColumn) ?? [];
        if (isMarkdown(editor)) {
            const uri = editor.document.uri;
            if (uri.fsPath.startsWith(root)) {
                const rel = relativePath(root, uri.fsPath);
                if (!column.includes(rel)) column.push(rel);
            }
        }
        columns.set(viewColumn, column);
    }

    if (maxColumn === 0) {
        return [];
    }

    const orderedColumns: string[][] = [];
    for (let column = 1; column <= maxColumn; column += 1) {
        orderedColumns.push(columns.get(column) ?? []);
    }
    return orderedColumns;
}

function formatSyncLayoutSummary(columns: string[][], focusFile?: string): string {
    const summarizedColumns = columns
        .map((column) => column.join(','))
        .join(' | ');
    return `Sync: --col ${summarizedColumns}${focusFile ? ` [focus: ${focusFile}]` : ''}`;
}

function buildSyncLayoutCommand(
    columns: string[][],
    focusFile?: string,
    noAutostart = false,
): string[] {
    if (!focusFile) {
        const firstVisible = flattenVisibleColumns(columns)[0];
        if (!firstVisible) return ['sync'];
        return buildSyncCommandArgs(columns, firstVisible, { noAutostart });
    }
    return buildSyncCommandArgs(columns, focusFile, { noAutostart });
}

function buildRouteLayoutArgs(columns: string[][], focusFile?: string): string[] {
    const normalizedColumns = normalizeVisibleColumns(columns);
    const args: string[] = [];
    if (normalizedColumns.length > 1) {
        for (const column of normalizedColumns) {
            args.push('--col', column.join(','));
        }
    } else {
        const visibleMd = flattenVisibleColumns(normalizedColumns);
        if (visibleMd.length > 0) {
            args.push('--col', visibleMd.join(','));
        }
    }
    if (focusFile) {
        args.push('--focus', focusFile);
    }
    return args;
}

function buildRunRouteCommandArgs(
    relativePath: string,
    columns: string[][],
    focusFile?: string,
): string[] {
    return [
        'route',
        '--dispatch-only',
        '--plain-trigger',
        '--debounce',
        '0',
        '--wait-for-ready',
        ROUTE_WAIT_FOR_READY_SECONDS,
        relativePath,
        ...buildRouteLayoutArgs(columns, focusFile),
    ];
}

// ---------------------------------------------------------------------------
// Feature 1: Run (Submit)
// ---------------------------------------------------------------------------

const trackedFiles = new Set<string>();
const editorCommandRegistry = new EditorCommandRegistry();
interface ActiveRoute {
    controller: AbortController;
    settled: Promise<void>;
}
const activeRoutes = new Map<string, ActiveRoute>();

async function submitAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
    await startRunForDocument(cwd, rel, editor.document.uri.fsPath);
}

function buildEditorCommandRouteKey(cwd: string, relativePath: string): string {
    return `${cwd}\0${relativePath}`;
}

async function ensureDocumentCleanForCommand(filePath: string, commandLabel: string): Promise<boolean> {
    const document = vscode.workspace.textDocuments.find((doc) => doc.uri.fsPath === filePath)
        ?? await vscode.workspace.openTextDocument(vscode.Uri.file(filePath));
    if (document.isDirty) {
        showError(`${commandLabel}: document has unsaved editor changes; synchronize the buffer before running this command`);
        return false;
    }
    return true;
}

async function startRunForDocument(cwd: string, rel: string, filePath: string): Promise<void> {
    const routeKey = buildEditorCommandRouteKey(cwd, rel);
    const decision = editorCommandRegistry.request(routeKey, EditorCommandKind.RunAgentDoc);
    switch (decision) {
        case EditorCommandDecision.StartNow:
            await executeRunForDocument(cwd, rel, filePath, routeKey);
            return;
        case EditorCommandDecision.DedupeActiveRun:
            showHint(`Run already dispatching for ${rel}`);
            return;
        case EditorCommandDecision.QueueRunAfterClear:
            showHint(`Run queued until Clear Session Context finishes for ${rel}`);
            return;
        default:
            showHint(`Run ignored while another command owns ${rel}`);
            return;
    }
}

async function executeRunForDocument(
    cwd: string,
    rel: string,
    filePath: string,
    routeKey: string,
): Promise<void> {
    const abortController = new AbortController();
    let routeGeneration: number | null = null;
    let resolveSettled: () => void = () => {};
    const settled = new Promise<void>((resolve) => {
        resolveSettled = resolve;
    });
    activeRoutes.set(routeKey, {
        controller: abortController,
        settled,
    });
    try {
        if (!(await ensureDocumentCleanForCommand(filePath, 'Run'))) return;
        routeGeneration = native.recordRouteDispatchStarted(filePath, routeKey, cwd);
        const output = await runCli(
            buildRunRouteCommandArgs(rel, collectVisibleMarkdownColumns(cwd), rel),
            cwd,
            { signal: abortController.signal },
        );
        native.recordRouteDispatchProven(filePath, routeGeneration, `vscode:${routeKey}`, cwd);
        // #r5at: read via the lazily-js reactive mirror (snapshot/delta over the
        // FFI state backbone), falling back to the cold projection pull. The
        // just-recorded dispatch facts surface as a warm delta without a full
        // re-render — the VS Code counterpart of the JB reactiveSummaryForFile.
        const summary = native.reactiveSummaryForFile(filePath, cwd);
        if (summary) {
            console.log(
                `[agent-doc/state-projection] ${stateMirror.compactMirrorSummary(summary)} `
                + `epoch=${native.mirrorEpochForFile(filePath) ?? '-'} file=${rel}`,
            );
        }
        showHint(output || `Routed ${rel}`);
        trackedFiles.add(filePath);
        ensurePromptPolling(cwd);
    } catch (err: any) {
        if (isCliCancelled(err)) {
            showHint(`Run cancelled before Clear Session Context for ${rel}`);
            return;
        }
        native.recordRouteBlocked(filePath, routeGeneration, err.message, cwd);
        const failure = buildRouteFailurePresentation(rel, err.message);
        showRouteFailureOutput(failure.title, failure.body);
        showError(failure.toast);
    } finally {
        if (activeRoutes.get(routeKey)?.controller === abortController) {
            activeRoutes.delete(routeKey);
        }
        editorCommandRegistry.complete(routeKey, EditorCommandKind.RunAgentDoc);
        resolveSettled();
    }
}

async function cancelActiveRoute(routeKey: string): Promise<void> {
    const route = activeRoutes.get(routeKey);
    if (!route) return;
    route.controller.abort();
    await Promise.race([
        route.settled,
        new Promise<void>((resolve) => setTimeout(resolve, ROUTE_CANCEL_WAIT_MS)),
    ]);
}

async function runSessionCommandForActiveFile(
    command: SessionCommandName,
    onSuccess: (output: string, relativePath: string) => void,
    onErrorLabel: string,
    onFailure?: (errorMessage: string, relativePath: string, cwd: string) => Promise<void> | void,
): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    let rel = '';
    let cwd = root;
    try {
        const resolved = resolveProject(root, editor.document.uri.fsPath);
        cwd = resolved.cwd;
        rel = resolved.relativePath;
        const output = await runCli(buildSessionCommandArgs(command, rel), cwd);
        onSuccess(output, rel);
    } catch (err: any) {
        if (onFailure) {
            await onFailure(err.message, rel, cwd);
            return;
        }
        showError(`${onErrorLabel}: ${err.message}`);
    }
}

function showSessionOutput(title: string, output: string): void {
    sessionOutputChannel.clear();
    sessionOutputChannel.appendLine(title);
    if (output.trim()) {
        sessionOutputChannel.appendLine(output.trim());
    } else {
        sessionOutputChannel.appendLine('(no output)');
    }
    sessionOutputChannel.show(true);
}

function showRouteFailureOutput(title: string, output: string): void {
    routeFailureOutputChannel.clear();
    routeFailureOutputChannel.appendLine(title);
    if (output.trim()) {
        routeFailureOutputChannel.appendLine(output.trim());
    } else {
        routeFailureOutputChannel.appendLine('(no output)');
    }
    routeFailureOutputChannel.show(true);
}

async function showSessionStatusAction(): Promise<void> {
    await runSessionCommandForActiveFile(
        'status',
        (output, rel) => {
            const presentation = buildSessionStatusPresentation(rel, output);
            showSessionOutput(presentation.title, presentation.body);
            showHint(presentation.hint);
        },
        'session status failed',
    );
}

async function restartSessionAction(): Promise<void> {
    await runSessionCommandForActiveFile(
        'restart-supervisor',
        (output, rel) => {
            showHint(buildSessionSuccessHint('restart-supervisor', rel, output));
        },
        'supervisor restart failed',
        async (errorMessage, rel, cwd) => {
            const busyRefusal = parseBusySessionRestartRefusal(errorMessage);
            const startingRefusal = parseStartingSessionRestartRefusal(errorMessage);
            if (!busyRefusal && !startingRefusal) {
                showError(`supervisor restart failed: ${errorMessage}`);
                return;
            }
            const message = busyRefusal
                ? buildBusySessionRestartBlockedMessage(rel, busyRefusal)
                : buildStartingSessionRestartBlockedMessage(rel, startingRefusal!);
            const action = await vscode.window.showWarningMessage(
                message,
                { modal: false },
                'Interrupt and restart',
                'Show status',
                'Copy details',
            );
            if (action === 'Interrupt and restart') {
                await interruptAndRestartSupervisor(cwd, rel);
            } else if (action === 'Show status') {
                await showSessionStatusFor(cwd, rel);
            } else if (action === 'Copy details') {
                await vscode.env.clipboard.writeText(errorMessage);
                showHint(`Copied restart details for ${rel}`);
            }
        },
    );
}

// #s81q: Restart Agent — runs the same `session restart-supervisor` path as
// "Recycle Supervisor" but with a distinct operator-facing intent (bring
// the agent harness back up, re-resolving a changed `agent:` frontmatter).
// Mirrors the JetBrains RestartAgentAction.
async function restartAgentAction(): Promise<void> {
    await runSessionCommandForActiveFile(
        'restart-supervisor',
        (output, rel) => {
            showHint(buildSessionSuccessHint('restart-supervisor', rel, output));
        },
        'agent restart failed',
    );
}

// #s81q: Stop Agent — `agent-doc session stop-agent <rel>`. Stops the harness
// agent child while keeping the supervisor alive at its keepalive prompt; the
// operator can bring it back with "Restart Agent".
async function stopAgentAction(): Promise<void> {
    await runSessionCommandForActiveFile(
        'stop-agent',
        (output, rel) => {
            showHint(buildSessionSuccessHint('stop-agent', rel, output));
        },
        'stop agent failed',
    );
}

// Cancel Turn — `agent-doc session cancel-turn <rel>`. Cancels the currently
// running turn while keeping the agent harness and its supervisor alive; no-op
// when the agent is idle, so it never closes the agent. Mirrors stopAgentAction.
async function cancelTurnAction(): Promise<void> {
    await runSessionCommandForActiveFile(
        'cancel-turn',
        (output, rel) => {
            showHint(buildSessionSuccessHint('cancel-turn', rel, output));
        },
        'cancel turn failed',
    );
}

// #s81q: Kill Supervisor — `agent-doc admin kill-supervisor <rel>`. Stops the
// whole route-owned supervisor process for this document. The CLI refuses to
// kill the caller's own ancestor, so this runs from the editor's project root.
// Unlike the session commands this is an `admin` subcommand, so it builds args
// directly rather than through buildSessionCommandArgs.
async function killSupervisorAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    try {
        const { cwd, relativePath } = resolveProject(root, editor.document.uri.fsPath);
        const output = await runCli(['admin', 'kill-supervisor', relativePath], cwd);
        showHint(output.trim() || `Killed supervisor for ${relativePath}`);
    } catch (err: any) {
        showError(`kill supervisor failed: ${err.message}`);
    }
}

// #plugin-cleanup-menu-command: project-level session-hygiene commands. Unlike
// the file-scoped session commands, `resync --fix` and `gc` operate on the whole
// session registry, so they run in the project root (the focused .md file's root
// when one is open, else the first workspace folder). Thin event-reporter: the
// CLI owns all cleanup logic; this only dispatches and reports the outcome.
function resolveCleanupCwd(): string | undefined {
    const editor = vscode.window.activeTextEditor;
    if (editor && isMarkdown(editor)) {
        const root = getWorkspaceRoot(editor.document.uri);
        if (root) return resolveProject(root, editor.document.uri.fsPath).cwd;
    }
    return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

async function runProjectCleanupCommand(label: string, args: string[]): Promise<void> {
    const cwd = resolveCleanupCwd();
    if (!cwd) {
        showError(`${label}: no workspace folder open`);
        return;
    }
    showHint(`${label}: running agent-doc ${args.join(' ')}…`);
    try {
        const output = await runCli(args, cwd, { timeoutMs: 30_000 });
        showSessionOutput(label, output || 'No changes.');
    } catch (err: any) {
        showError(`${label} failed: ${err.message}`);
    }
}

async function resyncFixSessionsAction(): Promise<void> {
    await runProjectCleanupCommand('Resync / Fix Sessions', ['resync', '--fix']);
}

async function gcStaleSessionsAction(): Promise<void> {
    await runProjectCleanupCommand('GC Stale Sessions', ['gc']);
}

async function fixDocumentAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    try {
        if (!(await ensureDocumentCleanForCommand(editor.document.uri.fsPath, 'Fix document'))) return;
        const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
        const output = await runCli(['fix', rel], cwd, { timeoutMs: 30_000 });
        showHint(output || `Fixed ${rel}`);
    } catch (err: any) {
        showError(`fix document failed: ${err.message}`);
    }
}

async function loadTmuxWindowAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    const { cwd } = resolveProject(root, editor.document.uri.fsPath);
    showHint('Loading tmux window...');
    await syncLayoutInternal(cwd, true, false);
}

async function compactExchangeAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    try {
        if (!(await ensureDocumentCleanForCommand(editor.document.uri.fsPath, 'Compact exchange'))) return;
        const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
        const output = await runCli(['compact', rel, '--component', 'exchange', '--commit'], cwd);
        showHint(output || `Compacted exchange for ${rel}`);
    } catch (err: any) {
        showError(`compact exchange failed: ${err.message}`);
    }
}

async function runWithJunieAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    try {
        if (!(await ensureDocumentCleanForCommand(editor.document.uri.fsPath, 'Run with Junie'))) return;
        const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
        const output = await runCli(['run', '--agent', 'junie', rel], cwd);
        showHint(output || `Ran Junie for ${rel}`);
    } catch (err: any) {
        showError(`run with Junie failed: ${err.message}`);
    }
}

async function clearSessionContextAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
    const filePath = editor.document.uri.fsPath;
    const routeKey = buildEditorCommandRouteKey(cwd, rel);
    const decision = editorCommandRegistry.request(routeKey, EditorCommandKind.ClearSessionContext);
    switch (decision) {
        case EditorCommandDecision.StartNow:
            break;
        case EditorCommandDecision.PreemptRunWithClear:
            showHint(`Cancelling Run before Clear Session Context for ${rel}`);
            await cancelActiveRoute(routeKey);
            break;
        case EditorCommandDecision.DedupeActiveClear:
            showHint(`Clear Session Context already running for ${rel}`);
            return;
        default:
            showHint(`Clear Session Context ignored while another command owns ${rel}`);
            return;
    }

    await executeClearSessionContext(cwd, rel, filePath, routeKey);
}

async function executeClearSessionContext(
    cwd: string,
    rel: string,
    filePath: string,
    routeKey: string,
): Promise<void> {
    try {
        if (!(await ensureDocumentCleanForCommand(filePath, 'Clear Session Context'))) return;
        const output = await runCli(buildSessionCommandArgs('clear', rel), cwd);
        showHint(buildSessionSuccessHint('clear', rel, output));
    } catch (err: any) {
        await handleClearSessionContextFailure(err.message, rel, cwd);
    } finally {
        const completion = editorCommandRegistry.complete(
            routeKey,
            EditorCommandKind.ClearSessionContext,
        );
        if (completion === EditorCommandCompletion.StartQueuedRun) {
            void executeRunForDocument(cwd, rel, filePath, routeKey);
        }
    }
}

async function handleClearSessionContextFailure(
    errorMessage: string,
    rel: string,
    cwd: string,
): Promise<void> {
    const refusal = parseBusySessionClearRefusal(errorMessage);
    if (!refusal) {
        showError(`session clear failed: ${errorMessage}`);
        return;
    }
    const action = await vscode.window.showWarningMessage(
        buildBusySessionClearBlockedMessage(rel, refusal),
        { modal: false },
        ...(refusal.protectedReason ? [] : ['Refresh and retry']),
        'Interrupt and clear',
        'Show status',
        'Copy details',
    );
    if (action === 'Refresh and retry') {
        await refreshAndRetryClearSessionContext(cwd, rel);
    } else if (action === 'Interrupt and clear') {
        await interruptAndClearSessionContext(cwd, rel);
    } else if (action === 'Show status') {
        await showSessionStatusFor(cwd, rel);
    } else if (action === 'Copy details') {
        await vscode.env.clipboard.writeText(errorMessage);
        showHint(`Copied busy session details for ${rel}`);
    }
}

async function showSessionStatusFor(cwd: string, rel: string): Promise<string> {
    const output = await runCli(buildSessionCommandArgs('status', rel), cwd);
    const presentation = buildSessionStatusPresentation(rel, output);
    showSessionOutput(presentation.title, presentation.body);
    showHint(presentation.hint);
    return output;
}

async function refreshAndRetryClearSessionContext(cwd: string, rel: string): Promise<void> {
    const output = await showSessionStatusFor(cwd, rel);
    if (!sessionStatusShowsIdleDirectPane(output)) return;
    const clearOutput = await runCli(buildSessionCommandArgs('clear', rel), cwd);
    showHint(buildSessionSuccessHint('clear', rel, clearOutput));
}

async function interruptAndClearSessionContext(cwd: string, rel: string): Promise<void> {
    const decision = await vscode.window.showWarningMessage(
        'Interrupt the running agent-doc turn and clear its session context? Unsaved work in the terminal session may be discarded.',
        { modal: true },
        'Interrupt and clear',
    );
    if (decision !== 'Interrupt and clear') return;
    const output = await runCli(buildSessionCommandArgs('interrupt-clear', rel), cwd);
    showHint(output || `Interrupted and cleared session context for ${rel}`);
}

async function interruptAndRestartSupervisor(cwd: string, rel: string): Promise<void> {
    const decision = await vscode.window.showWarningMessage(
        'Interrupt the running agent-doc turn and restart its supervisor? Unsaved work in the terminal session may be discarded.',
        { modal: true },
        'Interrupt and restart',
    );
    if (decision !== 'Interrupt and restart') return;
    const output = await runCli(buildForcedRestartSupervisorCommandArgs(rel), cwd);
    showHint(buildSessionSuccessHint('restart-supervisor', rel, output));
}

async function copySessionDiagnosticsAction(): Promise<void> {
    await runSessionCommandForActiveFile(
        'doctor',
        async (output, rel) => {
            await vscode.env.clipboard.writeText(output);
            showSessionOutput(`Session diagnostics: ${rel}`, output);
            showHint(buildSessionSuccessHint('doctor', rel, output));
        },
        'session diagnostics failed',
    );
}

async function interruptClearSessionContextAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
    await interruptAndClearSessionContext(cwd, rel);
}

// ---------------------------------------------------------------------------
// Feature 2: Claim
// ---------------------------------------------------------------------------

async function claimAction(): Promise<void> {
    await claimActionInternal(false);
}

async function forceClaimAction(): Promise<void> {
    await claimActionInternal(true);
}

async function claimActionInternal(force: boolean): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    if (commandRunning) {
        showHint('Command already in progress');
        return;
    }
    commandRunning = true;

    let crossSession: CrossSessionReject | undefined;
    let recoveryCtx: { cwd: string; rel: string; position?: string } | undefined;
    try {
        const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
        const split = detectSplit(editor);
        recoveryCtx = { cwd, rel, position: split.position };
        const args = ['claim', rel];
        if (force) {
            args.push('--force');
        }
        if (split.position) {
            args.push('--position', split.position);
        }

        const output = await runCli(args, cwd);
        const actionVerb = force ? 'Force-claimed' : 'Claimed';
        showHint(output || `${actionVerb} ${rel} (pos=${split.position || 'none'})`);

        // Trigger silent layout sync after claiming
        await syncLayoutInternal(cwd, false, true);
    } catch (err: any) {
        // A cross-session reject carries a structured marker (claim.rs); branch to a
        // choice dialog instead of surfacing the raw exit-1. Force claims can't
        // cross-session-reject, so only parse on the non-force path.
        const reject = force ? undefined : parseCrossSessionReject(err?.message ?? '');
        if (reject) {
            crossSession = reject;
        } else {
            showError(`claim failed: ${err.message}`);
        }
    } finally {
        commandRunning = false;
    }

    // Run the dialog/recovery after commandRunning is cleared so the re-claim can re-enter.
    if (crossSession && recoveryCtx) {
        await handleCrossSessionReject(recoveryCtx.cwd, recoveryCtx.rel, recoveryCtx.position, crossSession);
    }
}

/** Prompt Force Claim / Switch Project Session / Cancel on a cross-session reject. */
async function handleCrossSessionReject(
    cwd: string,
    rel: string,
    position: string | undefined,
    reject: CrossSessionReject,
): Promise<void> {
    const choice = await vscode.window.showWarningMessage(
        `Pane ${reject.paneId} is in tmux session '${reject.paneSession}', but this project's ` +
            `configured session is '${reject.configured}'. How do you want to claim it?`,
        { modal: true },
        'Force Claim',
        'Switch Project Session',
    );
    if (choice === 'Force Claim') {
        await reclaimAfterCrossSession(cwd, rel, position, { force: true });
    } else if (choice === 'Switch Project Session') {
        await reclaimAfterCrossSession(cwd, rel, position, { switchTo: reject.paneSession });
    }
    // undefined (Esc / dismiss) => Cancel, leave the file unclaimed.
}

/** Force-claim, or switch the configured session then claim, after a cross-session reject. */
async function reclaimAfterCrossSession(
    cwd: string,
    rel: string,
    position: string | undefined,
    opts: { force?: boolean; switchTo?: string },
): Promise<void> {
    if (commandRunning) {
        showHint('Command already in progress');
        return;
    }
    commandRunning = true;
    try {
        if (opts.switchTo) {
            await runCli(['session', 'set', opts.switchTo], cwd);
        }
        const args = ['claim', rel];
        if (opts.force) {
            args.push('--force');
        }
        if (position) {
            args.push('--position', position);
        }
        const output = await runCli(args, cwd);
        showHint(output || `Claimed ${rel}`);
        await syncLayoutInternal(cwd, false, true);
    } catch (err: any) {
        showError(`claim failed: ${err.message}`);
    } finally {
        commandRunning = false;
    }
}

// ---------------------------------------------------------------------------
// Feature 3: Sync Layout
// ---------------------------------------------------------------------------

async function syncLayoutAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) {
        showError('File is not in a workspace');
        return;
    }

    if (commandRunning) {
        showHint('Command already in progress');
        return;
    }
    commandRunning = true;

    try {
        const { cwd } = resolveProject(root, editor.document.uri.fsPath);
        await syncLayoutInternal(cwd, true, true);
    } finally {
        commandRunning = false;
    }
}

async function syncLayoutInternal(root: string, notify: boolean, noAutostart: boolean): Promise<void> {
    const visibleColumns = collectVisibleMarkdownColumns(root);
    const visibleMd = flattenVisibleColumns(visibleColumns);
    if (visibleMd.length === 0) {
        if (notify) showHint('No .md files open');
        return;
    }

    // Determine focused file
    const activeEditor = vscode.window.activeTextEditor;
    let focusFile: string | undefined;
    if (activeEditor && isMarkdown(activeEditor)) {
        const activeRoot = getWorkspaceRoot(activeEditor.document.uri);
        if (activeRoot) {
            const activeProject = resolveProject(activeRoot, activeEditor.document.uri.fsPath);
            if (activeProject.cwd === root) {
                focusFile = activeProject.relativePath;
            }
        }
    }

    try {
        const args = buildSyncLayoutCommand(visibleColumns, focusFile, noAutostart);
        const output = await runCli(args, root, { timeoutMs: SYNC_CLI_TIMEOUT_MS });
        if (notify) {
            if (isPreservedLayoutOutput(output)) {
                const warning = output
                    .split(/\r?\n/)
                    .find((line) => isPreservedLayoutOutput(line))
                    ?? output.trim();
                vscode.window.showWarningMessage(warning);
            } else {
                showHint(formatSyncLayoutSummary(visibleColumns, focusFile));
            }
        }
    } catch (err: any) {
        if (notify) showError(`sync failed: ${err.message}`);
    }
}

// ---------------------------------------------------------------------------
// Feature 4: Tab Sync (Automatic)
// ---------------------------------------------------------------------------

let tabSyncDebounceTimer: ReturnType<typeof setTimeout> | undefined;
let tabSyncRunning = false;
let lastTabSyncState: TabSyncState | undefined;
const TAB_SYNC_DEBOUNCE_MS = 100;
const TAB_SYNC_DEFERRED_RETRY_BASE_MS = 750;
const TAB_SYNC_DEFERRED_RETRY_MAX_MS = 5_000;
const TAB_SYNC_MAX_DEFERRED_RETRIES = 8;
let latestTabSyncGeneration = 0;
let tabSyncDeferredRetryKey: string | undefined;
let tabSyncDeferredRetryCount = 0;

interface PlannedTabSyncExecution {
    root: string;
    activeFsPath: string;
    planned: NonNullable<ReturnType<typeof buildTabChangeCommand>>;
}

function planCurrentTabChange(): PlannedTabSyncExecution | null {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return null;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) return null;

    const visibleColumns = collectVisibleMarkdownColumns(root);
    const visibleMd = flattenVisibleColumns(visibleColumns);
    const activeFsPath = editor.document.uri.fsPath;
    const activeFile = relativePath(root, activeFsPath);
    const planned = buildTabChangeCommand({
        activeFile,
        visibleMd,
        visibleColumns,
        previous: lastTabSyncState,
    });
    if (planned === null) return null;
    return { root, activeFsPath, planned };
}

function requestTabSync(delayMs = TAB_SYNC_DEBOUNCE_MS): number {
    const requestedGeneration = ++latestTabSyncGeneration;
    if (tabSyncDebounceTimer) clearTimeout(tabSyncDebounceTimer);
    tabSyncDebounceTimer = setTimeout(() => {
        void drainTabSync(requestedGeneration);
    }, delayMs);
    return requestedGeneration;
}

function resetTabSyncDeferredRetry(): void {
    tabSyncDeferredRetryKey = undefined;
    tabSyncDeferredRetryCount = 0;
}

function nextTabSyncDeferredRetryDelay(retryCount: number): number {
    const step = Math.max(retryCount - 1, 0);
    const delay = TAB_SYNC_DEFERRED_RETRY_BASE_MS * (2 ** Math.min(step, 3));
    return Math.min(delay, TAB_SYNC_DEFERRED_RETRY_MAX_MS);
}

function registerTabSyncDeferredRetry(execution: PlannedTabSyncExecution): number | null {
    const retryKey = `${execution.planned.nextState.visibleSignature}\u0000${execution.planned.nextState.activeFile}`;
    if (tabSyncDeferredRetryKey === retryKey) {
        tabSyncDeferredRetryCount += 1;
    } else {
        tabSyncDeferredRetryKey = retryKey;
        tabSyncDeferredRetryCount = 1;
    }
    if (tabSyncDeferredRetryCount > TAB_SYNC_MAX_DEFERRED_RETRIES) {
        return null;
    }
    return nextTabSyncDeferredRetryDelay(tabSyncDeferredRetryCount);
}

async function drainTabSync(requestedGeneration: number): Promise<void> {
    if (requestedGeneration !== latestTabSyncGeneration) return;
    if (tabSyncRunning) return;
    tabSyncRunning = true;

    let startedGeneration = requestedGeneration;
    let retryAlreadyScheduled = false;
    try {
        while (true) {
            startedGeneration = latestTabSyncGeneration;
            const execution = planCurrentTabChange();
            if (execution === null) {
                if (!shouldReplayQueuedTabChange(startedGeneration, latestTabSyncGeneration)) break;
                continue;
            }

            try {
                let output = '';
                if (execution.planned.command.kind === 'focus') {
                    const { cwd, relativePath: rel } = resolveProject(execution.root, execution.activeFsPath);
                    output = await runCli(buildImmediateFocusCommandArgs(rel), cwd, { timeoutMs: FOCUS_CLI_TIMEOUT_MS });
                } else {
                    output = await runCli(execution.planned.command.args, execution.root, { timeoutMs: SYNC_CLI_TIMEOUT_MS });
                }
                const result = analyzeTabSyncCommandResult(
                    execution.planned.command,
                    0,
                    output,
                );
                if (result.applied) {
                    lastTabSyncState = execution.planned.nextState;
                    resetTabSyncDeferredRetry();
                } else if (result.shouldRetry) {
                    if (shouldScheduleDeferredTabSyncRetry(startedGeneration, latestTabSyncGeneration)) {
                        const delayMs = registerTabSyncDeferredRetry(execution);
                        if (delayMs !== null) {
                            requestTabSync(delayMs);
                            retryAlreadyScheduled = true;
                        }
                    }
                    break;
                }
            } catch {
                resetTabSyncDeferredRetry();
                // Silently ignore tab sync errors
            }

            if (!shouldReplayQueuedTabChange(startedGeneration, latestTabSyncGeneration)) break;
        }
    } finally {
        tabSyncRunning = false;
        if (!retryAlreadyScheduled && shouldReplayQueuedTabChange(startedGeneration, latestTabSyncGeneration)) {
            requestTabSync(0);
        }
    }
}

function focusExistingPaneForTabChange(execution: PlannedTabSyncExecution, generation: number): void {
    void (async () => {
        if (generation !== latestTabSyncGeneration) return;
        try {
            const { cwd, relativePath: rel } = resolveProject(execution.root, execution.activeFsPath);
            await runCli(buildImmediateFocusCommandArgs(rel), cwd, { timeoutMs: FOCUS_CLI_TIMEOUT_MS });
            if (generation === latestTabSyncGeneration) {
                showHint(`Focus: ${rel}`);
            }
        } catch {
            // Missing or stale panes are expected during tab churn; background sync owns reconciliation.
        }
    })();
}

function onTabChanged(): void {
    const execution = planCurrentTabChange();
    if (execution === null) return;
    const generation = requestTabSync();
    focusExistingPaneForTabChange(execution, generation);
}

// ---------------------------------------------------------------------------
// Feature 5: Prompt Polling
// ---------------------------------------------------------------------------

let promptPollInterval: ReturnType<typeof setInterval> | undefined;
let promptPollRoot: string | undefined;
let currentPromptKey: string | undefined;
let answeredPromptKey: string | undefined;

function ensurePromptPolling(root: string): void {
    if (promptPollInterval && promptPollRoot === root) return;

    // If root changed, stop previous poller
    if (promptPollInterval) {
        clearInterval(promptPollInterval);
    }

    promptPollRoot = root;
    currentPromptKey = undefined;
    answeredPromptKey = undefined;

    promptPollInterval = setInterval(() => pollPrompts(root), 1500);
}

function stopPromptPolling(): void {
    if (promptPollInterval) {
        clearInterval(promptPollInterval);
        promptPollInterval = undefined;
    }
    promptPollRoot = undefined;
    currentPromptKey = undefined;
    answeredPromptKey = undefined;
    trackedFiles.clear();
}

async function pollPrompts(root: string): Promise<void> {
    for (const fsPath of trackedFiles) {
        const doc = vscode.workspace.textDocuments.find(d => d.uri.fsPath === fsPath);
        if (doc && doc.isDirty) {
            return;
        }
    }

    let stdout: string;
    try {
        stdout = await runCli(['prompt', '--all'], root);
    } catch {
        return; // silently ignore poll errors
    }

    let entries: PromptAllEntry[];
    try {
        entries = JSON.parse(stdout);
        if (!Array.isArray(entries)) return;
    } catch {
        return;
    }

    const normalized = normalizePromptEntries(entries);

    // Clear answered key if it's no longer in the active set
    if (answeredPromptKey && !normalized.some(e => e.key === answeredPromptKey)) {
        answeredPromptKey = undefined;
    }

    // Filter out recently answered
    const active = answeredPromptKey
        ? normalized.filter(e => e.key !== answeredPromptKey)
        : normalized;

    if (active.length === 0) {
        currentPromptKey = undefined;
        return;
    }

    // Stick with current prompt if it's still active
    if (currentPromptKey && active.some(e => e.key === currentPromptKey)) {
        return;
    }

    // Pick next prompt
    const next = active[0];
    currentPromptKey = next.key;

    const fileName = next.file.split('/').pop() || next.file;
    const totalActive = active.length;
    const prefix = `[${fileName}] `;
    const suffix = totalActive > 1 ? `  (${totalActive} prompts pending)` : '';
    const question = `${prefix}${next.info.question || 'Permission required'}${suffix}`;

    const options = next.info.options!;
    const items = buildPromptQuickPickItems(options, next.info.selected);

    const selected = await vscode.window.showQuickPick(items, {
        title: 'Agent Doc Prompt',
        placeHolder: question,
    });

    if (selected) {
        answeredPromptKey = currentPromptKey;
        currentPromptKey = undefined;
        try {
            await runCli(['prompt', '--answer', selected.answerIndex.toString(), next.file], next.cwd ?? root);
        } catch (err: any) {
            answeredPromptKey = undefined;
            showError(`prompt --answer failed: ${err.message}`);
        }
    } else {
        // User dismissed — don't re-show the same prompt until it changes
        currentPromptKey = undefined;
    }
}

// ---------------------------------------------------------------------------
// Feature 6: Popup Menu
// ---------------------------------------------------------------------------

async function popupMenuAction(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const selected = await vscode.window.showQuickPick(buildPrimaryPopupMenuItems(), {
        title: 'Agent Doc',
        placeHolder: 'Select action',
    });

    if (!selected) return;

    switch (selected.id) {
        case 'submit':
            await submitAction();
            break;
        case 'claim':
            await claimAction();
            break;
        case 'fixDocument':
            await fixDocumentAction();
            break;
        case 'compactExchange':
            await compactExchangeAction();
            break;
        case 'syncLayout':
            await syncLayoutAction();
            break;
        case 'loadTmuxWindow':
            await loadTmuxWindowAction();
            break;
        case 'status':
            await showSessionStatusAction();
            break;
        case 'restartSupervisor':
            await restartSessionAction();
            break;
        case 'restartAgent':
            await restartAgentAction();
            break;
        case 'clear':
            await clearSessionContextAction();
            break;
        case 'interruptClear':
            await interruptClearSessionContextAction();
            break;
        case 'doctor':
            await copySessionDiagnosticsAction();
            break;
        case 'more': {
            const overflow = await vscode.window.showQuickPick(buildOverflowPopupMenuItems(), {
                title: 'Agent Doc More Actions',
                placeHolder: 'Select action',
            });
            if (!overflow) return;
            switch (overflow.id) {
                case 'runWithJunie':
                    await runWithJunieAction();
                    break;
                case 'forceClaim':
                    await forceClaimAction();
                    break;
                case 'stopAgent':
                    await stopAgentAction();
                    break;
                case 'cancelTurn':
                    await cancelTurnAction();
                    break;
                case 'killSupervisor':
                    await killSupervisorAction();
                    break;
                case 'resyncFixSessions':
                    await resyncFixSessionsAction();
                    break;
                case 'gcStaleSessions':
                    await gcStaleSessionsAction();
                    break;
            }
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// IPC Patch Watcher
// ---------------------------------------------------------------------------

/**
 * Watches `.agent-doc/patches/` for JSON patch files written by `agent-doc write --ipc`
 * and applies them via VS Code's WorkspaceEdit API. This avoids "externally modified"
 * dialogs and preserves cursor position / undo stack.
 *
 * Flow:
 * 1. `agent-doc write --ipc` writes `<hash>.json` to `.agent-doc/patches/`
 * 2. FileSystemWatcher detects the new file
 * 3. Reads JSON, finds/opens the target document, applies patches
 * 4. Writes ack-content and deletes the JSON file (ACK)
 * 5. agent-doc polls for deletion and updates the snapshot
 */

interface IpcComponentPatch {
    component: string;
    content: string;
    op?: string;
    node_id?: string;
    boundary_id?: string;
    ensure_boundary?: boolean;
}

interface IpcNodePatch {
    component: string;
    node_key: string;
    op: string;
    content?: string;
    expected_content?: string;
    expected_content_hash?: string;
    before?: string;
    after?: string;
    order?: string[];
}

interface IpcPatch {
    file: string;
    editor_id?: string;
    origin_editor_id?: string;
    patches: IpcComponentPatch[];
    node_patches?: IpcNodePatch[];
    unmatched: string;
    frontmatter?: string;
    fullContent?: string;
    reposition_boundary?: boolean;
    reposition_boundary_id?: string;
    preserve_head?: boolean;
    normalize_prefix_lines?: string[];
    patch_id?: string;
    expected_content_hash?: string;
    expected_content_len?: number;
}

class PatchWatcher implements vscode.Disposable {
    private watcher: vscode.FileSystemWatcher | undefined;
    private signalWatcher: vscode.FileSystemWatcher | undefined;
    private saveSignalWatcher: vscode.FileSystemWatcher | undefined;
    private liveBufferSignalWatcher: vscode.FileSystemWatcher | undefined;
    private typingListener: vscode.Disposable | undefined;
    private openListener: vscode.Disposable | undefined;
    private crdtReplicas: CrdtReplicaManager | undefined;
    private patchesDir: string | undefined;
    private outputChannel: vscode.OutputChannel;
    /** Track last typing time per file for debounce */
    private lastTypingTime = new Map<string, number>();
    /** Patch files delayed because the target document is still being edited. */
    private pendingPatchRetries = new Set<string>();
    /** Documents for which this VS Code instance has published plugin-owner proof. */
    private ownedDocs = new Set<string>();
    /** Coalesced full-buffer live reports; never run from onDidChangeTextDocument. */
    private liveBufferReportTimers = new Map<string, ReturnType<typeof setTimeout>>();
    /** Native typing markers are queued off the text-change listener path. */
    private nativeChangeTimers = new Map<string, ReturnType<typeof setTimeout>>();
    /** CRDT local forwards are queued off the text-change listener path. */
    private crdtLocalChangeTimers = new Map<string, Set<ReturnType<typeof setTimeout>>>();
    /** Native editor-op writes are queued off the text-change listener path. */
    private pendingEditorOpReports: PendingEditorOpReport[] = [];
    private editorOpReportTimer: ReturnType<typeof setTimeout> | undefined;

    constructor() {
        this.outputChannel = vscode.window.createOutputChannel('Agent Doc Patches');
    }

    start(): void {
        const patchesDir = this.findPatchesDir();
        if (!patchesDir) {
            this.outputChannel.appendLine('PatchWatcher: no .agent-doc/patches/ directory found');
            return;
        }

        this.patchesDir = patchesDir;

        // Ensure the directory exists
        try {
            fs.mkdirSync(patchesDir, { recursive: true });
        } catch {
            // already exists or can't create — either way, continue
        }

        // Watch for new .json files in the patches directory
        const pattern = new vscode.RelativePattern(patchesDir, '*.json');
        this.watcher = vscode.workspace.createFileSystemWatcher(pattern, false, true, true);
        this.watcher.onDidCreate((uri) => this.onPatchFileCreated(uri));

        // Watch for VCS refresh signal (created by agent-doc commit)
        const signalPattern = new vscode.RelativePattern(patchesDir, 'vcs-refresh.signal');
        this.signalWatcher = vscode.workspace.createFileSystemWatcher(signalPattern, false, false, true);
        this.signalWatcher.onDidCreate(() => this.onVcsRefreshSignal(patchesDir));
        this.signalWatcher.onDidChange(() => this.onVcsRefreshSignal(patchesDir));

        // Watch for save-document signal (#jbeditorsavedrift-vscode): the binary
        // detected the editor buffer is ahead of disk (carry-forward drift) and
        // asks us to flush it. VS Code parity for the JB plugin's socket
        // `save_document` handler — delivered as a file signal because the
        // extension watches `.agent-doc/patches/` instead of the socket.
        const saveSignalPattern = new vscode.RelativePattern(patchesDir, 'save-document.signal');
        this.saveSignalWatcher = vscode.workspace.createFileSystemWatcher(saveSignalPattern, false, false, true);
        this.saveSignalWatcher.onDidCreate(() => this.onSaveDocumentSignal(patchesDir));
        this.saveSignalWatcher.onDidChange(() => this.onSaveDocumentSignal(patchesDir));

        // Watch for read-only live-buffer publication requests. This mirrors the
        // JB plugin's socket `publish_live_buffer` path, but stays on VS Code's
        // existing file-signal transport.
        const liveBufferSignalPattern = new vscode.RelativePattern(patchesDir, 'publish-live-buffer.signal');
        this.liveBufferSignalWatcher = vscode.workspace.createFileSystemWatcher(liveBufferSignalPattern, false, false, true);
        this.liveBufferSignalWatcher.onDidCreate(() => this.onPublishLiveBufferSignal(patchesDir));
        this.liveBufferSignalWatcher.onDidChange(() => this.onPublishLiveBufferSignal(patchesDir));

        const projectRoot = path.dirname(path.dirname(patchesDir));
        this.crdtReplicas = new CrdtReplicaManager({
            projectRoot,
            identity: EDITOR_ID,
            listDocuments: () => this.currentProjectMarkdownSnapshots(projectRoot),
            applyText: (filePath, text, expectedText) => this.applyCrdtReplicaText(filePath, text, expectedText),
            logger: {
                debug: (message) => this.outputChannel.appendLine(message),
                warn: (message) => this.outputChannel.appendLine(message),
            },
        });
        this.crdtReplicas.start();
        this.openListener = vscode.workspace.onDidOpenTextDocument((document) => {
            if (!this.targetsProjectMarkdown(document, projectRoot)) return;
            const text = document.getText();
            seedEditorOpShadow(document.uri.fsPath, text);
            void this.crdtReplicas?.attachDocument(document.uri.fsPath, text);
            this.scheduleLiveBufferReport(document, projectRoot);
        });

        // Track typing events for debounce (TS fallback + FFI)
        this.typingListener = vscode.workspace.onDidChangeTextDocument((e) => {
            if (e.document.languageId === 'markdown' && e.contentChanges.length > 0) {
                const fsPath = e.document.uri.fsPath;
                const remoteCrdtApply = this.crdtReplicas?.isApplyingRemote(fsPath) ?? false;
                if (!remoteCrdtApply) {
                    this.lastTypingTime.set(fsPath, Date.now());
                }
                const eventProjectRoot = this.patchesDir
                    ? path.dirname(path.dirname(this.patchesDir))
                    : undefined;
                this.scheduleNativeDocumentChanged(fsPath, eventProjectRoot);
                this.scheduleLiveBufferReport(e.document, eventProjectRoot);
                const changes: ReplicaTextChange[] = e.contentChanges.map((change) => ({
                    rangeOffset: change.rangeOffset,
                    rangeLength: change.rangeLength,
                    text: change.text,
                }));
                this.scheduleCrdtLocalChangeDelta(fsPath, changes);
                // #qnodemerge4wire Phase 4: report the real editor op so a concurrent
                // agent merge aligns to the user's actual edit boundaries.
                if (!remoteCrdtApply) {
                    this.scheduleEditorOpReport(fsPath, e.contentChanges, eventProjectRoot);
                }
            }
        });

        this.outputChannel.appendLine(`PatchWatcher: watching ${patchesDir}`);

        // Process any existing patch files and signals on startup
        this.processVcsRefreshSignal(patchesDir);
        void this.processSaveDocumentSignal(patchesDir);
        void this.processPublishLiveBufferSignal(patchesDir);
        this.processPendingPatches(patchesDir);

        // #yzer / #evmhplugin: activation is the VS Code analog of the JB plugin's
        // IPC (re)connect — the editor just opened a buffer the binary may have
        // advanced past (committed content the buffer never saw) while VS Code was
        // closed. Realtime cutover keeps visible buffers editor-owned here: the
        // binary FFI may report stale state, but this extension no longer mutates
        // open buffers as a reconnect repair.
        void this.reconcileStaleBuffersOnReconnect(patchesDir);
    }

    /**
     * #yzer / #evmhplugin: reconcile every open markdown buffer under this
     * patches-dir root whose editor buffer is PROVABLY stale committed content
     * (the binary advanced disk/HEAD while VS Code was closed). Realtime cutover
     * disables editor-open reconnect repair writes; `reread_disk` decisions are
     * logged and the buffer is kept editor-owned.
     */
    private async reconcileStaleBuffersOnReconnect(patchesDir: string): Promise<void> {
        const root = path.dirname(path.dirname(patchesDir));
        for (const doc of vscode.workspace.textDocuments) {
            if (doc.languageId !== 'markdown') continue;
            const filePath = doc.uri.fsPath;
            if (!filePath.startsWith(root + path.sep)) continue;
            let decision;
            try {
                decision = native.reconnectBufferDecision(root, filePath, doc.getText());
            } catch (e: any) {
                this.outputChannel.appendLine(`reconnect: decision FFI failed for ${filePath}: ${e.message}`);
                continue;
            }
            if (!decision || decision.decision !== 'reread_disk' || typeof decision.content !== 'string') {
                if (decision && decision.decision !== 'in_sync' && decision.decision !== 'keep_buffer') {
                    this.outputChannel.appendLine(`reconnect: ${filePath} decision=${decision.decision} (buffer kept) #yzer`);
                }
                continue;
            }
            this.outputChannel.appendLine(`reconnect: reread_disk repair is disabled for ${filePath}; buffer kept #yzer`);
        }
    }

    private findPatchesDir(): string | undefined {
        // Walk up from workspace root to find .agent-doc/patches/
        const roots = vscode.workspace.workspaceFolders;
        if (!roots || roots.length === 0) return undefined;

        let dir = roots[0].uri.fsPath;
        const root = path.parse(dir).root;

        while (dir !== root) {
            const candidate = path.join(dir, '.agent-doc', 'patches');
            if (fs.existsSync(path.join(dir, '.agent-doc'))) {
                return candidate;
            }
            dir = path.dirname(dir);
        }

        // Fallback: use workspace root
        return path.join(roots[0].uri.fsPath, '.agent-doc', 'patches');
    }

    private onVcsRefreshSignal(patchesDir: string): void {
        this.processVcsRefreshSignal(patchesDir);
    }

    private processVcsRefreshSignal(patchesDir: string): void {
        const signalFile = path.join(patchesDir, 'vcs-refresh.signal');
        try {
            if (fs.existsSync(signalFile)) {
                fs.unlinkSync(signalFile);
                // Trigger VS Code's git extension to refresh
                vscode.commands.executeCommand('git.refresh');
                this.outputChannel.appendLine('VCS refresh triggered after external commit');
            }
        } catch {
            // signal file may have been consumed by another process
        }
    }

    private onSaveDocumentSignal(patchesDir: string): void {
        void this.processSaveDocumentSignal(patchesDir);
    }

    private onPublishLiveBufferSignal(patchesDir: string): void {
        void this.processPublishLiveBufferSignal(patchesDir);
    }

    /**
     * Handle a legacy `save-document.signal` file written by the binary. Realtime
     * cutover disables plugin-driven saves; the signal is consumed and logged so
     * a stale repair surface cannot flush a visible buffer behind the controller.
     */
    private async processSaveDocumentSignal(patchesDir: string): Promise<void> {
        const signalFile = path.join(patchesDir, 'save-document.signal');
        let raw: string;
        try {
            raw = fs.readFileSync(signalFile, 'utf8');
        } catch {
            // Signal absent or already consumed by another watcher pass.
            return;
        }
        // Consume the signal immediately so a re-fire does not re-process it.
        try {
            fs.unlinkSync(signalFile);
        } catch {
            // Already consumed by a concurrent process.
        }

        const signal = parseSaveDocumentSignal(raw);
        if (!signal) {
            this.outputChannel.appendLine('save_document: malformed or empty signal payload, ignoring');
            return;
        }
        this.outputChannel.appendLine(`save_document IPC is disabled for ${signal.file}`);
    }

    /**
     * Handle a read-only live-buffer publication request from the binary. The
     * signal asks VS Code to republish its current visible-buffer proof; it must
     * not mutate or save the document.
     */
    private async processPublishLiveBufferSignal(patchesDir: string): Promise<void> {
        const signalFile = path.join(patchesDir, 'publish-live-buffer.signal');
        let raw: string;
        try {
            raw = fs.readFileSync(signalFile, 'utf8');
        } catch {
            return;
        }
        try {
            fs.unlinkSync(signalFile);
        } catch {
            // Already consumed by a concurrent watcher pass.
        }

        let parsed: any;
        try {
            parsed = JSON.parse(raw);
        } catch {
            this.outputChannel.appendLine('publish_live_buffer: malformed signal payload, ignoring');
            return;
        }
        const file = typeof parsed?.file === 'string' ? parsed.file : undefined;
        if (!file) {
            this.outputChannel.appendLine('publish_live_buffer: missing file field, ignoring');
            return;
        }

        const projectRoot = path.dirname(path.dirname(patchesDir));
        const document = vscode.workspace.textDocuments.find((doc) => doc.uri.fsPath === file);
        if (!document || !this.targetsProjectMarkdown(document, projectRoot)) {
            this.outputChannel.appendLine(`publish_live_buffer: no open markdown document for ${file}`);
            return;
        }
        this.publishLiveBufferNow(document, projectRoot);
    }

    /**
     * Write proven editor-apply content to the ack-content sidecar
     * (`.agent-doc/ack-content/<patch_id>.md`). No-op without a patch_id.
     */
    private writeAckContent(
        patchId: string | undefined,
        content: string,
        patchesDir: string,
    ): boolean {
        if (!patchId) {
            return true;
        }
        const sidecar = ackContentSidecarPath(patchesDir, patchId);
        try {
            fs.mkdirSync(path.dirname(sidecar), { recursive: true });
            fs.writeFileSync(sidecar, content);
            return true;
        } catch (e) {
            this.outputChannel.appendLine(`save_document: ack-content write failed: ${e}`);
            return false;
        }
    }

    private processPendingPatches(dir: string): void {
        try {
            const files = fs.readdirSync(dir).filter(f => f.endsWith('.json'));
            for (const file of files) {
                const uri = vscode.Uri.file(path.join(dir, file));
                this.onPatchFileCreated(uri);
            }
        } catch {
            // directory might not exist yet
        }
    }

    private scheduleCrdtLocalChangeDelta(fsPath: string, changes: readonly ReplicaTextChange[]): void {
        const timer = setTimeout(() => {
            const timers = this.crdtLocalChangeTimers.get(fsPath);
            timers?.delete(timer);
            if (timers?.size === 0) this.crdtLocalChangeTimers.delete(fsPath);
            const crdtForward = this.crdtReplicas?.handleLocalChangeDelta(fsPath, changes);
            crdtForward?.catch((err: any) => {
                this.outputChannel.appendLine(`crdt-replica: local change skipped for ${fsPath}: ${err?.message ?? err}`);
            });
        }, 0);
        let timers = this.crdtLocalChangeTimers.get(fsPath);
        if (!timers) {
            timers = new Set();
            this.crdtLocalChangeTimers.set(fsPath, timers);
        }
        timers.add(timer);
    }

    private targetsThisEditor(patch: IpcPatch): boolean {
        if (patch.editor_id && patch.editor_id !== EDITOR_ID) {
            return false;
        }
        if (!patch.editor_id && patch.origin_editor_id === EDITOR_ID) {
            return false;
        }
        return true;
    }

    private projectRoot(): string | undefined {
        return this.patchesDir ? path.dirname(path.dirname(this.patchesDir)) : undefined;
    }

    private ownsDocument(filePath: string, projectRoot?: string): boolean {
        const owns = native.pluginOwnerTryAcquire(filePath, EDITOR_ID, process.pid, projectRoot);
        if (owns) {
            this.ownedDocs.add(filePath);
        } else {
            this.ownedDocs.delete(filePath);
        }
        return owns;
    }

    private async onPatchFileCreated(uri: vscode.Uri): Promise<void> {
        try {
            const raw = fs.readFileSync(uri.fsPath, 'utf-8');
            const patch = JSON.parse(raw) as IpcPatch;

            if (!patch.file) {
                this.outputChannel.appendLine(`PatchWatcher: invalid patch (no file field): ${uri.fsPath}`);
                return;
            }

            if (!this.targetsThisEditor(patch)) {
                this.outputChannel.appendLine(`PatchWatcher: ignoring patch for editor_id ${patch.editor_id ?? '-'}: ${path.basename(uri.fsPath)}`);
                return;
            }

            if (consumeClaimedPatch(patch.patch_id, patch.file)) {
                this.outputChannel.appendLine(`PatchWatcher: claimed patch_id ${patch.patch_id} already closed out locally, deleting ${path.basename(uri.fsPath)}`);
                try { fs.unlinkSync(uri.fsPath); } catch { /* already consumed */ }
                return;
            }

            if (isPatchAlreadyApplied(patch.file, uri.fsPath)) {
                this.outputChannel.appendLine(`PatchWatcher: snapshot newer than patch file, deleting stale ${path.basename(uri.fsPath)}`);
                try { fs.unlinkSync(uri.fsPath); } catch { /* already consumed */ }
                return;
            }

            if ((patch.fullContent ?? '') !== '') {
                this.outputChannel.appendLine(`PatchWatcher: full content IPC is disabled, deleting stale/foreign ${path.basename(uri.fsPath)}`);
                try { fs.unlinkSync(uri.fsPath); } catch { /* already consumed */ }
                return;
            }

            // Handle reposition-only signals with typing debounce
            if (isPureRepositionSignal(patch)) {
                this.repositionBoundaryWithDebounce(
                    patch.file,
                    uri.fsPath,
                    patch.reposition_boundary_id,
                    0,
                    patch.preserve_head ?? false,
                );
                return;
            }

            const projectRoot = this.patchesDir ? path.dirname(path.dirname(this.patchesDir)) : undefined;
            if (!this.ownsDocument(patch.file, projectRoot)) {
                this.outputChannel.appendLine(`PatchWatcher: not the live owner of ${patch.file}, leaving patch for owner instance: ${uri.fsPath}`);
                return;
            }
            const stateGeneration = native.recordEditorPatchQueued(patch.file, patch.patch_id, projectRoot);
            if (!this.awaitIdleBeforeDocumentMutation(patch.file, 'file patch', uri.fsPath)) {
                native.recordEditorRetryRequested(
                    patch.file,
                    patch.patch_id,
                    stateGeneration,
                    'typing_active',
                    projectRoot,
                );
                return;
            }

            const applied = await this.applyPatch(patch, uri.fsPath);

            if (applied) {
                native.recordEditorAckObserved(patch.file, patch.patch_id, stateGeneration, projectRoot);
                // ACK: delete the patch file
                try {
                    fs.unlinkSync(uri.fsPath);
                } catch (e: any) {
                    this.outputChannel.appendLine(`PatchWatcher: failed to delete patch file: ${e.message}`);
                }
            } else {
                native.recordEditorRetryRequested(
                    patch.file,
                    patch.patch_id,
                    stateGeneration,
                    'file_apply_failed',
                    projectRoot,
                );
                this.outputChannel.appendLine(`PatchWatcher: patch not applied, leaving for retry: ${uri.fsPath}`);
            }
        } catch (e: any) {
            this.outputChannel.appendLine(`PatchWatcher: failed to process ${uri.fsPath}: ${e.message}`);
        }
    }

    /**
     * Reposition boundary marker with typing debounce.
     * Waits until the user stops typing (500ms idle) before applying,
     * up to a 5s timeout. ACKs the patch file after applying.
     *
     * Uses FFI `agent_doc_reposition_boundary_to_end` when available,
     * falls back to TS implementation.
     */
    private repositionBoundaryWithDebounce(
        filePath: string,
        patchFilePath: string,
        boundaryId?: string,
        elapsed = 0,
        preserveHead = false,
    ): void {
        const debounceMs = 500;
        const timeoutMs = 5000;
        const projectRoot = this.patchesDir ? path.dirname(path.dirname(this.patchesDir)) : undefined;

        // Check typing idle — prefer FFI, fall back to TS timestamp
        const ffiIdle = native.isAvailable(projectRoot)
            ? native.isIdle(filePath, debounceMs, projectRoot)
            : null;
        const tsIdle = (Date.now() - (this.lastTypingTime.get(filePath) ?? 0)) >= debounceMs;
        const idle = ffiIdle ?? tsIdle;

        if (!idle && elapsed < timeoutMs) {
            setTimeout(() => {
                this.repositionBoundaryWithDebounce(filePath, patchFilePath, boundaryId, elapsed + 500, preserveHead);
            }, 500);
            return;
        }

        if (!idle) {
            this.outputChannel.appendLine(`PatchWatcher: typing debounce timed out before reposition for ${filePath}; retrying`);
            setTimeout(() => {
                this.repositionBoundaryWithDebounce(filePath, patchFilePath, boundaryId, 0, preserveHead);
            }, debounceMs);
            return;
        }

        // Apply reposition via WorkspaceEdit (cursor-safe)
        const fileUri = vscode.Uri.file(filePath);
        vscode.workspace.openTextDocument(fileUri).then(async (document) => {
            const content = document.getText();
            // Prefer FFI reposition, fall back to TS
            const repositioned = preserveHead
                ? (native.repositionBoundaryToEndPreserveHead(content, projectRoot, boundaryId)
                    ?? this.repositionBoundaryToEndPreserveHeadTs(content, 'exchange', boundaryId))
                : (native.repositionBoundaryToEnd(content, projectRoot, boundaryId)
                    ?? this.repositionBoundaryToEndTs(content, 'exchange', boundaryId));
            const proof = createEditorApplyProof(content, document.version);
            if (repositioned && repositioned !== content) {
                if (!isEditorApplyProofCurrent(proof, document.getText(), document.version)) {
                    this.outputChannel.appendLine(`PatchWatcher: stale editor generation before reposition for ${filePath}; retrying`);
                    this.schedulePatchRetry(patchFilePath);
                    return;
                }
                await this.applyMinimalTextEdit(document, repositioned);
            }
            // ACK: delete the patch file
            try { fs.unlinkSync(patchFilePath); } catch { /* already consumed */ }
        }).then(undefined, (err: any) => {
            this.outputChannel.appendLine(`PatchWatcher: reposition failed: ${err.message}`);
            try { fs.unlinkSync(patchFilePath); } catch { /* best effort ACK */ }
        });
    }

    private repositionBoundaryToEndTs(doc: string, component: string, boundaryId?: string): string | null {
        return repositionBoundaryToEnd(doc, component, boundaryId);
    }

    private repositionBoundaryToEndPreserveHeadTs(doc: string, component: string, boundaryId?: string): string | null {
        return repositionBoundaryToEndPreserveHead(doc, component, boundaryId);
    }

    private awaitIdleBeforeDocumentMutation(filePath: string, operation: string, patchFilePath?: string): boolean {
        const debounceMs = 500;
        const timeoutMs = 5000;
        const projectRoot = this.patchesDir ? path.dirname(path.dirname(this.patchesDir)) : undefined;
        const nativeIdle = native.awaitIdle(filePath, debounceMs, timeoutMs, projectRoot);
        const tsIdle = (Date.now() - (this.lastTypingTime.get(filePath) ?? 0)) >= debounceMs;
        if (nativeIdle && tsIdle) {
            return true;
        }

        this.outputChannel.appendLine(`PatchWatcher: typing debounce timed out before ${operation} for ${filePath}`);
        if (patchFilePath) {
            this.schedulePatchRetry(patchFilePath);
        }
        return false;
    }

    private schedulePatchRetry(patchFilePath: string): void {
        if (this.pendingPatchRetries.has(patchFilePath)) return;
        this.pendingPatchRetries.add(patchFilePath);
        setTimeout(() => {
            this.pendingPatchRetries.delete(patchFilePath);
            if (fs.existsSync(patchFilePath)) {
                this.onPatchFileCreated(vscode.Uri.file(patchFilePath));
            }
        }, 500);
    }

    private async applyPatch(patch: IpcPatch, patchFilePath?: string): Promise<boolean> {
        const fileUri = vscode.Uri.file(patch.file);

        // Find or open the target document
        let document: vscode.TextDocument;
        try {
            document = await vscode.workspace.openTextDocument(fileUri);
        } catch (e: any) {
            this.outputChannel.appendLine(`PatchWatcher: could not open ${patch.file}: ${e.message}`);
            return false;
        }

        const baselineContent = document.getText();
        const proof = createEditorApplyProof(baselineContent, document.version);

        if (patch.fullContent != null && patch.fullContent !== '') {
            this.outputChannel.appendLine(`PatchWatcher: full content IPC is disabled for ${patch.file}; rejecting patch`);
            return false;
        }

        // Component-based patching (template/stream-mode documents)
        let content = baselineContent;
        const projectRoot = this.patchesDir ? path.dirname(path.dirname(this.patchesDir)) : undefined;

        // Apply frontmatter patch first
        if (patch.frontmatter) {
            content = this.applyFrontmatterPatch(content, patch.frontmatter);
        }

        const nodePatchedComponents = new Set((patch.node_patches ?? []).map(p => p.component));
        const nodePatchNativeAvailable = (patch.node_patches?.length ?? 0) > 0 && native.canApplyNodePatches(projectRoot);
        if (nodePatchNativeAvailable) {
            const nodePatched = native.applyNodePatches(content, patch.node_patches ?? [], projectRoot);
            if (nodePatched == null) {
                this.outputChannel.appendLine(`PatchWatcher: native node-patch apply rejected ${patch.file}`);
                return false;
            }
            content = nodePatched;
        }

        // Apply component patches
        for (const p of patch.patches) {
            if (nodePatchNativeAvailable && nodePatchedComponents.has(p.component)) {
                this.outputChannel.appendLine(`PatchWatcher: skipping legacy component patch for node-patched ${p.component}`);
                continue;
            }
            content = this.applyComponentPatch(content, p.component, p.content, p.op);
        }

        // Apply unmatched content to exchange or output component
        if (patch.unmatched && patch.unmatched.trim() !== '') {
            const withExchange = this.applyComponentPatch(content, 'exchange', patch.unmatched);
            if (withExchange !== content) {
                content = withExchange;
            } else {
                content = this.applyComponentPatch(content, 'output', patch.unmatched);
            }
        }

        if (patch.reposition_boundary) {
            content = native.repositionBoundaryToEnd(content, projectRoot, patch.reposition_boundary_id)
                ?? this.repositionBoundaryToEndTs(content, 'exchange', patch.reposition_boundary_id)
                ?? content;
        }

        // Apply ❯  prefix normalization after boundary reposition so prompts
        // typed after the prior boundary are in the exchange user region.
        if (patch.normalize_prefix_lines && patch.normalize_prefix_lines.length > 0) {
            content = this.normalizeExchangePrefixes(content, patch.normalize_prefix_lines);
        }

        content = annotateExchangeHeadingsAgainstBaseline(content, 'exchange', baselineContent) ?? content;
        const normalized = native.normalizeTemplateStructure(content, projectRoot);
        if (normalized == null) {
            this.outputChannel.appendLine(`PatchWatcher: native template-structure guard rejected ${patch.file}`);
            return false;
        }
        content = normalized;

        // Apply the combined edit
        if (content !== baselineContent) {
            if (!this.verifyApplyProof(document, proof, patch.file, 'component patch', patchFilePath)) {
                return false;
            }
            const ok = await this.applyMinimalTextEdit(document, content);
            if (!ok) {
                this.outputChannel.appendLine(`PatchWatcher: WorkspaceEdit failed for component patches`);
                return false;
            }
        }

        const patchesDir = patchFilePath ? path.dirname(patchFilePath) : this.patchesDir;
        if (!patchesDir) {
            this.outputChannel.appendLine(`PatchWatcher: no patches dir for ack-content ${patch.file}`);
            return false;
        }
        return this.writeAckContent(patch.patch_id, document.getText(), patchesDir);
    }

    private async applyMinimalTextEdit(document: vscode.TextDocument, targetContent: string): Promise<boolean> {
        const before = document.getText();
        const replacement = calculateMinimalReplacement(before, targetContent);
        if (replacement == null) {
            return true;
        }
        if (isFullDocumentReplacement(before, replacement)) {
            this.outputChannel.appendLine(`PatchWatcher: full-document WorkspaceEdit replacement is disabled for ${document.uri.fsPath}`);
            return false;
        }
        const range = new vscode.Range(
            document.positionAt(replacement.start),
            document.positionAt(replacement.start + replacement.deleteLength),
        );
        const edit = new vscode.WorkspaceEdit();
        edit.replace(document.uri, range, replacement.text);
        const applied = await vscode.workspace.applyEdit(edit);
        return applied && document.getText() === targetContent;
    }

    private async applyCrdtReplicaText(filePath: string, targetContent: string, expectedContent: string): Promise<boolean> {
        const document = vscode.workspace.textDocuments.find((doc) => doc.uri.fsPath === filePath);
        if (!document) return false;
        const currentContent = document.getText();
        if (currentContent === targetContent) {
            return true;
        }
        if (currentContent !== expectedContent) {
            this.outputChannel.appendLine(`PatchWatcher: stale CRDT remote update for ${filePath}; editor text advanced before apply`);
            return false;
        }
        const projectRoot = this.patchesDir
            ? path.dirname(path.dirname(this.patchesDir))
            : undefined;
        const normalized = native.normalizeTemplateStructure(targetContent, projectRoot);
        if (normalized == null) {
            this.outputChannel.appendLine(`PatchWatcher: CRDT remote update rejected by template-structure guard for ${filePath}`);
            return false;
        }
        if (normalized !== targetContent) {
            this.outputChannel.appendLine(`PatchWatcher: CRDT remote update requires template-structure repair for ${filePath}; rejecting to keep replica state coherent`);
            return false;
        }
        return this.applyMinimalTextEdit(document, targetContent);
    }

    private currentProjectMarkdownSnapshots(projectRoot: string): Array<{ filePath: string; text: string }> {
        return vscode.workspace.textDocuments
            .filter((document) => this.targetsProjectMarkdown(document, projectRoot))
            .map((document) => {
                const text = document.getText();
                seedEditorOpShadow(document.uri.fsPath, text);
                return { filePath: document.uri.fsPath, text };
            });
    }

    private scheduleNativeDocumentChanged(fsPath: string, projectRoot: string | undefined): void {
        if (this.nativeChangeTimers.has(fsPath)) return;
        const timer = setTimeout(() => {
            this.nativeChangeTimers.delete(fsPath);
            native.documentChanged(fsPath, projectRoot);
        }, 0);
        this.nativeChangeTimers.set(fsPath, timer);
    }

    private scheduleLiveBufferReport(document: vscode.TextDocument, projectRoot: string | undefined): void {
        const fsPath = document.uri.fsPath;
        const previous = this.liveBufferReportTimers.get(fsPath);
        if (previous) clearTimeout(previous);
        const timer = setTimeout(() => {
            this.liveBufferReportTimers.delete(fsPath);
            const latest = vscode.workspace.textDocuments.find((doc) => doc.uri.fsPath === fsPath);
            if (!latest || latest.languageId !== 'markdown' || latest.uri.scheme !== 'file') return;
            this.publishLiveBufferNow(latest, projectRoot);
        }, LIVE_BUFFER_REPORT_DELAY_MS);
        this.liveBufferReportTimers.set(fsPath, timer);
    }

    private publishLiveBufferNow(document: vscode.TextDocument, projectRoot: string | undefined): void {
        const fsPath = document.uri.fsPath;
        const timer = this.liveBufferReportTimers.get(fsPath);
        if (timer) {
            clearTimeout(timer);
            this.liveBufferReportTimers.delete(fsPath);
        }
        const text = document.getText();
        seedEditorOpShadow(fsPath, text);
        native.documentChangedDigestContent(fsPath, text, projectRoot, EDITOR_ID);
    }

    private scheduleEditorOpReport(
        fsPath: string,
        changes: readonly vscode.TextDocumentContentChangeEvent[],
        projectRoot: string | undefined,
    ): void {
        const report = captureEditorChangeReport(fsPath, changes, projectRoot);
        if (!report) return;
        this.pendingEditorOpReports.push(report);
        if (this.editorOpReportTimer) return;
        this.editorOpReportTimer = setTimeout(() => this.flushEditorOpReports(), 0);
    }

    private flushEditorOpReports(): void {
        this.editorOpReportTimer = undefined;
        const reports = this.pendingEditorOpReports.splice(0);
        for (const report of reports) {
            reportEditorChange(report);
        }
    }

    private targetsProjectMarkdown(document: vscode.TextDocument, projectRoot: string): boolean {
        return document.languageId === 'markdown'
            && document.uri.scheme === 'file'
            && document.uri.fsPath.startsWith(projectRoot + path.sep);
    }

    handleDocumentClosed(filePath: string): void {
        native.pluginOwnerRelease(filePath, EDITOR_ID, this.projectRoot());
        this.ownedDocs.delete(filePath);
        const timer = this.liveBufferReportTimers.get(filePath);
        if (timer) clearTimeout(timer);
        this.liveBufferReportTimers.delete(filePath);
        const nativeTimer = this.nativeChangeTimers.get(filePath);
        if (nativeTimer) clearTimeout(nativeTimer);
        this.nativeChangeTimers.delete(filePath);
        const crdtTimers = this.crdtLocalChangeTimers.get(filePath);
        if (crdtTimers) {
            for (const crdtTimer of crdtTimers) clearTimeout(crdtTimer);
        }
        this.crdtLocalChangeTimers.delete(filePath);
        this.pendingEditorOpReports = this.pendingEditorOpReports.filter((report) => report.fsPath !== filePath);
        clearEditorOpShadow(filePath);
        void this.crdtReplicas?.handleDocumentClosed(filePath);
    }

    private verifyApplyProof(
        document: vscode.TextDocument,
        proof: ReturnType<typeof createEditorApplyProof>,
        filePath: string,
        operation: string,
        patchFilePath?: string,
    ): boolean {
        if (isEditorApplyProofCurrent(proof, document.getText(), document.version)) {
            return true;
        }
        this.outputChannel.appendLine(`PatchWatcher: stale editor generation before ${operation} for ${filePath}; rejecting patch`);
        if (patchFilePath) {
            this.schedulePatchRetry(patchFilePath);
        }
        return false;
    }

    /**
     * Add ❯  prefix to user-input lines in the exchange component.
     * Only normalizes the user-input region (before the LAST boundary marker).
     * Uses the last boundary — historical cycles each leave a marker, so stopping
     * at the first one would misclassify later user-input lines as agent region.
     */
    private normalizeExchangePrefixes(doc: string, lines: string[]): string {
        if (lines.length === 0) return doc;
        const openTag = /<!-- agent:exchange(\s[^>]*)? -->/;
        const closeTag = '<!-- /agent:exchange -->';
        const boundaryPattern = /<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->/g;

        const openMatch = openTag.exec(doc);
        if (!openMatch) return doc;
        const closeIdx = doc.indexOf(closeTag, openMatch.index + openMatch[0].length);
        if (closeIdx < 0) return doc;

        const beforeExchange = doc.substring(0, openMatch.index + openMatch[0].length);
        const exchangeContent = doc.substring(openMatch.index + openMatch[0].length, closeIdx);
        const afterExchange = doc.substring(closeIdx);

        // Find the LAST boundary marker — use it as the user-region boundary
        let lastBoundaryIdx = -1;
        let m: RegExpExecArray | null;
        const re = new RegExp(boundaryPattern.source, 'g');
        while ((m = re.exec(exchangeContent)) !== null) {
            lastBoundaryIdx = m.index;
        }
        const userRegionEnd = lastBoundaryIdx >= 0 ? lastBoundaryIdx : exchangeContent.length;
        let userRegion = exchangeContent.substring(0, userRegionEnd);
        const agentRegion = exchangeContent.substring(userRegionEnd);

        // Build normalized target set once — trimEnd() absorbs trailing-whitespace
        // divergence between binary disk-side payload and editor buffer.
        const targetLines = new Set(lines.filter(l => l.trim()).map(l => l.trimEnd()));

        let inResponseBlock = false;
        const normalizedLines = userRegion.split('\n').map(docLine => {
            const trimmed = docLine.trim();
            if (/<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->/.test(trimmed)) {
                inResponseBlock = false;
                return docLine;
            }
            if (this.isExchangeResponseHeadingForPrefixRepair(trimmed)) {
                inResponseBlock = true;
                return docLine;
            }
            const normalized = docLine.trimEnd();
            const isTarget = targetLines.has(normalized);
            if (inResponseBlock) {
                if (this.startsPromptRunAfterResponseForPrefixRepair(trimmed, isTarget)) {
                    inResponseBlock = false;
                } else {
                    return docLine;
                }
            }
            if (normalized.startsWith('❯ ')) return docLine;       // already prefixed
            if (isTarget) return `❯ ${docLine}`; // match — add prefix
            return docLine;
        });
        userRegion = normalizedLines.join('\n');

        return beforeExchange + userRegion + agentRegion + afterExchange;
    }

    private isExchangeResponseHeadingForPrefixRepair(trimmed: string): boolean {
        return trimmed === '## Assistant'
            || trimmed.startsWith('### Re:')
            || trimmed.startsWith('#### Re:')
            || trimmed.startsWith('##### Re:')
            || trimmed.startsWith('###### Re:');
    }

    private startsPromptRunAfterResponseForPrefixRepair(trimmed: string, isTarget: boolean): boolean {
        const alreadyPrefixed = trimmed.startsWith('❯ ');
        const unprefixed = alreadyPrefixed ? trimmed.substring(2).trimStart() : trimmed;
        return this.lineLooksLikeFreshPromptAfterResponseForPrefixRepair(unprefixed)
            || ((alreadyPrefixed || isTarget) && !this.lineLooksLikePlainResponseAfterPromptForPrefixRepair(unprefixed));
    }

    private lineLooksLikeFreshPromptAfterResponseForPrefixRepair(trimmed: string): boolean {
        const lower = trimmed.replace(/^❯\s*/, '').trim().toLowerCase();
        return trimmed.startsWith('❯')
            || trimmed.endsWith('?')
            || lower === 'go'
            || lower === 'continue'
            || lower.startsWith('do #')
            || lower.startsWith('do [#')
            || lower.startsWith('fix #')
            || lower.startsWith('run ')
            || lower.startsWith('rerun ')
            || lower.startsWith('build ')
            || lower.startsWith('test ')
            || lower.startsWith('commit ')
            || lower.startsWith('push ')
            || lower.startsWith('verify ')
            || lower.startsWith('investigate ');
    }

    private lineLooksLikePlainResponseAfterPromptForPrefixRepair(trimmed: string): boolean {
        if (!trimmed.trim()) return false;
        if (this.lineLooksLikeFreshPromptAfterResponseForPrefixRepair(trimmed)) return false;
        if (trimmed.startsWith('- ')
            || trimmed.startsWith('* ')
            || trimmed.startsWith('Plan:')
            || trimmed.startsWith('Verification')
            || trimmed.startsWith('What changed:')
            || trimmed.startsWith('Follow-up:')
            || trimmed.startsWith('Commit / push:')
            || trimmed.startsWith('Backlog:')
            || trimmed.startsWith('`#')) {
            return true;
        }
        const lower = trimmed.toLowerCase();
        return lower.startsWith('i updated ')
            || lower.startsWith('i fixed ')
            || lower.startsWith('i added ')
            || lower.startsWith('i implemented ')
            || lower.startsWith('i left ')
            || lower.startsWith('updated ')
            || lower.startsWith('fixed ')
            || lower.startsWith('added ')
            || lower.startsWith('implemented ');
    }

    /**
     * Merge YAML key/value pairs into the document's frontmatter.
     */
    private applyFrontmatterPatch(doc: string, yamlFields: string): string {
        if (!doc.startsWith('---\n')) return doc;

        const endIdx = doc.indexOf('\n---\n', 4);
        if (endIdx < 0) return doc;

        const existingYaml = doc.substring(4, endIdx);
        const body = doc.substring(endIdx + 5); // skip \n---\n

        // Parse existing frontmatter as key/value pairs (preserve order)
        const existing = new Map<string, string>();
        const order: string[] = [];
        for (const line of existingYaml.split('\n')) {
            const colonIdx = line.indexOf(':');
            if (colonIdx > 0) {
                const key = line.substring(0, colonIdx).trim();
                const value = line.substring(colonIdx + 1).trim();
                if (!existing.has(key)) {
                    order.push(key);
                }
                existing.set(key, value);
            }
        }

        // Merge new fields
        for (const line of yamlFields.split('\n')) {
            const colonIdx = line.indexOf(':');
            if (colonIdx > 0) {
                const key = line.substring(0, colonIdx).trim();
                const value = line.substring(colonIdx + 1).trim();
                if (key) {
                    if (!existing.has(key)) {
                        order.push(key);
                    }
                    existing.set(key, value);
                }
            }
        }

        // Rebuild frontmatter
        const newYaml = order.map(k => `${k}: ${existing.get(k)}`).join('\n');
        return `---\n${newYaml}\n---\n${body}`;
    }

    /**
     * Replace content between `<!-- agent:name ... -->` and `<!-- /agent:name -->` markers.
     * Handles open tags with inline attributes (e.g., `<!-- agent:exchange patch=append -->` or `mode=append` as alias).
     * Skips matches that fall inside fenced code blocks.
     */
    private applyComponentPatch(doc: string, component: string, content: string, modeOverride?: string): string {
        // Match open tag with optional attributes: <!-- agent:NAME ... -->
        const openPattern = new RegExp(`<!-- agent:${this.escapeRegex(component)}(\\s[^>]*)? -->`, 'g');
        const closeTag = `<!-- /agent:${component} -->`;

        const codeRanges = this.findCodeBlockRanges(doc);

        // Find the first open tag match that is NOT inside a fenced code block
        let openMatch: RegExpExecArray | null = null;
        while ((openMatch = openPattern.exec(doc)) !== null) {
            const matchStart = openMatch.index;
            const insideCode = codeRanges.some(([start, end]) => matchStart >= start && matchStart < end);
            if (!insideCode) break;
        }
        if (!openMatch) return doc;

        const contentStart = openMatch.index + openMatch[0].length;

        // Find close tag that is also NOT inside a fenced code block
        let closeIdx = -1;
        let searchFrom = contentStart;
        while (true) {
            closeIdx = doc.indexOf(closeTag, searchFrom);
            if (closeIdx < 0) return doc;
            const insideCode = codeRanges.some(([start, end]) => closeIdx >= start && closeIdx < end);
            if (!insideCode) break;
            searchFrom = closeIdx + closeTag.length;
        }

        // Parse mode from inline attributes: patch= takes precedence, mode= as fallback
        const overrideMode = this.componentPatchModeOverride(modeOverride);
        let mode = overrideMode ?? 'replace';
        if (overrideMode == null && openMatch[1]) {
            const patchMatch = /patch=(\S+)/.exec(openMatch[1]);
            const modeMatch = /mode=(\S+)/.exec(openMatch[1]);
            if (patchMatch) {
                mode = patchMatch[1];
            } else if (modeMatch) {
                mode = modeMatch[1];
            }
        }

        const before = doc.substring(0, contentStart);
        const after = doc.substring(closeIdx);
        const trimmedContent = content.trimEnd();

        if (mode === 'append') {
            if (appendPatchAlreadyPresent(doc, component, content)) {
                return doc;
            }
            // Append before closing marker, preserving existing content
            const existing = doc.substring(contentStart, closeIdx);
            return before + existing.trimEnd() + '\n' + trimmedContent + '\n' + after;
        }

        if (mode === 'prepend') {
            // Prepend after opening marker, preserving existing content
            const existing = doc.substring(contentStart, closeIdx);
            return before + '\n' + trimmedContent + '\n' + existing.trimStart() + after;
        }

        // Default: replace mode
        return before + '\n' + trimmedContent + '\n' + after;
    }

    private componentPatchModeOverride(op?: string): string | undefined {
        const normalized = op?.trim().toLowerCase();
        if (normalized === 'append' || normalized === 'prepend' || normalized === 'replace') {
            return normalized;
        }
        return undefined;
    }

    /**
     * Find byte ranges of fenced code blocks in the document.
     * Returns an array of [start, end] pairs where start is the offset of the
     * opening fence line and end is the offset just past the closing fence line.
     */
    private findCodeBlockRanges(doc: string): Array<[number, number]> {
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
                // End of fenced block: include everything up to end of closing fence line
                const lineEnd = doc.indexOf('\n', match.index + match[0].length);
                const blockEnd = lineEnd >= 0 ? lineEnd + 1 : doc.length;
                ranges.push([fenceStart, blockEnd]);
                insideFence = false;
            }
        }

        return ranges;
    }

    private escapeRegex(s: string): string {
        return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    }

    dispose(): void {
        this.watcher?.dispose();
        this.signalWatcher?.dispose();
        this.saveSignalWatcher?.dispose();
        this.liveBufferSignalWatcher?.dispose();
        this.typingListener?.dispose();
        this.openListener?.dispose();
        for (const filePath of this.ownedDocs) {
            native.pluginOwnerRelease(filePath, EDITOR_ID, this.projectRoot());
        }
        this.ownedDocs.clear();
        for (const timer of this.liveBufferReportTimers.values()) {
            clearTimeout(timer);
        }
        this.liveBufferReportTimers.clear();
        for (const timer of this.nativeChangeTimers.values()) {
            clearTimeout(timer);
        }
        this.nativeChangeTimers.clear();
        for (const timers of this.crdtLocalChangeTimers.values()) {
            for (const timer of timers) clearTimeout(timer);
        }
        this.crdtLocalChangeTimers.clear();
        if (this.editorOpReportTimer) clearTimeout(this.editorOpReportTimer);
        this.editorOpReportTimer = undefined;
        this.pendingEditorOpReports = [];
        this.crdtReplicas?.dispose();
        this.outputChannel.dispose();
    }
}

let patchWatcher: PatchWatcher | undefined;
let syntaxDecorationController: SyntaxDecorationController | undefined;

// ---------------------------------------------------------------------------
// Activation / Deactivation
// ---------------------------------------------------------------------------

export function activate(context: vscode.ExtensionContext): void {
    // Feature 1: Run (Submit)
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.submit', submitAction)
    );

    // Feature 2: Claim
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.claim', claimAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.forceClaim', forceClaimAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.runWithJunie', runWithJunieAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.fixDocument', fixDocumentAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.compactExchange', compactExchangeAction)
    );

    // Feature 3: Sync Layout
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.syncLayout', syncLayoutAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.loadTmuxWindow', loadTmuxWindowAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.showSessionStatus', showSessionStatusAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.restartSession', restartSessionAction)
    );

    // #s81q: Restart Agent / Stop Agent / Kill Supervisor menu commands.
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.restartAgent', restartAgentAction)
    );
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.stopAgent', stopAgentAction)
    );
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.cancelTurn', cancelTurnAction)
    );
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.killSupervisor', killSupervisorAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.clearSessionContext', clearSessionContextAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.interruptClearSessionContext', interruptClearSessionContextAction)
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.copySessionDiagnostics', copySessionDiagnosticsAction)
    );

    // #plugin-cleanup-menu-command: project-level session-hygiene commands.
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.resyncFixSessions', resyncFixSessionsAction)
    );
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.gcStaleSessions', gcStaleSessionsAction)
    );

    // Feature 6: Popup Menu
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.popupMenu', popupMenuAction)
    );

    // Feature 4: Tab Sync (Automatic)
    context.subscriptions.push(
        vscode.window.onDidChangeActiveTextEditor(() => onTabChanged())
    );
    context.subscriptions.push(
        vscode.window.onDidChangeVisibleTextEditors(() => onTabChanged())
    );

    // Feature: File Rename Handling
    // When a session document is renamed/moved, trigger a sync so the Rust
    // CLI updates sessions.json with the new path and reuses the existing pane.
    context.subscriptions.push(
        vscode.workspace.onDidRenameFiles((event) => {
            for (const { oldUri, newUri } of event.files) {
                if (!newUri.fsPath.endsWith('.md')) continue;
                const root = getWorkspaceRoot(newUri);
                if (!root) continue;
                const newRel = relativePath(root, newUri.fsPath);
                const visibleColumns = collectVisibleMarkdownColumns(root);
                const args = buildSyncLayoutCommand(visibleColumns, newRel, false);
                args.push('--rename');
                runCli(args, root).catch(() => {});
            }
        })
    );

    context.subscriptions.push(
        vscode.workspace.onDidCloseTextDocument((document) => {
            if (document.languageId !== 'markdown') return;
            native.documentClosedForEditor(document.uri.fsPath, getWorkspaceRoot(document.uri), EDITOR_ID);
            patchWatcher?.handleDocumentClosed(document.uri.fsPath);
            // #r5at: evict the per-document reactive mirror so a reused path
            // (move/symlink/reopen) does not surface stale projection state.
            native.evictStateMirrorForFile(document.uri.fsPath);
        })
    );

    // Feature 10: Slash Command Autocomplete
    context.subscriptions.push(
        vscode.languages.registerCompletionItemProvider(
            { language: 'markdown' },
            new SlashCommandCompletionProvider(),
            '/'
        )
    );

    syntaxDecorationController = new SyntaxDecorationController();
    context.subscriptions.push(syntaxDecorationController);

    // IPC Patch Watcher
    patchWatcher = new PatchWatcher();
    patchWatcher.start();
    context.subscriptions.push(patchWatcher);

    // Status bar item cleanup
    context.subscriptions.push(statusBarItem);
    context.subscriptions.push(sessionOutputChannel);
    context.subscriptions.push(routeFailureOutputChannel);
}

export function deactivate(): void {
    // Clean up prompt polling
    stopPromptPolling();

    // Clean up tab sync debounce
    if (tabSyncDebounceTimer) {
        clearTimeout(tabSyncDebounceTimer);
        tabSyncDebounceTimer = undefined;
    }

    // Clean up status bar
    if (statusBarTimeout) {
        clearTimeout(statusBarTimeout);
        statusBarTimeout = undefined;
    }

    // Clean up patch watcher
    patchWatcher?.dispose();
    patchWatcher = undefined;

    syntaxDecorationController?.dispose();
    syntaxDecorationController = undefined;

    // Reset state
    lastTabSyncState = undefined;
    resolvedAgentDoc = null;
    commandRunning = false;
    editorCommandRegistry.resetForTest();
    for (const route of activeRoutes.values()) {
        route.controller.abort();
    }
    activeRoutes.clear();
}
