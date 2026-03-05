import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import * as fs from 'fs';
import { execFile } from 'child_process';

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
        const rel = relativePath(root, editor.document.uri.fsPath);
        const output = await runCli(['route', rel], root);
        showHint(output || `Routed ${rel}`);
        // Track file for prompt polling
        trackedFiles.add(editor.document.uri.fsPath);
        ensurePromptPolling(root);
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
        const rel = relativePath(root, editor.document.uri.fsPath);
        const split = detectSplit(editor);
        const args = ['claim', rel];
        if (split.position) {
            args.push('--position', split.position);
        }

        const output = await runCli(args, root);
        showHint(output || `Claimed ${rel} (pos=${split.position || 'none'})`);

        // Trigger silent layout sync after claiming
        await syncLayoutInternal(root, false);
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

    // Reset state
    lastTabSyncSignature = '';
    resolvedAgentDoc = null;
    commandRunning = false;
}
