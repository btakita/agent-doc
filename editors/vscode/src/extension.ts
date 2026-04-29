import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import * as fs from 'fs';
import { execFile } from 'child_process';
import * as native from './native';
import { consumeClaimedPatch, isPatchAlreadyApplied } from './patchGuard';
import { annotateExchangeHeadingsAgainstBaseline, repositionBoundaryToEnd, repositionBoundaryToEndPreserveHead } from './reposition';

// ---------------------------------------------------------------------------
// CLI Resolution (Feature 9)
// ---------------------------------------------------------------------------

let resolvedAgentDoc: string | null = null;

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

function isMarkdownUri(uri: vscode.Uri): boolean {
    return uri.fsPath.endsWith('.md');
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

/** Run an agent-doc CLI command. Returns stdout on success. */
function runCli(args: string[], cwd: string): Promise<string> {
    const bin = resolveAgentDoc();
    return new Promise((resolve, reject) => {
        execFile(bin, args, { cwd, maxBuffer: 1024 * 1024 }, (err, stdout, stderr) => {
            if (err) {
                reject(new Error(stderr?.trim() || err.message));
            } else {
                resolve(stdout.trim());
            }
        });
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
    });

    constructor() {
        this.disposables.push(
            this.componentDecoration,
            this.patchDecoration,
            this.boundaryDecoration,
            this.scratchDecoration,
            this.promptDecoration,
            this.responseHeadingDecoration,
            this.trackedIdDecoration,
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
            component: [] as vscode.Range[],
            patch: [] as vscode.Range[],
            boundary: [] as vscode.Range[],
            scratch: [] as vscode.Range[],
            prompt: [] as vscode.Range[],
            responseHeading: [] as vscode.Range[],
            trackedId: [] as vscode.Range[],
        };

        for (const token of tokens) {
            const range = new vscode.Range(
                editor.document.positionAt(token.start),
                editor.document.positionAt(token.end),
            );
            switch (token.kind) {
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
                case 'prompt':
                    ranges.prompt.push(range);
                    break;
                case 'response_heading':
                    ranges.responseHeading.push(range);
                    break;
                case 'tracked_id':
                    ranges.trackedId.push(range);
                    break;
            }
        }

        editor.setDecorations(this.componentDecoration, ranges.component);
        editor.setDecorations(this.patchDecoration, ranges.patch);
        editor.setDecorations(this.boundaryDecoration, ranges.boundary);
        editor.setDecorations(this.scratchDecoration, ranges.scratch);
        editor.setDecorations(this.promptDecoration, ranges.prompt);
        editor.setDecorations(this.responseHeadingDecoration, ranges.responseHeading);
        editor.setDecorations(this.trackedIdDecoration, ranges.trackedId);
    }

    private clearEditor(editor: vscode.TextEditor): void {
        editor.setDecorations(this.componentDecoration, []);
        editor.setDecorations(this.patchDecoration, []);
        editor.setDecorations(this.boundaryDecoration, []);
        editor.setDecorations(this.scratchDecoration, []);
        editor.setDecorations(this.promptDecoration, []);
        editor.setDecorations(this.responseHeadingDecoration, []);
        editor.setDecorations(this.trackedIdDecoration, []);
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

function collectVisibleMdFiles(root: string): string[] {
    const files: string[] = [];
    for (const group of vscode.window.tabGroups.all) {
        const activeTab = group.activeTab;
        if (activeTab?.input instanceof vscode.TabInputText) {
            const uri = activeTab.input.uri;
            if (isMarkdownUri(uri) && uri.fsPath.startsWith(root)) {
                const rel = relativePath(root, uri.fsPath);
                if (!files.includes(rel)) files.push(rel);
            }
        }
    }
    return files;
}

// ---------------------------------------------------------------------------
// Feature 1: Run (Submit)
// ---------------------------------------------------------------------------

const trackedFiles = new Set<string>();

async function submitAction(): Promise<void> {
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
        await editor.document.save();
        const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
        const output = await runCli(['route', rel], cwd);
        showHint(output || `Routed ${rel}`);
        // Track file for prompt polling
        trackedFiles.add(editor.document.uri.fsPath);
        ensurePromptPolling(cwd);
    } catch (err: any) {
        showError(`route failed: ${err.message}`);
    } finally {
        commandRunning = false;
    }
}

// ---------------------------------------------------------------------------
// Feature 2: Claim
// ---------------------------------------------------------------------------

async function claimAction(): Promise<void> {
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
        const { cwd, relativePath: rel } = resolveProject(root, editor.document.uri.fsPath);
        const split = detectSplit(editor);
        const args = ['claim', rel];
        if (split.position) {
            args.push('--position', split.position);
        }

        const output = await runCli(args, cwd);
        showHint(output || `Claimed ${rel} (pos=${split.position || 'none'})`);

        // Trigger silent layout sync after claiming
        await syncLayoutInternal(cwd, false);
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
        await syncLayoutInternal(root, true);
    } finally {
        commandRunning = false;
    }
}

async function syncLayoutInternal(root: string, notify: boolean): Promise<void> {
    const visibleMd = collectVisibleMdFiles(root);
    if (visibleMd.length === 0) {
        if (notify) showHint('No .md files open');
        return;
    }

    // Determine focused file
    const activeEditor = vscode.window.activeTextEditor;
    let focusFile: string | undefined;
    if (activeEditor && isMarkdown(activeEditor)) {
        const activeRoot = getWorkspaceRoot(activeEditor.document.uri);
        if (activeRoot === root) {
            focusFile = relativePath(root, activeEditor.document.uri.fsPath);
        }
    }

    try {
        // Always use sync --col format for consistency with JetBrains plugin.
        // Group all visible files into a single column (VS Code doesn't easily
        // expose multi-column layout structure via API).
        const colArg = visibleMd.join(',');
        const args = ['sync', '--col', colArg];
        if (focusFile) {
            args.push('--focus', focusFile);
        }

        const output = await runCli(args, root);
        if (notify) {
            showHint(`Sync: --col ${colArg}${focusFile ? ` [focus: ${focusFile}]` : ''}`);
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
let lastTabSyncSignature = '';

function onTabChanged(): void {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isMarkdown(editor)) return;

    const root = getWorkspaceRoot(editor.document.uri);
    if (!root) return;

    // Build a signature of the current visible md file set + active file
    const visibleMd = collectVisibleMdFiles(root);
    const activeFile = relativePath(root, editor.document.uri.fsPath);
    const signature = `${activeFile}|${visibleMd.sort().join(',')}`;
    if (signature === lastTabSyncSignature) return;

    // Debounce: 500ms
    if (tabSyncDebounceTimer) clearTimeout(tabSyncDebounceTimer);
    tabSyncDebounceTimer = setTimeout(async () => {
        if (tabSyncRunning) return; // concurrency guard
        tabSyncRunning = true;

        try {
            const colArg = visibleMd.join(',');
            const args = ['sync', '--col', colArg, '--focus', activeFile];
            await runCli(args, root);
            lastTabSyncSignature = signature;
        } catch {
            // Silently ignore tab sync errors
        } finally {
            tabSyncRunning = false;
        }
    }, 500);
}

// ---------------------------------------------------------------------------
// Feature 5: Prompt Polling
// ---------------------------------------------------------------------------

interface PromptOption {
    index: number;
    label: string;
}

interface PromptInfo {
    active: boolean;
    question?: string;
    options?: PromptOption[];
    selected?: number;
}

interface PromptAllEntry {
    session_id: string;
    file: string;
    info: PromptInfo;
}

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
    // Auto-save tracked files before polling
    for (const fsPath of trackedFiles) {
        const doc = vscode.workspace.textDocuments.find(d => d.uri.fsPath === fsPath);
        if (doc && doc.isDirty) {
            try { await doc.save(); } catch { /* best effort */ }
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

    // Normalize entries to have an info field
    const normalized: Array<{ file: string; key: string; info: PromptInfo }> = [];
    for (const entry of entries) {
        // The CLI may return the info fields at the top level or nested
        const info: PromptInfo = entry.info ?? {
            active: (entry as any).active ?? false,
            question: (entry as any).question,
            options: (entry as any).options,
            selected: (entry as any).selected,
        };
        if (!info.active || !info.options || info.options.length === 0) continue;
        const key = `${entry.file}:${info.question}`;
        normalized.push({ file: entry.file, key, info });
    }

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
    const items = options.map(opt => ({
        label: `[${opt.index}] ${opt.label}`,
        index: opt.index,
    }));

    const selected = await vscode.window.showQuickPick(items, {
        title: 'Agent Doc Prompt',
        placeHolder: question,
    });

    if (selected) {
        answeredPromptKey = currentPromptKey;
        currentPromptKey = undefined;
        try {
            await runCli(['prompt', '--answer', selected.index.toString(), next.file], root);
        } catch (err: any) {
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

    const items = [
        { label: '$(play) Run (Submit)', id: 'submit' },
        { label: '$(link) Claim', id: 'claim' },
        { label: '$(layout) Sync Layout', id: 'syncLayout' },
    ];

    const selected = await vscode.window.showQuickPick(items, {
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
        case 'syncLayout':
            await syncLayoutAction();
            break;
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
 * 4. Saves the document and deletes the JSON file (ACK)
 * 5. agent-doc polls for deletion and updates the snapshot
 */

interface IpcComponentPatch {
    component: string;
    content: string;
}

interface IpcPatch {
    file: string;
    patches: IpcComponentPatch[];
    unmatched: string;
    frontmatter?: string;
    fullContent?: string;
    reposition_boundary?: boolean;
    reposition_boundary_id?: string;
    preserve_head?: boolean;
    normalize_prefix_lines?: string[];
    patch_id?: string;
}

class PatchWatcher implements vscode.Disposable {
    private watcher: vscode.FileSystemWatcher | undefined;
    private signalWatcher: vscode.FileSystemWatcher | undefined;
    private typingListener: vscode.Disposable | undefined;
    private patchesDir: string | undefined;
    private outputChannel: vscode.OutputChannel;
    /** Track last typing time per file for debounce */
    private lastTypingTime = new Map<string, number>();

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

        // Track typing events for debounce (TS fallback + FFI)
        this.typingListener = vscode.workspace.onDidChangeTextDocument((e) => {
            if (e.document.languageId === 'markdown' && e.contentChanges.length > 0) {
                const fsPath = e.document.uri.fsPath;
                this.lastTypingTime.set(fsPath, Date.now());
                // Also record in FFI debounce tracker (shared with JB plugin)
                native.documentChanged(fsPath, this.patchesDir ? path.dirname(path.dirname(this.patchesDir)) : undefined);
            }
        });

        this.outputChannel.appendLine(`PatchWatcher: watching ${patchesDir}`);

        // Process any existing patch files and signals on startup
        this.processVcsRefreshSignal(patchesDir);
        this.processPendingPatches(patchesDir);
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

    private async onPatchFileCreated(uri: vscode.Uri): Promise<void> {
        try {
            const raw = fs.readFileSync(uri.fsPath, 'utf-8');
            const patch = JSON.parse(raw) as IpcPatch;

            if (!patch.file) {
                this.outputChannel.appendLine(`PatchWatcher: invalid patch (no file field): ${uri.fsPath}`);
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

            // Handle reposition-only signals with typing debounce
            if (patch.reposition_boundary && patch.patches.length === 0) {
                this.repositionBoundaryWithDebounce(
                    patch.file,
                    uri.fsPath,
                    patch.reposition_boundary_id,
                    0,
                    patch.preserve_head ?? false,
                );
                return;
            }

            const applied = await this.applyPatch(patch);

            if (applied) {
                // ACK: delete the patch file
                try {
                    fs.unlinkSync(uri.fsPath);
                } catch (e: any) {
                    this.outputChannel.appendLine(`PatchWatcher: failed to delete patch file: ${e.message}`);
                }
            } else {
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
            if (repositioned && repositioned !== content && document.getText() === content) {
                const fullRange = new vscode.Range(
                    document.positionAt(0),
                    document.positionAt(content.length),
                );
                const edit = new vscode.WorkspaceEdit();
                edit.replace(fileUri, fullRange, repositioned);
                await vscode.workspace.applyEdit(edit);
                await document.save();
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

    private async applyPatch(patch: IpcPatch): Promise<boolean> {
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
        const fullRange = new vscode.Range(
            document.positionAt(0),
            document.positionAt(baselineContent.length),
        );

        // Full content replacement — only for append-mode documents without component patches.
        // When component patches are present, use the patch path instead so the response
        // is applied correctly. Applying fullContent alongside patches would replace the
        // document before patches run, causing the response to be lost or duplicated.
        if (patch.fullContent != null && patch.fullContent !== '' && patch.patches.length === 0) {
            const content = baselineContent;
            if (patch.fullContent !== content) {
                const edit = new vscode.WorkspaceEdit();
                edit.replace(fileUri, fullRange, patch.fullContent);
                const ok = await vscode.workspace.applyEdit(edit);
                if (!ok) {
                    this.outputChannel.appendLine(`PatchWatcher: WorkspaceEdit failed for full content replacement`);
                    return false;
                }
            }
            await document.save();
            return true;
        }

        // Component-based patching (template/stream-mode documents)
        let content = baselineContent;

        // Apply frontmatter patch first
        if (patch.frontmatter) {
            content = this.applyFrontmatterPatch(content, patch.frontmatter);
        }

        // Apply component patches
        for (const p of patch.patches) {
            content = this.applyComponentPatch(content, p.component, p.content);
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

        // Apply ❯  prefix normalization for user-input lines in exchange
        if (patch.normalize_prefix_lines && patch.normalize_prefix_lines.length > 0) {
            content = this.normalizeExchangePrefixes(content, patch.normalize_prefix_lines);
        }

        content = annotateExchangeHeadingsAgainstBaseline(content, 'exchange', baselineContent) ?? content;

        // Apply the combined edit
        if (content !== baselineContent) {
            const edit = new vscode.WorkspaceEdit();
            edit.replace(fileUri, fullRange, content);
            const ok = await vscode.workspace.applyEdit(edit);
            if (!ok) {
                this.outputChannel.appendLine(`PatchWatcher: WorkspaceEdit failed for component patches`);
                return false;
            }
        }

        await document.save();
        return true;
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

        const normalizedLines = userRegion.split('\n').map(docLine => {
            const normalized = docLine.trimEnd();
            if (normalized.startsWith('❯ ')) return docLine;       // already prefixed
            if (targetLines.has(normalized)) return `❯ ${docLine}`; // match — add prefix
            return docLine;
        });
        userRegion = normalizedLines.join('\n');

        return beforeExchange + userRegion + agentRegion + afterExchange;
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
    private applyComponentPatch(doc: string, component: string, content: string): string {
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
        let mode = 'replace';
        if (openMatch[1]) {
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
        this.typingListener?.dispose();
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

    // Feature 3: Sync Layout
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.syncLayout', syncLayoutAction)
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
                const visibleMd = collectVisibleMdFiles(root);
                runCli(['sync', '--col', visibleMd.join(','), '--focus', newRel, '--rename'], root).catch(() => {});
            }
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
    lastTabSyncSignature = '';
    resolvedAgentDoc = null;
    commandRunning = false;
}
