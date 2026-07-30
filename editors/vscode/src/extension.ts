import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import * as fs from 'fs';
import * as crypto from 'crypto';
import * as net from 'net';
import { execFile } from 'child_process';
import * as native from './native.js';
import * as stateMirror from './stateMirror.js';
import { createEditorApplyProof, isEditorApplyProofCurrent } from './patchGuard.js';
import { EditorIntent } from './editorIntent.js';
import { processSaveDocumentIntent } from './saveDocumentIntent.js';
import { appendPatchAlreadyPresent, calculateMinimalReplacement, isFullDocumentReplacement } from './patchPlan.js';
import {
    buildCrossSessionClaimArgs,
    parseCrossSessionReject,
    CrossSessionReject,
} from './crossSession.js';
import { buildEditorRoutePayload, buildEditorRouteCommandMessage, resolveEditorRouteTerminal } from './commandPlane.js';
import { annotateExchangeHeadingsAgainstBaseline, repositionBoundaryToEnd, repositionBoundaryToEndPreserveHead } from './reposition.js';
import {
    buildBusySessionRestartBlockedMessage,
    buildBusySessionClearBlockedMessage,
    buildForcedRestartSupervisorCommandArgs,
    buildRouteFailurePresentation,
    buildSessionCommandArgs,
    buildSessionStatusPresentation,
    buildSessionSuccessHint,
    buildStartingSessionRestartBlockedMessage,
    buildTurnStatePresentation,
    parseBusySessionRestartRefusal,
    parseBusySessionClearRefusal,
    parseStartingSessionRestartRefusal,
    sessionStatusShowsIdleDirectPane,
    type SessionCommandName,
} from './sessionUi.js';
import {
    buildOverflowPopupMenuItems,
    buildPrimaryPopupMenuItems,
} from './popupMenu.js';
import {
    buildEditorSurface,
    buildSyncCommandArgs,
    flattenVisibleColumns,
    intentFromReceipt,
    isPreservedLayoutOutput,
    normalizeVisibleColumns,
    syncHintFromReceipt,
    type EditorSurface,
} from './tabSync.js';
import {
    EditorCommandCompletion,
    EditorCommandDecision,
    EditorCommandKind,
    EditorCommandRegistry,
} from './editorCommandState.js';
import {
    CrdtReplicaManager,
    type ReplicaLocalChangeAdmission,
    type ReplicaTextChange,
} from './crdtReplica.js';
import { registerReliableSyncLiveness } from './reliableSyncLiveness.js';
import { DebounceCore } from '@lazily-hub/lazily-js/rateshape';

// ---------------------------------------------------------------------------
// CLI Resolution (Feature 9)
// ---------------------------------------------------------------------------

let resolvedAgentDoc: string | null = null;
const AUTOMATIC_SYNC_CLI_TIMEOUT_MS = 5_000;
const ROUTE_CANCEL_WAIT_MS = 5_000;
const ROUTE_WAIT_FOR_READY_SECONDS = '120';
const EDITOR_ID = `vscode-${process.pid}-${crypto.randomUUID()}`;
const LAZILY_CURRENT_OBSERVATION_DELAY_MS = 75;

function monotonicMillis(): number {
    return Number(process.hrtime.bigint() / 1_000_000n);
}

interface GitRepositoryApi {
    status(): Promise<void>;
}

interface GitExtensionApi {
    getRepository(uri: vscode.Uri): GitRepositoryApi | null;
}

interface GitExtensionExports {
    getAPI(version: 1): GitExtensionApi;
}

async function refreshVcsForFile(filePath: string): Promise<void> {
    const extension = vscode.extensions.getExtension<GitExtensionExports>('vscode.git');
    if (!extension) return;
    const exports = extension.isActive ? extension.exports : await extension.activate();
    const repository = exports.getAPI(1).getRepository(vscode.Uri.file(filePath));
    await repository?.status();
}

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

interface LazilyCurrentObservationState {
    debounce: DebounceCore<string>;
    timer: ReturnType<typeof setTimeout> | undefined;
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

function isCliTimeout(err: unknown): boolean {
    return err instanceof Error && err.message.startsWith('timed out after ');
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

// Project Controller→plugin turn-state coordination: reflect the authoritative
// lazily state projection in a status-bar indicator. The editor never reads
// cycle sidecars for this hot path.
const turnStatusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
const TURN_STATUS_MIN_REFRESH_INTERVAL_MS = 1_500;
const TURN_STATUS_PROJECT_CONTROLLER_TIMEOUT_MS = 1_500;
let turnStatusWatcherRoot: string | undefined;
let turnStatusRefreshTimer: ReturnType<typeof setTimeout> | undefined;
let turnStatusLastRefreshMs = 0;
let turnStatusRefreshSeq = 0;
const turnStatusMirrors = new Map<string, InstanceType<typeof stateMirror.GraphView>>();
// Durable state-event versions successfully folded by this peer. Each value is
// reported on the NEXT subscription, so delivery without a successful apply is
// never acknowledged.
const turnStatusAppliedDocumentVersions = new Map<string, number>();
const turnStatusRecordedDocumentVersions = new Map<string, number>();

function activeAgentDocProjectRoot(): string | undefined {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !editor.document.fileName.endsWith('.md')) return undefined;
    const workspaceRoot = getWorkspaceRoot(editor.document.uri);
    if (!workspaceRoot) return undefined;
    return resolveProject(workspaceRoot, editor.document.uri.fsPath).cwd;
}

function disposeTurnStatusWatcher(): void {
    turnStatusWatcherRoot = undefined;
    if (turnStatusRefreshTimer) clearTimeout(turnStatusRefreshTimer);
    turnStatusRefreshTimer = undefined;
    turnStatusMirrors.clear();
    turnStatusAppliedDocumentVersions.clear();
    turnStatusRecordedDocumentVersions.clear();
}

function configureTurnStatusWatcher(): void {
    const root = activeAgentDocProjectRoot();
    if (root === turnStatusWatcherRoot) return;
    disposeTurnStatusWatcher();
    if (!root) return;
    turnStatusWatcherRoot = root;
}

function turnStatusRefreshDelayMs(): number {
    const now = Date.now();
    const minIntervalUntil = turnStatusLastRefreshMs + TURN_STATUS_MIN_REFRESH_INTERVAL_MS;
    return Math.max(0, minIntervalUntil - now);
}

function refreshTurnStatus(reason = 'event', force = false): void {
    if (force) {
        if (turnStatusRefreshTimer) clearTimeout(turnStatusRefreshTimer);
        turnStatusRefreshTimer = undefined;
        void refreshTurnStatusNow(reason);
        return;
    }
    if (turnStatusRefreshTimer) return;
    const delayMs = turnStatusRefreshDelayMs();
    turnStatusRefreshTimer = setTimeout(() => {
        turnStatusRefreshTimer = undefined;
        void refreshTurnStatusNow(reason);
    }, delayMs);
}

async function turnProjectionFromProjectController(
    projectRoot: string,
    filePath: string,
): Promise<import('./sessionUi.js').TurnProjection> {
    const docHash = native.documentHash(filePath);
    let mirror = turnStatusMirrors.get(docHash);
    if (!mirror) {
        mirror = new stateMirror.GraphView();
        turnStatusMirrors.set(docHash, mirror);
    }
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), TURN_STATUS_PROJECT_CONTROLLER_TIMEOUT_MS);
    const appliedVersion = turnStatusAppliedDocumentVersions.get(docHash) ?? 0;
    const recordedVersion = turnStatusRecordedDocumentVersions.get(docHash) ?? 0;
    const pendingAck = appliedVersion > recordedVersion ? appliedVersion : 0;
    try {
        const data = await requestProjectController(
            projectRoot,
            {
                command: 'state_subscribe',
                file: filePath,
                generation: mirror.isInitialized ? mirror.epoch : 0,
                diagnostic_payload: JSON.stringify({
                    document_hash: docHash,
                    peer_pid: process.pid,
                    editor_id: EDITOR_ID,
                    acked_version: pendingAck,
                }),
            },
            controller.signal,
        );
        const message = data?.message;
        if (!message || typeof message !== 'object') {
            throw new Error('Project Controller state_subscribe response missing message');
        }
        if (data?.document_hash && data.document_hash !== docHash) {
            throw new Error('Project Controller returned state for a different document');
        }
        if (data?.peer_ack_recorded === true && pendingAck > 0) {
            turnStatusRecordedDocumentVersions.set(
                docHash,
                Math.max(recordedVersion, pendingAck),
            );
        }
        if (!stateMirror.applyIpcMessageToView(mirror, JSON.stringify(message))) {
            throw new Error('Project Controller state_subscribe message did not apply');
        }
        if (typeof data?.document_version !== 'number') {
            throw new Error('Project Controller state_subscribe response missing document_version');
        }
        const previousVersion = turnStatusAppliedDocumentVersions.get(docHash) ?? 0;
        turnStatusAppliedDocumentVersions.set(
            docHash,
            Math.max(previousVersion, data.document_version),
        );
        return stateMirror.agentDocTurnProjectionFromView(mirror);
    } finally {
        clearTimeout(timer);
    }
}

async function refreshTurnStatusNow(reason: string): Promise<void> {
    const seq = ++turnStatusRefreshSeq;
    const editor = vscode.window.activeTextEditor;
    if (!editor || !editor.document.fileName.endsWith('.md')) {
        turnStatusBarItem.hide();
        return;
    }
    const workspaceRoot = getWorkspaceRoot(editor.document.uri);
    const projectRoot = workspaceRoot
        ? resolveProject(workspaceRoot, editor.document.uri.fsPath).cwd
        : undefined;
    turnStatusLastRefreshMs = Date.now();
    if (!projectRoot) {
        turnStatusBarItem.text = 'agent-doc: Project Controller disconnected';
        turnStatusBarItem.tooltip = 'Agent Doc Project Controller is not connected for this document.';
        turnStatusBarItem.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
        turnStatusBarItem.show();
        return;
    }
    let projection: import('./sessionUi.js').TurnProjection | null = null;
    let disconnected: string | null = null;
    try {
        projection = await turnProjectionFromProjectController(projectRoot, editor.document.fileName);
    } catch (err: any) {
        disconnected = err?.message ?? 'Project Controller request failed';
    }
    if (seq !== turnStatusRefreshSeq) return;
    if (disconnected) {
        turnStatusBarItem.text = 'agent-doc: Project Controller disconnected';
        turnStatusBarItem.tooltip = `Agent Doc Project Controller is not connected for this document.\n${disconnected}`;
        turnStatusBarItem.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
        turnStatusBarItem.show();
        return;
    }
    const presentation = buildTurnStatePresentation(projection);
    if (presentation.label) {
        // Prominence parity with the JetBrains editor banner: tooltip + an
        // attention background while the Project Controller turn is in flight.
        turnStatusBarItem.text = presentation.label;
        turnStatusBarItem.tooltip =
            presentation.tooltip
            ?? "Agent Doc turn state — the Project Controller's authoritative turn phase for this document";
        turnStatusBarItem.backgroundColor = new vscode.ThemeColor(
            'statusBarItem.warningBackground',
        );
        turnStatusBarItem.show();
    } else {
        turnStatusBarItem.backgroundColor = undefined;
        turnStatusBarItem.hide();
    }
}

function refreshActiveTurnStatus(): void {
    configureTurnStatusWatcher();
    refreshTurnStatus('active-editor', true);
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

function focusReceiptFocused(raw: string | null): boolean {
    if (!raw) return false;
    try {
        const parsed = JSON.parse(raw);
        const data = parsed && typeof parsed === 'object' && 'data' in parsed
            ? (parsed as any).data
            : parsed;
        return data?.focused === true;
    } catch {
        return false;
    }
}

function syncReceiptReason(raw: string | null): string {
    if (!raw) return '';
    try {
        const parsed = JSON.parse(raw);
        const data = parsed && typeof parsed === 'object' && 'data' in parsed
            ? (parsed as any).data
            : parsed;
        return typeof data?.reason === 'string' ? data.reason : '';
    } catch {
        return raw;
    }
}

function buildSyncLayoutColumns(columns: string[][]): string[] {
    return normalizeVisibleColumns(columns).map((column) => column.join(','));
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

function controllerSocketPath(projectRoot: string): string {
    return path.join(projectRoot, '.agent-doc', 'controller.sock');
}

async function ensureProjectControllerRunning(projectRoot: string, signal: AbortSignal): Promise<void> {
    await runCli(
        ['controller', 'status', '--project-root', projectRoot, '--ensure'],
        projectRoot,
        { signal, timeoutMs: 60_000 },
    );
}

function requestProjectController(
    projectRoot: string,
    request: Record<string, unknown>,
    signal: AbortSignal,
): Promise<any> {
    return new Promise((resolve, reject) => {
        if (signal.aborted) {
            reject(new CliCancelledError());
            return;
        }

        const socket = net.createConnection(controllerSocketPath(projectRoot));
        let response = '';
        let settled = false;

        const cleanup = () => {
            signal.removeEventListener('abort', abortHandler);
            socket.removeAllListeners();
            socket.destroy();
        };
        const finish = (fn: () => void) => {
            if (settled) return;
            settled = true;
            cleanup();
            fn();
        };
        const abortHandler = () => {
            socket.destroy();
            finish(() => reject(new CliCancelledError()));
        };

        signal.addEventListener('abort', abortHandler, { once: true });
        socket.setEncoding('utf8');
        socket.on('connect', () => {
            socket.write(`${JSON.stringify(request)}\n`);
        });
        socket.on('data', (chunk: string) => {
            response += chunk;
            const newline = response.indexOf('\n');
            if (newline < 0) return;
            const line = response.slice(0, newline).trim();
            finish(() => {
                try {
                    const envelope = JSON.parse(line);
                    if (envelope?.ok !== true) {
                        reject(new Error(envelope?.error || 'project controller request failed'));
                        return;
                    }
                    resolve(envelope.data);
                } catch (err: any) {
                    reject(new Error(`failed to parse project controller response: ${err.message}`));
                }
            });
        });
        socket.on('error', (err) => {
            finish(() => reject(err));
        });
        socket.on('end', () => {
            finish(() => reject(new Error('project controller closed connection without a response')));
        });
    });
}

// #lzmsgpcp: route `Run Agent Doc` through the lazily command/RPC message plane
// (`command-plane-v1`) instead of the classic `editor_route` request. Phase 7
// gate 3 — default-on; the controller keeps both endpoints in shadow mode, so
// `AGENT_DOC_COMMAND_PLANE=0` falls back to the classic path.
function commandPlaneEnabled(): boolean {
    return process.env.AGENT_DOC_COMMAND_PLANE !== '0';
}

// Route via the command plane: send a `CommandSubmit` envelope (namespace
// `agent-doc`, name `editor_route`) and resolve ONLY on a terminal `applied`
// projection. Envelope + terminal resolution live in `./commandPlane`.
async function runEditorRouteViaCommandPlane(
    cwd: string,
    rel: string,
    filePath: string,
    routeKey: string,
    layoutArgs: string[],
    signal: AbortSignal,
): Promise<string> {
    const { commandId, message } = buildEditorRouteCommandMessage(
        rel,
        routeKey,
        layoutArgs,
        Number(ROUTE_WAIT_FOR_READY_SECONDS),
    );
    const data = await requestProjectController(
        cwd,
        {
            command: 'editor_command_submit',
            file: filePath,
            diagnostic_payload: JSON.stringify(message),
        },
        signal,
    );
    return resolveEditorRouteTerminal(data, commandId);
}

async function runEditorRouteViaProjectController(
    cwd: string,
    rel: string,
    filePath: string,
    routeKey: string,
    signal: AbortSignal,
): Promise<string> {
    await ensureProjectControllerRunning(cwd, signal);
    const layoutArgs = buildRouteLayoutArgs(collectVisibleMarkdownColumns(cwd), rel);
    if (commandPlaneEnabled()) {
        return runEditorRouteViaCommandPlane(cwd, rel, filePath, routeKey, layoutArgs, signal);
    }
    const data = await requestProjectController(
        cwd,
        {
            command: 'editor_route',
            file: filePath,
            diagnostic_payload: JSON.stringify(
                buildEditorRoutePayload(rel, routeKey, layoutArgs, Number(ROUTE_WAIT_FOR_READY_SECONDS)),
            ),
        },
        signal,
    );
    const exitCode = typeof data?.exit_code === 'number' ? data.exit_code : 1;
    const output = typeof data?.output === 'string' ? data.output : '';
    if (exitCode !== 0) {
        throw new Error(output || `Project Controller editor_route failed with exit code ${exitCode}`);
    }
    return output;
}

// ---------------------------------------------------------------------------
// Feature 1: Run (Submit)
// ---------------------------------------------------------------------------

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
        const output = await runEditorRouteViaProjectController(cwd, rel, filePath, routeKey, abortController.signal);
        native.recordRouteDispatchProven(filePath, routeGeneration, `vscode:${routeKey}`, cwd);
        // #r5at: read via the lazily-js reactive mirror (snapshot/delta over the
        // FFI state backbone), falling back to the cold projection pull. The
        // just-recorded dispatch facts surface as a warm delta without a full
        // re-render — the VS Code counterpart of the JB reactiveSummaryForFile.
        const summary = native.reactiveSummaryForFile(filePath, cwd);
        if (summary) {
            console.log(
                `[agent-doc/state-projection] ${stateMirror.compactAgentDocProjection(summary)} `
                + `epoch=${native.mirrorEpochForFile(filePath) ?? '-'} file=${rel}`,
            );
        }
        showHint(output || `Routed ${rel}`);
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

// Restart Agent replaces the harness child and re-resolves current `agent:`
// frontmatter. Recycle Supervisor remains a controller-code refresh that
// preserves the child.
async function restartAgentAction(): Promise<void> {
    await runSessionCommandForActiveFile(
        'restart-agent',
        (output, rel) => {
            showHint(buildSessionSuccessHint('restart-agent', rel, output));
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

/** Prompt for a fresh authoritative-session pane or an explicit cross-session recovery. */
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
        'New Pane in This Session',
        'Force Claim',
        'Switch Project Session',
    );
    if (choice === 'New Pane in This Session') {
        await reclaimAfterCrossSession(cwd, rel, position, { newPane: true });
    } else if (choice === 'Force Claim') {
        await reclaimAfterCrossSession(cwd, rel, position, { force: true });
    } else if (choice === 'Switch Project Session') {
        await reclaimAfterCrossSession(cwd, rel, position, { switchTo: reject.paneSession });
    }
    // undefined (Esc / dismiss) => Cancel, leave the file unclaimed.
}

/** Provision, force-claim, or switch the configured session after a cross-session reject. */
async function reclaimAfterCrossSession(
    cwd: string,
    rel: string,
    position: string | undefined,
    opts: { force?: boolean; newPane?: boolean; switchTo?: string },
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
        const args = buildCrossSessionClaimArgs(rel, position, opts);
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
        await syncLayoutInternal(cwd, true, false);
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
        const effectiveFocusFile = focusFile ?? flattenVisibleColumns(visibleColumns)[0];
        const receipt = native.syncTmuxLayoutJson({
            projectRoot: root,
            columns: buildSyncLayoutColumns(visibleColumns),
            focus: effectiveFocusFile,
            noAutostart,
            exactVisible: false,
        });
        if (!receipt) {
            throw new Error('project controller sync failed');
        }
        if (notify) {
            const reason = syncReceiptReason(receipt);
            if (isPreservedLayoutOutput(reason)) {
                void vscode.window.showWarningMessage(
                    'Sync deferred: the current tmux layout was preserved because one or more requested files are still blocked.',
                );
            } else {
                showHint(formatSyncLayoutSummary(visibleColumns, effectiveFocusFile));
            }
        }
    } catch (err: any) {
        if (notify) showError(`sync failed: ${err.message}`);
    }
}

// ---------------------------------------------------------------------------
// Feature 4: Editor Surface Reporting (Automatic)
// ---------------------------------------------------------------------------
//
// `#jbsurfaceswap`: the extension reports one observation per tab/visibility
// change and the reactive graph behind `agent_doc_editor_surface_observe_json`
// derives focus-vs-sync, dedups repeats, and drives the Project Controller
// command. What is left here is event-storm handling that is genuinely the
// editor's: a debounce plus a generation guard so a burst reports only its
// final state.

let surfaceDebounceTimer: ReturnType<typeof setTimeout> | undefined;
let surfaceReportRunning = false;
const SURFACE_DEBOUNCE_MS = 100;
let latestSurfaceGeneration = 0;
/** Every project root observed this session, released on deactivate. */
const observedSurfaceRoots = new Set<string>();

interface PendingSurfaceObservation {
    root: string;
    relativePath: string;
    surface: EditorSurface;
}

/**
 * Absolute paths, matching the JetBrains plugin and the controller's document
 * identity. `collectVisibleMarkdownColumns` reports workspace-relative paths
 * because the CLI took them that way; the surface graph focuses a document by
 * path, so it needs the real one.
 */
function absolutizeColumns(root: string, columns: string[][]): string[][] {
    return columns.map((column) => column.map((file) => path.join(root, file)));
}

function captureCurrentSurface(forceReconcile: boolean): PendingSurfaceObservation | null {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return null;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) return null;

    const activeFsPath = editor.document.uri.fsPath;
    const visibleColumns = absolutizeColumns(root, collectVisibleMarkdownColumns(root));
    const surface = buildEditorSurface({
        activeFile: activeFsPath,
        visibleMd: flattenVisibleColumns(visibleColumns),
        visibleColumns,
        forceReconcile,
    });
    if (surface === null) return null;
    return { root, relativePath: relativePath(root, activeFsPath), surface };
}

function requestSurfaceObservation(delayMs = SURFACE_DEBOUNCE_MS): number {
    const requestedGeneration = ++latestSurfaceGeneration;
    if (surfaceDebounceTimer) clearTimeout(surfaceDebounceTimer);
    surfaceDebounceTimer = setTimeout(() => {
        surfaceDebounceTimer = undefined;
        reportCurrentSurface(requestedGeneration);
    }, delayMs);
    return requestedGeneration;
}

function reportCurrentSurface(requestedGeneration: number): void {
    if (requestedGeneration !== latestSurfaceGeneration) return;
    if (surfaceReportRunning) return;
    surfaceReportRunning = true;
    try {
        const pending = captureCurrentSurface(false);
        if (pending === null) return;
        observedSurfaceRoots.add(pending.root);
        const receipt = native.editorSurfaceObserveJson({
            projectRoot: pending.root,
            surfaceJson: JSON.stringify(pending.surface),
        });
        if (receipt === null) return;
        const intent = intentFromReceipt(receipt);
        if (intent && intent.kind !== 'idle') {
            const hint = syncHintFromReceipt(receipt);
            if (hint) showHint(hint);
        }
    } finally {
        surfaceReportRunning = false;
        // A change that arrived while we were reporting has already bumped the
        // generation; report the newer state rather than dropping it.
        if (latestSurfaceGeneration !== requestedGeneration && !surfaceDebounceTimer) {
            requestSurfaceObservation(0);
        }
    }
}

/** Release every observed root's surface graph. */
function forgetObservedSurfaces(): void {
    for (const root of observedSurfaceRoots) {
        native.editorSurfaceForget(root);
    }
    observedSurfaceRoots.clear();
}

function onTabChanged(): void {
    requestSurfaceObservation();
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
// Lazily editor endpoint
// ---------------------------------------------------------------------------

/**
 * Hosts the PID-scoped endpoint for the registered Lazily replica. Document
 * mutations arrive as typed messages and publish typed receipts; the filesystem
 * is never used as a live-buffer or patch transport.
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
    private socketServer: net.Server | undefined;
    private socketPath: string | undefined;
    private typingListener: vscode.Disposable | undefined;
    private openListener: vscode.Disposable | undefined;
    private saveListener: vscode.Disposable | undefined;
    private closeListener: vscode.Disposable | undefined;
    private crdtReplicas: CrdtReplicaManager | undefined;
    private projectRootPath: string | undefined;
    private outputChannel: vscode.OutputChannel;
    /** Track last typing time per file for debounce */
    private lastTypingTime = new Map<string, number>();
    private disposed = false;
    /**
     * #falsetyping-guard: paths with an unsaved *local operator* edit ahead of
     * disk. Set when a non-remoteCrdtApply text change lands; cleared when the
     * document is saved or closed. Lets the live-buffer report tell the CLI that a
     * buffer divergence is replica-driven (a remoteCrdtApply) rather than operator
     * text, so the visible-write guard re-merges on replica churn instead of
     * failing closed. Absent from the set = no unsaved operator edits.
     */
    private unsyncedLocalEditDocs = new Set<string>();
    /** Lazily KeepLatest debounce state; full-buffer reads stay off the change listener. */
    private lazilyCurrentObservations = new Map<string, LazilyCurrentObservationState>();
    /** Clean external-reload reconciliation is also deferred off the UI listener. */
    private deferredReconnectTimers = new Map<string, ReturnType<typeof setTimeout>>();
    /** CRDT local forwards are queued off the text-change listener path. */
    private crdtLocalChangeTimers = new Map<string, Set<ReturnType<typeof setTimeout>>>();
    /** Native editor-op writes are queued off the text-change listener path. */
    private pendingEditorOpReports: PendingEditorOpReport[] = [];
    private editorOpReportTimer: ReturnType<typeof setTimeout> | undefined;
    constructor() {
        this.outputChannel = vscode.window.createOutputChannel('Agent Doc Patches');
    }

    start(): void {
        this.disposed = false;
        const projectRoot = this.findProjectRoot();
        if (!projectRoot) {
            this.outputChannel.appendLine('PatchWatcher: no agent-doc project root found');
            return;
        }

        this.projectRootPath = projectRoot;
        this.startSocketListener(projectRoot);
        this.crdtReplicas = new CrdtReplicaManager({
            projectRoot,
            identity: EDITOR_ID,
            listDocuments: () => this.currentProjectMarkdownSnapshots(projectRoot),
            currentText: (filePath) => this.currentOpenDocumentText(filePath),
            applyText: (filePath, text, expectedText) => this.applyCrdtReplicaText(filePath, text, expectedText),
            resolveDeferredReconnectContent: (filePath, editorText) =>
                native.deferredWriteReconnectContent(filePath, editorText, projectRoot),
            settleDeferredReconnectContent: (filePath, editorText) => {
                native.deferredWriteReconnectPropagated(filePath, editorText, projectRoot);
            },
            normalizeTemplateStructure: (text) => native.normalizeTemplateStructure(text, projectRoot),
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
            this.scheduleLazilyCurrentObservation(document, projectRoot);
        });

        // Track typing events for debounce (TS fallback + FFI)
        this.typingListener = vscode.workspace.onDidChangeTextDocument((e) => {
            if (e.document.languageId === 'markdown' && e.contentChanges.length > 0) {
                const fsPath = e.document.uri.fsPath;
                const remoteCrdtApply = this.crdtReplicas?.isApplyingRemote(fsPath) ?? false;
                // A genuine user edit makes the document dirty (or carries an
                // explicit undo/redo reason). A clean whole-buffer/cache reload
                // is a visibility event, not an editor-origin mutation.
                const operatorEdit = !remoteCrdtApply && (e.document.isDirty || e.reason !== undefined);
                const admission = this.crdtReplicas?.captureLocalChange(fsPath, operatorEdit);
                if (operatorEdit) {
                    this.lastTypingTime.set(fsPath, Date.now());
                    // #falsetyping-guard: a genuine local operator edit is now
                    // ahead of disk until saved. A remoteCrdtApply is replica
                    // churn, not operator text, and must NOT set this.
                    this.unsyncedLocalEditDocs.add(fsPath);
                }
                const eventProjectRoot = this.projectRootPath;
                this.scheduleLazilyCurrentObservation(e.document, eventProjectRoot);
            if (!operatorEdit && !remoteCrdtApply) {
                // A clean cache reload may be the operator accepting a pending
                // external-disk candidate. The shared resolver returns content
                // only with exact Lazily lineage; attachDocument then resets the
                // replica from the visible buffer before it can publish.
                this.scheduleDeferredReconnectRefresh(e.document);
            }
                const changes: ReplicaTextChange[] = e.contentChanges.map((change) => ({
                    rangeOffset: change.rangeOffset,
                    rangeLength: change.rangeLength,
                    text: change.text,
                }));
                this.scheduleCrdtLocalChangeDelta(fsPath, changes, admission);
                // #qnodemerge4wire Phase 4: report the real editor op so a concurrent
                // agent merge aligns to the user's actual edit boundaries.
                if (operatorEdit) {
                    this.scheduleEditorOpReport(fsPath, e.contentChanges, eventProjectRoot);
                }
            }
        });

        // #falsetyping-guard: a saved or closed document has no unsaved operator
        // edits ahead of disk to protect, so drop its local-edit marker. This lets
        // replica churn after a save/submit reconcile instead of staying wedged.
        this.saveListener = vscode.workspace.onDidSaveTextDocument((document) => {
            this.unsyncedLocalEditDocs.delete(document.uri.fsPath);
        });
        this.closeListener = vscode.workspace.onDidCloseTextDocument((document) => {
            this.unsyncedLocalEditDocs.delete(document.uri.fsPath);
        });

        this.outputChannel.appendLine(`PatchWatcher: Lazily endpoint active for ${projectRoot}`);

    }

    private findProjectRoot(): string | undefined {
        // Walk up from workspace root to find the agent-doc project root.
        const roots = vscode.workspace.workspaceFolders;
        if (!roots || roots.length === 0) return undefined;

        let dir = roots[0].uri.fsPath;
        const root = path.parse(dir).root;

        while (dir !== root) {
            if (fs.existsSync(path.join(dir, '.agent-doc'))) {
                return dir;
            }
            dir = path.dirname(dir);
        }

        return roots[0].uri.fsPath;
    }

    private startSocketListener(projectRoot: string): void {
        if (this.socketServer) return;
        const socketPath = path.join(projectRoot, '.agent-doc', `ipc-${process.pid}.sock`);
        fs.mkdirSync(path.dirname(socketPath), { recursive: true });
        try { fs.unlinkSync(socketPath); } catch { /* no stale endpoint */ }

        this.socketPath = socketPath;
        this.socketServer = net.createServer((socket) => {
            let buffered = '';
            let handled = false;
            socket.setEncoding('utf8');
            socket.on('data', (chunk: string) => {
                if (handled) return;
                buffered += chunk;
                const newline = buffered.indexOf('\n');
                if (newline < 0) return;
                handled = true;
                socket.pause();
                let message: Record<string, unknown>;
                try {
                    message = JSON.parse(buffered.slice(0, newline));
                } catch {
                    socket.end('{"type":"receipt","status":"rejected","reason":"invalid_json"}\n');
                    return;
                }
                if (message.early_receipt === true) {
                    socket.write('{"type":"receipt","status":"accepted"}\n');
                }
                void this.handleSocketMessage(message, projectRoot).then(
                    (outcome) => {
                        const reason = outcome === 2 ? ',"reason":"already_applied"' : '';
                        const status = outcome === 0 ? 'rejected' : 'applied';
                        socket.end(`{"type":"receipt","status":"${status}"${reason}}\n`);
                    },
                    (error: any) => {
                        this.outputChannel.appendLine(`[socket] apply failed: ${error?.message ?? error}`);
                        socket.end('{"type":"receipt","status":"rejected","reason":"apply_failed"}\n');
                    },
                );
            });
        });
        this.socketServer.on('error', (error) => {
            this.outputChannel.appendLine(`[socket] listener error: ${error.message}`);
        });
        this.socketServer.listen(socketPath);
    }

    private targetsSocketMessage(message: Record<string, unknown>): boolean {
        return message.editor_id === EDITOR_ID
            && (message.editor_pid === undefined || message.editor_pid === process.pid);
    }

    private async handleSocketMessage(
        message: Record<string, unknown>,
        projectRoot: string,
    ): Promise<number> {
        const type = typeof message.type === 'string' ? message.type : '';
        if (!this.targetsSocketMessage(message)) {
            return 0;
        }
        const filePath = typeof message.file === 'string' ? message.file : undefined;
        switch (type) {
            // `ApplyStructuralOp` (CRDT structural ops, `#crdtstructops` Phase C)
            // rides the same node-patch apply path as `ApplyCanonical`: the binary
            // sends only `node_patches` (strike/mark_done) with no canonical content.
            case EditorIntent.ApplyCanonical:
            case EditorIntent.ApplyStructuralOp: {
                if (!filePath || (typeof message.fullContent === 'string' && message.fullContent.length > 0)) return 0;
                const patch = {
                    ...message,
                    file: filePath,
                    patches: Array.isArray(message.patches) ? message.patches : [],
                    node_patches: Array.isArray(message.node_patches) ? message.node_patches : [],
                    unmatched: typeof message.unmatched === 'string' ? message.unmatched : '',
                } as unknown as IpcPatch;
                const generation = native.recordEditorPatchQueued(filePath, patch.patch_id, projectRoot);
                const before = this.currentOpenDocumentText(filePath);
                const applied = await this.applyPatch(patch);
                if (!applied) {
                    native.recordEditorPatchRejected(filePath, patch.patch_id, generation, 'socket_apply_failed', projectRoot);
                    native.recordEditorRetryRequested(filePath, patch.patch_id, generation, 'socket_apply_failed', projectRoot);
                    return 0;
                }
                native.recordEditorPatchApplied(filePath, patch.patch_id, generation, projectRoot);
                return before === this.currentOpenDocumentText(filePath) ? 2 : 1;
            }
            case EditorIntent.Reposition:
                return filePath && await this.repositionBoundaryFromSocket(
                    filePath,
                    typeof message.boundary_id === 'string' ? message.boundary_id : undefined,
                    message.preserve_head === true,
                    projectRoot,
                ) ? 1 : 0;
            case EditorIntent.RefreshContent:
                return filePath && typeof message.content === 'string'
                    && await this.refreshContentFromSocket(
                        filePath,
                        message.content,
                        typeof message.expected_content_hash === 'string' ? message.expected_content_hash : undefined,
                        typeof message.expected_content_len === 'number' ? message.expected_content_len : undefined,
                        projectRoot,
                    ) ? 1 : 0;
            case EditorIntent.ObserveLazilyCurrent: {
                const document = filePath
                    ? vscode.workspace.textDocuments.find((candidate) => candidate.uri.fsPath === filePath)
                    : undefined;
                if (!document) return 0;
                this.observeLazilyCurrentNow(document, projectRoot);
                return 1;
            }
            case EditorIntent.DeliverCrdtRemote:
                if (!filePath) return 0;
                if (message.reason === 'request_full_state' || message.reason === 'ack_recovery_force_refresh') {
                    await this.crdtReplicas?.handleReattachRequest(
                        filePath,
                        this.unsyncedLocalEditDocs.has(filePath),
                    );
                }
                this.crdtReplicas?.requestRemoteDrain(filePath);
                return 1;
            case EditorIntent.SaveDocument: {
                const patchId = typeof message.patch_id === 'string' ? message.patch_id : undefined;
                return processSaveDocumentIntent(filePath, {
                    fileExists: (candidate) => fs.existsSync(candidate),
                    findOpenDocument: (candidate) => vscode.workspace.textDocuments.find(
                        (document) => document.uri.fsPath === candidate,
                    ),
                    publishSavedContent: (candidate, content) => this.writeEditorContentProjection(
                        patchId,
                        candidate,
                        content,
                        projectRoot,
                    ),
                    observeSavedContent: (document) => this.observeLazilyCurrentNow(
                        document as vscode.TextDocument,
                        projectRoot,
                    ),
                    recordOutcome: (candidate, status) => {
                        native.recordEditorSurfaceEvent(
                            projectRoot,
                            EDITOR_ID,
                            candidate,
                            'vcs_refresh_save',
                            EditorIntent.SaveDocument,
                            EditorIntent.SaveDocument,
                            patchId,
                            status,
                        );
                    },
                    reportFailure: (candidate, error) => this.outputChannel.appendLine(
                        `PatchWatcher: save_document failed for ${candidate}: ${error instanceof Error ? error.message : String(error)}`,
                    ),
                });
            }
            case EditorIntent.RefreshVcs:
                if (filePath) await refreshVcsForFile(filePath);
                return 1;
            case EditorIntent.ReloadLibrary:
                native.forceReloadLib(projectRoot);
                for (const snapshot of this.currentProjectMarkdownSnapshots(projectRoot)) {
                    await this.crdtReplicas?.attachDocument(snapshot.filePath, snapshot.text, true);
                }
                return 1;
            default:
                return 0;
        }
    }

    private async repositionBoundaryFromSocket(
        filePath: string,
        boundaryId: string | undefined,
        preserveHead: boolean,
        projectRoot: string,
    ): Promise<boolean> {
        const document = await vscode.workspace.openTextDocument(vscode.Uri.file(filePath));
        const content = document.getText();
        const target = preserveHead
            ? (native.repositionBoundaryToEndPreserveHead(content, projectRoot, boundaryId)
                ?? this.repositionBoundaryToEndPreserveHeadTs(content, 'exchange', boundaryId))
            : (native.repositionBoundaryToEnd(content, projectRoot, boundaryId)
                ?? this.repositionBoundaryToEndTs(content, 'exchange', boundaryId));
        return target == null || target === content || this.applyMinimalTextEdit(document, target);
    }

    private async refreshContentFromSocket(
        filePath: string,
        content: string,
        expectedHash: string | undefined,
        expectedLen: number | undefined,
        projectRoot: string,
    ): Promise<boolean> {
        const document = await vscode.workspace.openTextDocument(vscode.Uri.file(filePath));
        const current = document.getText();
        if (expectedLen !== undefined && current.length !== expectedLen) return false;
        if (expectedHash !== undefined
            && crypto.createHash('sha256').update(current, 'utf8').digest('hex') !== expectedHash) return false;
        const normalized = native.normalizeTemplateStructure(content, projectRoot);
        return normalized === content && this.applyMinimalTextEdit(document, content);
    }

    private writeEditorContentProjection(
        patchId: string | undefined,
        filePath: string,
        content: string,
        projectRoot: string,
    ): boolean {
        if (!patchId) {
            return true;
        }
        if (!native.recordEditorContentApplied(projectRoot, patchId, filePath, content, EDITOR_ID)) {
            this.outputChannel.appendLine(`PatchWatcher: lazily content receipt failed for ${patchId}`);
            return false;
        }
        return true;
    }

    private scheduleCrdtLocalChangeDelta(
        fsPath: string,
        changes: readonly ReplicaTextChange[],
        admission?: ReplicaLocalChangeAdmission,
    ): void {
        const timer = setTimeout(() => {
            const timers = this.crdtLocalChangeTimers.get(fsPath);
            timers?.delete(timer);
            if (timers?.size === 0) this.crdtLocalChangeTimers.delete(fsPath);
            const crdtForward = this.crdtReplicas?.handleLocalChangeDelta(fsPath, changes, admission);
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

    private projectRoot(): string | undefined {
        return this.projectRootPath;
    }

    private repositionBoundaryToEndTs(doc: string, component: string, boundaryId?: string): string | null {
        return repositionBoundaryToEnd(doc, component, boundaryId);
    }

    private repositionBoundaryToEndPreserveHeadTs(doc: string, component: string, boundaryId?: string): string | null {
        return repositionBoundaryToEndPreserveHead(doc, component, boundaryId);
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
        const projectRoot = this.projectRoot();

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

        if (!projectRoot) {
            this.outputChannel.appendLine(`PatchWatcher: no project root for content projection ${patch.file}`);
            return false;
        }
        return this.writeEditorContentProjection(patch.patch_id, patch.file, document.getText(), projectRoot);
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
        const projectRoot = this.projectRootPath;
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

    private currentOpenDocumentText(filePath: string): string | null {
        return vscode.workspace.textDocuments.find((doc) => doc.uri.fsPath === filePath)?.getText() ?? null;
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

    private scheduleDeferredReconnectRefresh(document: vscode.TextDocument): void {
        const fsPath = document.uri.fsPath;
        const prior = this.deferredReconnectTimers.get(fsPath);
        if (prior) clearTimeout(prior);
        const timer = setTimeout(() => {
            this.deferredReconnectTimers.delete(fsPath);
            const latest = vscode.workspace.textDocuments.find((doc) => doc.uri.fsPath === fsPath);
            if (!latest || latest.languageId !== 'markdown' || latest.uri.scheme !== 'file') return;
            void this.crdtReplicas?.attachDocument(fsPath, latest.getText(), true);
        }, 0);
        this.deferredReconnectTimers.set(fsPath, timer);
    }

    private scheduleLazilyCurrentObservation(document: vscode.TextDocument, projectRoot: string | undefined): void {
        const fsPath = document.uri.fsPath;
        const state = this.lazilyCurrentObservations.get(fsPath) ?? {
            debounce: new DebounceCore<string>(LAZILY_CURRENT_OBSERVATION_DELAY_MS),
            timer: undefined,
        };
        this.lazilyCurrentObservations.set(fsPath, state);
        state.debounce.input(monotonicMillis(), fsPath);
        if (state.timer) clearTimeout(state.timer);
        state.timer = setTimeout(
            () => this.drainLiveBufferReport(fsPath, state, projectRoot),
            LAZILY_CURRENT_OBSERVATION_DELAY_MS,
        );
    }

    private drainLiveBufferReport(
        fsPath: string,
        state: LazilyCurrentObservationState,
        projectRoot: string | undefined,
    ): void {
        if (this.lazilyCurrentObservations.get(fsPath) !== state) return;
        const emittedPath = state.debounce.tick(monotonicMillis());
        if (emittedPath === null) {
            // A timer may wake just before the monotone quiet boundary. Preserve
            // one driver for the current generation instead of dropping work.
            state.timer = setTimeout(
                () => this.drainLiveBufferReport(fsPath, state, projectRoot),
                LAZILY_CURRENT_OBSERVATION_DELAY_MS,
            );
            return;
        }
        state.timer = undefined;
        this.lazilyCurrentObservations.delete(fsPath);
        const latest = vscode.workspace.textDocuments.find((doc) => doc.uri.fsPath === emittedPath);
        if (!latest || latest.languageId !== 'markdown' || latest.uri.scheme !== 'file') return;
        this.observeLazilyCurrentNow(latest, projectRoot);
    }

    private observeLazilyCurrentNow(document: vscode.TextDocument, projectRoot: string | undefined): void {
        const fsPath = document.uri.fsPath;
        const state = this.lazilyCurrentObservations.get(fsPath);
        if (state) {
            if (state.timer) clearTimeout(state.timer);
            this.lazilyCurrentObservations.delete(fsPath);
        }
        const text = document.getText();
        seedEditorOpShadow(fsPath, text);
        // #falsetyping-guard: a clean (fully saved) document has no unsaved edits
        // at all, so clear any stale local-edit marker. Otherwise the buffer is
        // dirty: the edits are operator text only if a local (non-remoteCrdtApply)
        // change is still pending since the last save.
        if (!document.isDirty) {
            this.unsyncedLocalEditDocs.delete(fsPath);
        }
        const noUnsavedOperatorEdits = !document.isDirty || !this.unsyncedLocalEditDocs.has(fsPath);
        native.lazilyCurrentObserved(fsPath, text, projectRoot, EDITOR_ID, noUnsavedOperatorEdits);
        void this.crdtReplicas?.attachDocument(fsPath, text, true);
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
        const state = this.lazilyCurrentObservations.get(filePath);
        if (state?.timer) clearTimeout(state.timer);
        this.lazilyCurrentObservations.delete(filePath);
        const reconnectTimer = this.deferredReconnectTimers.get(filePath);
        if (reconnectTimer) clearTimeout(reconnectTimer);
        this.deferredReconnectTimers.delete(filePath);
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
        this.disposed = true;
        this.socketServer?.close();
        this.socketServer = undefined;
        if (this.socketPath) {
            try { fs.unlinkSync(this.socketPath); } catch { /* already closed */ }
            this.socketPath = undefined;
        }
        this.typingListener?.dispose();
        this.openListener?.dispose();
        this.saveListener?.dispose();
        this.closeListener?.dispose();
        for (const state of this.lazilyCurrentObservations.values()) {
            if (state.timer) clearTimeout(state.timer);
        }
        this.lazilyCurrentObservations.clear();
        for (const timer of this.deferredReconnectTimers.values()) {
            clearTimeout(timer);
        }
        this.deferredReconnectTimers.clear();
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

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    // #lzsync 3B clean split: the generic lazily GraphView is a plain statically
    // imported class (esbuild inlines it; tsc require()s it), so per-document views
    // construct synchronously — no async constructor preload is needed.

    // Sidecar-retirement Phase 3C (design B): report this editor's open-set to the
    // reliable-sync liveness plane via a lazily-js OrSet graph → FFI push. No-op
    // unless the controller dual-run flag is on.
    registerReliableSyncLiveness(context, EDITOR_ID);

    // Coordinate Project Controller turn state into the status bar. Refresh on
    // active editor changes and editor/plugin events; state itself comes from
    // the Project Controller lazily projection, never from sidecar files.
    context.subscriptions.push(
        turnStatusBarItem,
        vscode.window.onDidChangeActiveTextEditor(() => refreshActiveTurnStatus()),
        { dispose: () => disposeTurnStatusWatcher() },
    );
    refreshActiveTurnStatus();

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
    // Clean up the editor-surface debounce
    if (surfaceDebounceTimer) {
        clearTimeout(surfaceDebounceTimer);
        surfaceDebounceTimer = undefined;
    }
    // Release each observed root's surface graph: its reconciled-layout history
    // must not outlive the editor that produced it.
    forgetObservedSurfaces();

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
    latestSurfaceGeneration = 0;
    resolvedAgentDoc = null;
    commandRunning = false;
    editorCommandRegistry.resetForTest();
    for (const route of activeRoutes.values()) {
        route.controller.abort();
    }
    activeRoutes.clear();
}
