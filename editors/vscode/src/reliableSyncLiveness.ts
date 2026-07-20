/**
 * agent-doc VS Code reliable-sync liveness reporter (sidecar-retirement Phase 3C,
 * design B).
 *
 * Symmetric with the JetBrains `ReliableSyncLivenessListener`: this editor hosts
 * a real lazily-js liveness graph (`OrSet` per open document — the same add-wins
 * convergent cell the controller's `LivenessProjection` folds), derives the
 * externally-tagged `LivenessOp` batch it pushes, and hands it to the Rust FFI
 * (`reliableSyncLivenessEnqueue` / `Flush`) which keeps the durable outbox + the
 * controller socket. The historical dual-run flag can still disable the channel
 * for rollback; default-on delivery feeds the authoritative, durably journaled
 * controller projection. `Alive{false}` is injected controller-side by the S4b
 * OS exit watcher, not here.
 */
import * as vscode from 'vscode';
import { randomUUID } from 'crypto';
// esbuild inlines this at build time; the 4-up path reaches `src/lazily-js` in
// the monorepo (same resolution the state-graph-mirror import uses).
import { OrSet } from '@lazily-hub/lazily-js';
import {
documentIdForPath,
EDITOR_CAPABILITY_LIST,
EDITOR_PLUGIN_KIND,
EDITOR_PLUGIN_VERSION,
isSessionDocument,
    reliableSyncLivenessEnqueue,
    reliableSyncLivenessFlush,
} from './native.js';

/** This editor's open-set as lazily-js `OrSet`s, one per `document_hash`. */
class LivenessGraph {
    private readonly docs = new Map<string, { orSet: any; tags: string[] }>();

    constructor(private readonly pid: number) {}

    /** Mark `documentHash` opened once; returns null for a duplicate IDE event. */
open(documentHash: string, path: string, editorId: string): string | null {
        if (this.docs.get(documentHash)?.orSet.present() === true) return null;
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
{
Register: {
document_hash: documentHash,
pid: this.pid,
path,
editor_id: editorId,
editor_kind: EDITOR_PLUGIN_KIND,
editor_version: EDITOR_PLUGIN_VERSION,
capabilities: [...EDITOR_CAPABILITY_LIST].sort(),
timestamp_ms: Date.now(),
},
},
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
export function registerReliableSyncLiveness(
context: vscode.ExtensionContext,
editorId: string,
): void {
    const graph = new LivenessGraph(process.pid);

    const workspaceRootFor = (document: vscode.TextDocument): string | undefined =>
        vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath
            ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;

    const push = (root: string, documentHash: string, opsJson: string) => {
        if (reliableSyncLivenessEnqueue(root, documentHash, opsJson) === 0) {
            reliableSyncLivenessFlush(root, documentHash);
        }
    };

    const reportOpen = (document: vscode.TextDocument) => {
        const root = workspaceRootFor(document);
        if (!root) return;
        const filePath = document.uri.fsPath;
        // Off the event loop's critical path — the flush may do a controller RPC.
        setImmediate(() => {
            // Scope liveness to agent-doc session documents only: a plain source
            // file opened as a tab must not enter the plane (it would over-count the
            // session-document scope). This disk read
            // is appropriate at open time — it is the moment we decide whether to
            // start tracking a possibly-random `.md` tab at all.
            if (!isSessionDocument(filePath, root)) return;
            const documentHash = documentIdForPath(filePath, root);
            if (!documentHash) return;
const opsJson = graph.open(documentHash, filePath, editorId);
            if (!opsJson) return;
            push(root, documentHash, opsJson);
        });
    };

    const reportClose = (document: vscode.TextDocument) => {
        const root = workspaceRootFor(document);
        if (!root) return;
        const filePath = document.uri.fsPath;
        setImmediate(() => {
            const documentHash = documentIdForPath(filePath, root);
            if (!documentHash) return;
            // `#lzsync-close-no-disk-regate`: do not re-check `isSessionDocument` here
            // (see the JB `ReliableSyncLivenessListener` for the rationale) — a file
            // can legitimately become unreadable at close time even though this
            // editor genuinely opened it as a tracked session document earlier, and
            // re-gating on a disk read would silently drop the compensating `Close`
            // op, leaving the plane's OrSet permanently "present". `graph.close()` is
            // itself the correct gate: it returns null when this editor never opened
            // the doc.
            const opsJson = graph.close(documentHash);
            if (!opsJson) return;
            push(root, documentHash, opsJson);
        });
    };

    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument(reportOpen),
        vscode.workspace.onDidCloseTextDocument(reportClose),
    );

    // Activation can occur after VS Code has restored editor tabs, in which case
    // no fresh onDidOpen event is emitted. Seed those already-open documents so a
    // new installation/controller has a durable liveness fact immediately.
    for (const document of vscode.workspace.textDocuments) reportOpen(document);
}
