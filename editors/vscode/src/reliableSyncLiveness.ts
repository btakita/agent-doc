/**
 * agent-doc VS Code reliable-sync liveness reporter (sidecar-retirement Phase 3C,
 * design B).
 *
 * Symmetric with the JetBrains `ReliableSyncLivenessListener`: this editor hosts
 * a real lazily-js liveness graph (`OrSet` per open document — the same add-wins
 * convergent cell the controller's `LivenessProjection` folds), derives the
 * externally-tagged `LivenessOp` batch it pushes, and hands it to the Rust FFI
 * (`reliableSyncLivenessEnqueue` / `Flush`) which keeps the durable outbox + the
 * controller socket. The FFI enqueue is a no-op unless the controller dual-run
 * flag is on, so this is safe on every install — sidecars stay authoritative
 * until the operator opts into the cutover. `Alive{false}` is injected
 * controller-side by the S4b OS exit watcher, not here.
 */
import * as vscode from 'vscode';
import { randomUUID } from 'crypto';
// esbuild inlines this at build time; the 4-up path reaches `src/lazily-js` in
// the monorepo (same resolution the state-graph-mirror import uses).
import { OrSet } from '../../../../lazily-js/src/index.js';
import {
    documentIdForPath,
    reliableSyncLivenessEnqueue,
    reliableSyncLivenessFlush,
} from './native';

/** This editor's open-set as lazily-js `OrSet`s, one per `document_hash`. */
class LivenessGraph {
    private readonly docs = new Map<string, { orSet: any; tags: string[] }>();

    constructor(private readonly pid: number) {}

    /** Mark `documentHash` opened; returns the `Open` op batch JSON to push. */
    open(documentHash: string): string {
        const tag = randomUUID();
        let state = this.docs.get(documentHash);
        if (!state) {
            state = { orSet: new OrSet(), tags: [] };
            this.docs.set(documentHash, state);
        }
        state.orSet.add(tag);
        state.tags.push(tag);
        return JSON.stringify([
            { Open: { document_hash: documentHash, pid: this.pid, tag } },
        ]);
    }

    /** Mark `documentHash` closed; returns the `Close` op batch JSON, or null. */
    close(documentHash: string): string | null {
        const state = this.docs.get(documentHash);
        if (!state) return null;
        const observed = [...state.tags];
        state.orSet.removeObserved(observed);
        state.tags = [];
        return JSON.stringify([
            { Close: { document_hash: documentHash, pid: this.pid, observed_tags: observed } },
        ]);
    }

    /** Reactive: is this editor currently holding `documentHash` open? */
    isOpen(documentHash: string): boolean {
        return this.docs.get(documentHash)?.orSet.present() === true;
    }
}

/**
 * Wire VS Code open/close events to the liveness graph + FFI push. Returns a
 * disposable that unregisters the listeners.
 */
export function registerReliableSyncLiveness(context: vscode.ExtensionContext): void {
    const graph = new LivenessGraph(process.pid);

    const report = (document: vscode.TextDocument, buildOps: (hash: string) => string | null) => {
        const root = vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath
            ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        if (!root) return;
        const filePath = document.uri.fsPath;
        // Off the event loop's critical path — the flush may do a controller RPC.
        setImmediate(() => {
            const documentHash = documentIdForPath(filePath, root);
            if (!documentHash) return;
            const opsJson = buildOps(documentHash);
            if (!opsJson) return;
            if (reliableSyncLivenessEnqueue(root, documentHash, opsJson) === 0) {
                reliableSyncLivenessFlush(root, documentHash);
            }
        });
    };

    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument((document) => {
            report(document, (hash) => graph.open(hash));
        }),
        vscode.workspace.onDidCloseTextDocument((document) => {
            report(document, (hash) => graph.close(hash));
        }),
    );
}
