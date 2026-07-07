/**
 * #lzlosstree Phase 5 plugin apply side (VS Code): poll the lossless-tree frame the
 * binary drops for a tree-capable session and apply it to the live TextDocument.
 *
 * The binary owns the frame path (`losslessTreeFramePath`); when a frame is present the
 * watcher renders it (`losslessTreeRenderFrame`, native) and replaces the document text
 * via a WorkspaceEdit, then deletes the frame as an ACK. Rendering/hashing stay native
 * (FFI-first); the extension only moves the resulting text into the document.
 */
import * as vscode from 'vscode';
import * as fs from 'fs';
import { losslessTreeFramePath, losslessTreeRenderFrame } from './native';

const POLL_INTERVAL_MS = 150;

class LosslessTreeFrameWatcher {
    private timer: NodeJS.Timeout | undefined;

    constructor(private readonly document: vscode.TextDocument) {}

    start(): void {
        if (this.timer) return;
        this.timer = setInterval(() => { void this.tick(); }, POLL_INTERVAL_MS);
    }

    dispose(): void {
        if (this.timer) {
            clearInterval(this.timer);
            this.timer = undefined;
        }
    }

    private async tick(): Promise<void> {
        const filePath = this.document.uri.fsPath;
        const framePath = losslessTreeFramePath(filePath);
        if (!framePath || !fs.existsSync(framePath)) return;
        const rendered = losslessTreeRenderFrame(framePath);
        // Corrupt/absent frame: keep the buffer and let the binary re-emit.
        if (rendered === null) return;
        if (this.document.getText() === rendered) {
            // Already current — consume the frame as an ACK.
            try { fs.unlinkSync(framePath); } catch { /* re-poll */ }
            return;
        }
        const edit = new vscode.WorkspaceEdit();
        const fullRange = new vscode.Range(
            this.document.positionAt(0),
            this.document.positionAt(this.document.getText().length),
        );
        edit.replace(this.document.uri, fullRange, rendered);
        const applied = await vscode.workspace.applyEdit(edit);
        if (applied) {
            try { fs.unlinkSync(framePath); } catch { /* re-poll */ }
        }
    }
}

/**
 * Start a frame watcher for every open markdown document and keep the set in sync with
 * open/close events. Call once from the extension's `activate`.
 */
export function activateLosslessFrameWatchers(context: vscode.ExtensionContext): void {
    const watchers = new Map<string, LosslessTreeFrameWatcher>();

    const watch = (document: vscode.TextDocument): void => {
        if (document.languageId !== 'markdown' && !document.uri.fsPath.endsWith('.md')) return;
        const key = document.uri.toString();
        if (watchers.has(key)) return;
        const watcher = new LosslessTreeFrameWatcher(document);
        watcher.start();
        watchers.set(key, watcher);
    };

    const unwatch = (document: vscode.TextDocument): void => {
        const key = document.uri.toString();
        watchers.get(key)?.dispose();
        watchers.delete(key);
    };

    vscode.workspace.textDocuments.forEach(watch);
    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument(watch),
        vscode.workspace.onDidCloseTextDocument(unwatch),
        { dispose: () => { watchers.forEach((w) => w.dispose()); watchers.clear(); } },
    );
}
