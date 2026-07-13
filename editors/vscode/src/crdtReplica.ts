import * as net from 'net';
import * as path from 'path';
import { createHash } from 'crypto';
import {
    NativeReplicaNode,
    reliableSyncDocumentOpFlush,
    reliableSyncDocumentOpPush,
    reliableSyncTextAdoptPush,
} from './native';

export interface ReplicaRegisterAck {
    clientId: number;
    bootstrap?: Uint8Array | null;
}

export interface ReplicaRemoteUpdate {
    patchId: string;
    origin: number;
    target: number;
    generation: number;
    update: Uint8Array;
}

export interface ReplicaTransport {
    register(filePath: string, identity: string): Promise<ReplicaRegisterAck | null>;
    broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void>;
    pushDocumentOps?(filePath: string, deltaJson: string): Promise<boolean>;
    pushTextAdopt?(filePath: string, text: string): Promise<boolean>;
    flushDocumentOps?(filePath: string): void;
    pullUpdates(filePath: string, identity: string): Promise<ReplicaRemoteUpdate[]>;
    /** D2: fetch the pending delivery, distinguishing additive deltas from a replace
     * delivery (out-of-band deletion re-bootstrap). Defaults to wrapping pullUpdates. */
    pullDelivery?(filePath: string, identity: string): Promise<ReplicaPullDelivery>;
    ackUpdate(filePath: string, identity: string, patchId: string, generation: number): Promise<boolean>;
    deregister(filePath: string, identity: string): Promise<void>;
}

export interface ReplicaNode {
    open(clientId: number, initState?: Uint8Array | null): boolean;
    applyLocal(clientId: number, offset: number, deleteLen: number, insert: string): boolean;
    applyUpdate(clientId: number, update: Uint8Array): boolean;
    encodeState(): Uint8Array | null;
    stateVector?(): Uint8Array | null;
    diff?(theirStateVector: Uint8Array): Uint8Array | null;
    text(): string | null;
    close(clientId?: number): void;
}

export interface ReplicaTextChange {
    rangeOffset: number;
    rangeLength: number;
    text: string;
}

export interface ReplicaDocumentSnapshot {
    filePath: string;
    text: string;
}

interface ControllerResponse {
    ok: boolean;
    data?: unknown;
    error?: string;
}

interface ReplicaLogger {
    debug(message: string): void;
    warn(message: string): void;
}

const noopLogger: ReplicaLogger = {
    debug: () => {},
    warn: () => {},
};

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function asNumber(value: unknown): number | null {
    return typeof value === 'number' && Number.isSafeInteger(value) ? value : null;
}

function decodeBase64(value: unknown): Uint8Array | null {
    if (typeof value !== 'string') return null;
    try {
        return Buffer.from(value, 'base64');
    } catch {
        return null;
    }
}

function sha256(value: string): string {
    return createHash('sha256').update(value, 'utf8').digest('hex');
}

export function utf16RangeToCodePoints(
    oldText: string,
    rangeOffset: number,
    rangeLength: number,
): { offset: number; deleteLen: number } {
    const start = Math.max(0, Math.min(rangeOffset, oldText.length));
    const end = Math.max(start, Math.min(start + rangeLength, oldText.length));
    return {
        offset: Array.from(oldText.slice(0, start)).length,
        deleteLen: Array.from(oldText.slice(start, end)).length,
    };
}

export function applyReplicaTextChange(oldText: string, change: ReplicaTextChange): string | null {
    const start = Math.max(0, Math.min(change.rangeOffset, oldText.length));
    const end = Math.max(start, Math.min(start + change.rangeLength, oldText.length));
    return oldText.slice(0, start) + change.text + oldText.slice(end);
}

export function shouldApplyRemoteUpdate(update: ReplicaRemoteUpdate, clientId: number): boolean {
    return update.origin !== clientId;
}

export function parseRegisterResponse(response: ControllerResponse): ReplicaRegisterAck | null {
    if (!response.ok || !isRecord(response.data)) return null;
    const clientId = asNumber(response.data.client_id);
    if (clientId == null) return null;
    return {
        clientId,
        bootstrap: decodeBase64(response.data.bootstrap_b64),
    };
}

export function parsePullResponse(response: ControllerResponse): ReplicaRemoteUpdate[] {
    if (!response.ok || !isRecord(response.data) || !Array.isArray(response.data.updates)) return [];
    return response.data.updates.flatMap((entry): ReplicaRemoteUpdate[] => {
        if (!isRecord(entry)) return [];
        const patchId = typeof entry.patch_id === 'string' ? entry.patch_id : null;
        const origin = asNumber(entry.origin) ?? 0;
        const target = asNumber(entry.target) ?? 0;
        const generation = asNumber(entry.generation);
        const update = decodeBase64(entry.update_b64);
        if (!patchId || generation == null || update == null) return [];
        return [{ patchId, origin, target, generation, update }];
    });
}

/**
 * D2: the outcome of a `replica_pull` — a normal additive-delta batch, or a
 * **replace** delivery (out-of-band deletion re-bootstrap) whose text the editor
 * installs into its buffer wholesale instead of CRDT-merging. The CPC
 * decides which (FFI-first); the plugin is a thin consumer. Mirrors the JetBrains
 * `ReplicaPullDelivery` (specs/14-realtime-workflow.md § Editor Parity Requirement).
 */
export type ReplicaPullDelivery =
    | { kind: 'deltas'; updates: ReplicaRemoteUpdate[] }
    | { kind: 'replace'; text: string };

export function parsePullDelivery(response: ControllerResponse): ReplicaPullDelivery {
    if (
        response.ok &&
        isRecord(response.data) &&
        response.data.kind === 'replace' &&
        typeof response.data.replace === 'string'
    ) {
        return { kind: 'replace', text: response.data.replace };
    }
    return { kind: 'deltas', updates: parsePullResponse(response) };
}

export class ControllerSocketReplicaTransport implements ReplicaTransport {
    constructor(
        private readonly projectRoot: string,
        private readonly logger: ReplicaLogger = noopLogger,
    ) {}

    async register(filePath: string, identity: string): Promise<ReplicaRegisterAck | null> {
        const response = await this.send(this.controllerRequest('replica_register', filePath, identity));
        return response ? parseRegisterResponse(response) : null;
    }

    async broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void> {
        await this.send(this.controllerRequest('replica_update', filePath, identity, {
            update_b64: Buffer.from(update).toString('base64'),
        }));
    }

    async pushDocumentOps(filePath: string, deltaJson: string): Promise<boolean> {
        return reliableSyncDocumentOpPush(this.projectRoot, filePath, deltaJson);
    }

    async pushTextAdopt(filePath: string, text: string): Promise<boolean> {
        return reliableSyncTextAdoptPush(this.projectRoot, filePath, text);
    }

    flushDocumentOps(filePath: string): void {
        reliableSyncDocumentOpFlush(this.projectRoot, filePath);
    }

    async pullUpdates(filePath: string, identity: string): Promise<ReplicaRemoteUpdate[]> {
        const response = await this.send(this.controllerRequest('replica_pull', filePath, identity));
        return response ? parsePullResponse(response) : [];
    }

    async pullDelivery(filePath: string, identity: string): Promise<ReplicaPullDelivery> {
        this.flushDocumentOps(filePath);
        const response = await this.send(this.controllerRequest('replica_pull', filePath, identity));
        return response ? parsePullDelivery(response) : { kind: 'deltas', updates: [] };
    }

    async ackUpdate(
        filePath: string,
        identity: string,
        patchId: string,
        generation: number,
    ): Promise<boolean> {
        const response = await this.send(this.controllerRequest('replica_ack', filePath, identity, {
            patch_id: patchId,
            generation,
        }));
        return !!(response?.ok && isRecord(response.data) && response.data.acknowledged === true);
    }

    async deregister(filePath: string, identity: string): Promise<void> {
        await this.send(this.controllerRequest('replica_deregister', filePath, identity));
    }

    private controllerRequest(
        method: string,
        filePath: string,
        identity: string,
        fields: Record<string, unknown> = {},
    ): Record<string, unknown> {
        return {
            command: 'crdt_replica',
            file: filePath,
            diagnostic_payload: JSON.stringify({
                method,
                identity,
                source: 'vscode_plugin',
                ...fields,
            }),
        };
    }

    private async send(request: Record<string, unknown>): Promise<ControllerResponse | null> {
        const socketPath = this.controllerSocket();
        try {
            return await this.sendToSocket(socketPath, request);
        } catch (e: any) {
            this.logger.debug(`[crdt-replica] controller socket ${socketPath} unavailable: ${e?.message ?? e}`);
            return null;
        }
    }

    private controllerSocket(): string {
        return path.join(this.projectRoot, '.agent-doc', 'controller.sock');
    }

    private sendToSocket(socketPath: string, request: Record<string, unknown>): Promise<ControllerResponse> {
        return new Promise((resolve, reject) => {
            const socket = net.createConnection(socketPath);
            let buffer = '';
            let settled = false;
            let timeout: ReturnType<typeof setTimeout> | undefined;

            const finish = (err: Error | null, response?: ControllerResponse) => {
                if (settled) return;
                settled = true;
                if (timeout) clearTimeout(timeout);
                socket.removeAllListeners();
                socket.destroy();
                if (err) reject(err);
                else resolve(response ?? { ok: false, error: 'empty response' });
            };

            const parseLine = (line: string) => {
                if (line.trim().length === 0) {
                    finish(new Error('empty response'));
                    return;
                }
                try {
                    finish(null, JSON.parse(line) as ControllerResponse);
                } catch (e: any) {
                    finish(new Error(`invalid controller response: ${e?.message ?? e}`));
                }
            };

            timeout = setTimeout(() => finish(new Error('timeout waiting for controller response')), 1_000);
            socket.setEncoding('utf8');
            socket.once('connect', () => {
                socket.write(`${JSON.stringify(request)}\n`);
            });
            socket.on('data', (chunk: string) => {
                buffer += chunk;
                const newline = buffer.indexOf('\n');
                if (newline >= 0) parseLine(buffer.slice(0, newline));
            });
            socket.once('error', (err) => finish(err));
            socket.once('end', () => {
                if (!settled) parseLine(buffer);
            });
        });
    }
}

export class CrdtReplicaForwarder {
    attached = false;
    private clientId = 0;
    private pushedVersion: Uint8Array | null = null;

    constructor(
        private readonly filePath: string,
        private readonly identity: string,
        private readonly node: ReplicaNode,
        private readonly transport: ReplicaTransport,
    ) {}

    get currentClientId(): number {
        return this.clientId;
    }

    async register(): Promise<boolean> {
        const ack = await this.transport.register(this.filePath, this.identity);
        if (!ack) return false;
        this.clientId = ack.clientId;
        if (!this.node.open(ack.clientId, ack.bootstrap)) return false;
        this.attached = true;
        this.pushedVersion = this.node.stateVector?.() ?? new Uint8Array();
        return true;
    }

    async forwardLocalDelta(offset: number, deleteLen: number, insert: string): Promise<void> {
        if (!this.attached) return;
        if (!this.node.applyLocal(this.clientId, offset, deleteLen, insert)) return;
        await this.publishIncremental();
    }

    async ensureEditorText(editorText: string): Promise<void> {
        if (!this.attached) return;
        const current = this.node.text();
        if (current == null || current === editorText) return;
        const deleteLen = Array.from(current).length;
        if (!this.node.applyLocal(this.clientId, 0, deleteLen, editorText)) return;
        await this.publishIncremental();
    }

    private async publishIncremental(): Promise<void> {
        const frontier = this.pushedVersion ?? this.node.stateVector?.() ?? new Uint8Array();
        const update = this.node.diff?.(frontier) ?? this.node.encodeState();
        if (!update) return;
        const durable = this.transport.pushDocumentOps
            ? await this.transport.pushDocumentOps(this.filePath, Buffer.from(update).toString('utf8'))
            : true;
        if (durable) this.pushedVersion = this.node.stateVector?.() ?? this.pushedVersion;
        await this.transport.broadcastUpdate(this.filePath, this.identity, update);
    }

    async pushTextAdopt(editorText: string): Promise<boolean> {
        return this.transport.pushTextAdopt
            ? this.transport.pushTextAdopt(this.filePath, editorText)
            : false;
    }

    applyRemoteUpdate(update: Uint8Array): string | null {
        if (!this.attached) return null;
        if (!this.node.applyUpdate(this.clientId, update)) return null;
        this.pushedVersion = this.node.stateVector?.() ?? this.pushedVersion;
        return this.node.text();
    }

    replicaText(): string | null {
        if (!this.attached) return null;
        return this.node.text();
    }

    pullRemoteUpdates(): Promise<ReplicaRemoteUpdate[]> {
        if (!this.attached) return Promise.resolve([]);
        return this.transport.pullUpdates(this.filePath, this.identity);
    }

    /** D2: pull the pending delivery (normal deltas, or a replace delivery whose
     * text the caller installs wholesale for an out-of-band deletion re-bootstrap). */
    pullRemoteDelivery(): Promise<ReplicaPullDelivery> {
        if (!this.attached) return Promise.resolve({ kind: 'deltas', updates: [] });
        if (this.transport.pullDelivery) {
            return this.transport.pullDelivery(this.filePath, this.identity);
        }
        return this.transport
            .pullUpdates(this.filePath, this.identity)
            .then((updates) => ({ kind: 'deltas', updates }));
    }

    ackRemoteUpdate(update: ReplicaRemoteUpdate): Promise<boolean> {
        if (!this.attached) return Promise.resolve(false);
        return this.transport.ackUpdate(this.filePath, this.identity, update.patchId, update.generation);
    }

    async deregister(): Promise<void> {
        if (!this.attached) return;
        await this.transport.deregister(this.filePath, this.identity);
        this.node.close(this.clientId);
        this.pushedVersion = null;
        this.attached = false;
    }
}

export interface CrdtReplicaManagerOptions {
    projectRoot: string;
    identity: string;
    transport?: ReplicaTransport;
    nodeFactory?: () => ReplicaNode;
    listDocuments: () => ReplicaDocumentSnapshot[];
    currentText: (filePath: string) => string | null;
    applyText: (filePath: string, text: string, expectedText: string) => Promise<boolean>;
    logger?: ReplicaLogger;
}

export class CrdtReplicaManager {
    private readonly transport: ReplicaTransport;
    private readonly nodeFactory: () => ReplicaNode;
    private readonly logger: ReplicaLogger;
    private readonly shadows = new Map<string, string>();
    private readonly forwarders = new Map<string, CrdtReplicaForwarder>();
    private readonly attaching = new Map<string, Promise<CrdtReplicaForwarder | null>>();
    private readonly applyingRemote = new Set<string>();
    private readonly reattachRecovering = new Set<string>();
    private readonly pendingLocalEdits = new Map<string, number>();
    private readonly drainRequestedPaths = new Set<string>();
    private drainAllRequested = false;
    private drainQueued = false;
    private drainTimer: ReturnType<typeof setTimeout> | undefined;
    private disposed = false;

    constructor(private readonly options: CrdtReplicaManagerOptions) {
        this.logger = options.logger ?? noopLogger;
        this.transport = options.transport ?? new ControllerSocketReplicaTransport(options.projectRoot, this.logger);
        this.nodeFactory = options.nodeFactory ?? (() => new NativeReplicaNode(options.projectRoot));
    }

    start(): void {
        this.disposed = false;
        for (const doc of this.options.listDocuments()) {
            this.seedDocument(doc.filePath, doc.text);
            void this.attachDocument(doc.filePath);
        }
    }

    dispose(): void {
        this.disposed = true;
        this.drainRequestedPaths.clear();
        this.drainAllRequested = false;
        this.drainQueued = false;
        if (this.drainTimer) clearTimeout(this.drainTimer);
        this.drainTimer = undefined;
        for (const forwarder of this.forwarders.values()) {
            void forwarder.deregister();
        }
        this.forwarders.clear();
        this.attaching.clear();
        this.shadows.clear();
    }

    seedDocument(filePath: string, text: string): void {
        this.shadows.set(filePath, text);
    }

    async attachDocument(filePath: string, text?: string, forceRefresh = false): Promise<boolean> {
        if (text !== undefined) this.seedDocument(filePath, text);
        const forwarder = await this.forwarderFor(filePath);
        if (forceRefresh && forwarder && text !== undefined) await forwarder.ensureEditorText(text);
        if (forwarder) this.requestRemoteDrain(filePath);
        return forwarder != null;
    }

    isApplyingRemote(filePath: string): boolean {
        return this.applyingRemote.has(filePath);
    }

    async handleDocumentClosed(filePath: string): Promise<void> {
        this.shadows.delete(filePath);
        this.attaching.delete(filePath);
        const forwarder = this.forwarders.get(filePath);
        this.forwarders.delete(filePath);
        await forwarder?.deregister();
    }

    async handleLocalChange(
        filePath: string,
        newText: string,
        changes: readonly ReplicaTextChange[],
    ): Promise<void> {
        if (this.applyingRemote.has(filePath)) {
            this.shadows.set(filePath, newText);
            return;
        }
        const oldText = this.shadows.get(filePath);
        this.shadows.set(filePath, newText);
        if (oldText === undefined || changes.length !== 1) return;

        const change = changes[0];
        const { offset, deleteLen } = utf16RangeToCodePoints(
            oldText,
            change.rangeOffset,
            change.rangeLength,
        );
        this.markLocalPending(filePath);
        try {
            const forwarder = await this.forwarderFor(filePath);
            await forwarder?.forwardLocalDelta(offset, deleteLen, change.text);
        } finally {
            this.clearLocalPending(filePath);
            this.requestRemoteDrain(filePath);
        }
    }

    async handleLocalChangeDelta(
        filePath: string,
        changes: readonly ReplicaTextChange[],
    ): Promise<void> {
        if (this.applyingRemote.has(filePath)) {
            return;
        }
        const oldText = this.shadows.get(filePath);
        if (oldText === undefined || changes.length !== 1) return;

        const change = changes[0];
        const newText = applyReplicaTextChange(oldText, change);
        if (newText == null) {
            this.shadows.delete(filePath);
            return;
        }
        this.shadows.set(filePath, newText);
        const { offset, deleteLen } = utf16RangeToCodePoints(
            oldText,
            change.rangeOffset,
            change.rangeLength,
        );
        this.markLocalPending(filePath);
        try {
            const forwarder = await this.forwarderFor(filePath);
            await forwarder?.forwardLocalDelta(offset, deleteLen, change.text);
        } finally {
            this.clearLocalPending(filePath);
            this.requestRemoteDrain(filePath);
        }
    }

    /**
     * D2 — apply a REPLACE delivery: install the corrected canonical text into the
     * buffer wholesale (an out-of-band deletion the additive CRDT delta cannot
     * express), then re-bootstrap the local replica so later deltas are relative to
     * the corrected state. Never clobbers unsaved operator edits (fail-open).
     */
    private async applyReplaceDelivery(
        filePath: string,
        forwarder: CrdtReplicaForwarder,
        canonical: string,
    ): Promise<void> {
        if (this.hasPendingLocal(filePath)) return;
        const expectedText = this.shadows.get(filePath) ?? '';
        this.applyingRemote.add(filePath);
        let installed = false;
        try {
            installed = await this.options.applyText(filePath, canonical, expectedText);
            if (installed) this.shadows.set(filePath, canonical);
        } finally {
            this.applyingRemote.delete(filePath);
        }
        if (installed) {
            // Re-bootstrap the native replica against the corrected canonical.
            await forwarder.deregister();
            await forwarder.register();
        }
    }

    requestRemoteDrain(filePath?: string): void {
        if (this.disposed) return;
        if (filePath) {
            this.drainRequestedPaths.add(filePath);
        } else {
            this.drainAllRequested = true;
        }
        if (this.drainQueued) return;
        this.drainQueued = true;
        this.drainTimer = setTimeout(() => {
            this.drainTimer = undefined;
            void this.drainRequestedRemoteUpdates();
        }, 0);
    }

    /** Controller-proven genuine reattach: bounded text adopt, then re-bootstrap. */
    async handleReattachRequest(filePath: string): Promise<void> {
        if (this.reattachRecovering.has(filePath)) return;
        this.reattachRecovering.add(filePath);
        try {
        const forwarder = this.forwarders.get(filePath);
        if (!forwarder) return;
        const editorText = this.currentEditorText(filePath) ?? this.shadows.get(filePath);
        if (editorText === undefined || !(await forwarder.pushTextAdopt(editorText))) return;
        if (this.forwarders.get(filePath) !== forwarder) return;
        this.forwarders.delete(filePath);
        await forwarder.deregister();
        const reattached = await this.forwarderFor(filePath);
        if (reattached) this.requestRemoteDrain(filePath);
        } finally {
            this.reattachRecovering.delete(filePath);
        }
    }

    async drainRemoteUpdates(filePath?: string): Promise<void> {
        if (filePath) {
            this.drainRequestedPaths.delete(filePath);
        } else {
            this.drainRequestedPaths.clear();
            this.drainAllRequested = false;
        }
        if (this.drainTimer && !this.drainAllRequested && this.drainRequestedPaths.size === 0) {
            clearTimeout(this.drainTimer);
            this.drainTimer = undefined;
            this.drainQueued = false;
        }
        const paths = filePath ? [filePath] : Array.from(this.forwarders.keys());
        await this.drainRemoteUpdatesForPaths(paths);
    }

    private async drainRequestedRemoteUpdates(): Promise<void> {
        try {
            const paths = this.drainAllRequested
                ? Array.from(this.forwarders.keys())
                : Array.from(this.drainRequestedPaths);
            this.drainAllRequested = false;
            for (const filePath of paths) {
                this.drainRequestedPaths.delete(filePath);
            }
            await this.drainRemoteUpdatesForPaths(paths);
        } finally {
            this.drainQueued = false;
            if (!this.disposed && (this.drainAllRequested || this.drainRequestedPaths.size > 0)) {
                this.requestRemoteDrain();
            }
        }
    }

    private async drainRemoteUpdatesForPaths(paths: Iterable<string>): Promise<void> {
        for (const filePath of new Set(paths)) {
            const forwarder = this.forwarders.get(filePath);
            if (!forwarder) continue;
            if (this.hasPendingLocal(filePath)) continue;
            // D2: a replace delivery (out-of-band deletion re-bootstrap) installs the
            // corrected canonical wholesale; a normal delta batch applies per-update.
            const delivery = await forwarder.pullRemoteDelivery();
            if (delivery.kind === 'replace') {
                await this.applyReplaceDelivery(filePath, forwarder, delivery.text);
                continue;
            }
            const updates = delivery.updates;
            if (this.hasPendingLocal(filePath)) continue;
            for (const update of updates) {
                if (this.hasPendingLocal(filePath)) break;
                if (!shouldApplyRemoteUpdate(update, forwarder.currentClientId)) {
                    await forwarder.ackRemoteUpdate(update);
                    continue;
                }
                const expectedText = this.shadows.get(filePath);
                if (expectedText === undefined) continue;
                if (!(await this.editorReplicaBaselineMatches(filePath, forwarder, expectedText))) continue;
                const converged = forwarder.applyRemoteUpdate(update.update);
                if (converged == null) continue;
                if (this.hasPendingLocal(filePath)) continue;
                this.applyingRemote.add(filePath);
                try {
                    const applied = await this.options.applyText(filePath, converged, expectedText);
                    if (applied) {
                        this.shadows.set(filePath, converged);
                        await forwarder.ackRemoteUpdate(update);
                    } else {
                        const current = this.currentEditorText(filePath);
                        if (current != null) {
                            this.shadows.set(filePath, current);
                            await forwarder.ensureEditorText(current);
                            this.requestRemoteDrain(filePath);
                        }
                    }
                } finally {
                    this.applyingRemote.delete(filePath);
                }
            }
        }
    }

    private async editorReplicaBaselineMatches(
        filePath: string,
        forwarder: CrdtReplicaForwarder,
        expectedText: string,
    ): Promise<boolean> {
        const editorText = this.currentEditorText(filePath);
        if (editorText == null) {
            this.logger.warn(
                `[crdt-replica] incoming update deferred because the authoritative editor buffer is unavailable for ${filePath}: ` +
                `expected_hash=${sha256(expectedText)}`,
            );
            return false;
        }
        const replicaText = forwarder.replicaText();
        if (editorText === expectedText && replicaText === expectedText) return true;
        if (replicaText === expectedText) {
            this.logger.warn(
                `[crdt-replica] incoming update deferred while editor buffer is published first for ${filePath}: ` +
                `editor_hash=${sha256(editorText)} expected_hash=${sha256(expectedText)} replica_hash=${sha256(replicaText)}`,
            );
            this.shadows.set(filePath, editorText);
            await forwarder.ensureEditorText(editorText);
            this.requestRemoteDrain(filePath);
            return false;
        }
        if (replicaText === editorText) {
            this.logger.warn(
                `[crdt-replica] incoming update deferred after shadow realignment for ${filePath}: ` +
                `editor_hash=${sha256(editorText)} expected_hash=${sha256(expectedText)} replica_hash=${sha256(replicaText)}`,
            );
            this.shadows.set(filePath, editorText);
            this.requestRemoteDrain(filePath);
            return false;
        }
        this.logger.warn(
            `[crdt-replica] incoming update deferred because local replica baseline differs from the authoritative editor buffer for ${filePath}: ` +
            `editor_hash=${sha256(editorText)} expected_hash=${sha256(expectedText)} ` +
            `replica_hash=${replicaText == null ? 'missing' : sha256(replicaText)}`,
        );
        this.shadows.set(filePath, editorText);
        await forwarder.ensureEditorText(editorText);
        this.requestRemoteDrain(filePath);
        return false;
    }

    private currentEditorText(filePath: string): string | null {
        return this.options.currentText(filePath);
    }

    private async forwarderFor(filePath: string): Promise<CrdtReplicaForwarder | null> {
        const existing = this.forwarders.get(filePath);
        if (existing) return existing;
        const pending = this.attaching.get(filePath);
        if (pending) return pending;

        const attach = (async () => {
            const identity = `${this.options.identity}:${filePath}`;
            const forwarder = new CrdtReplicaForwarder(
                filePath,
                identity,
                this.nodeFactory(),
                this.transport,
            );
            try {
                if (!(await forwarder.register())) return null;
                const editorText = this.shadows.get(filePath);
                if (editorText !== undefined) await forwarder.ensureEditorText(editorText);
                this.forwarders.set(filePath, forwarder);
                return forwarder;
            } catch (e: any) {
                this.logger.debug(`[crdt-replica] attach skipped for ${filePath}: ${e?.message ?? e}`);
                return null;
            } finally {
                this.attaching.delete(filePath);
            }
        })();
        this.attaching.set(filePath, attach);
        return attach;
    }

    private markLocalPending(filePath: string): void {
        this.pendingLocalEdits.set(filePath, (this.pendingLocalEdits.get(filePath) ?? 0) + 1);
    }

    private clearLocalPending(filePath: string): void {
        const count = this.pendingLocalEdits.get(filePath) ?? 0;
        if (count <= 1) {
            this.pendingLocalEdits.delete(filePath);
        } else {
            this.pendingLocalEdits.set(filePath, count - 1);
        }
    }

    private hasPendingLocal(filePath: string): boolean {
        return (this.pendingLocalEdits.get(filePath) ?? 0) > 0;
    }
}
