import * as net from 'net';
import * as path from 'path';
import { createHash } from 'crypto';
import {
    NativeReplicaNode,
    reliableSyncDocumentOpFlush,
    reliableSyncDocumentOpPush,
    reliableSyncTextAdoptPush,
} from './native.js';

export interface ReplicaRegisterAck {
    clientId: number;
    bootstrap?: Uint8Array | null;
    lineage?: string | null;
}

export interface ReplicaRemoteUpdate {
    patchId: string;
    origin: number;
    target: number;
    generation: number;
    expectedContentHash?: string;
    update: Uint8Array;
}

export interface ReplicaTransport {
    register(filePath: string, identity: string): Promise<ReplicaRegisterAck | null>;
    broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void>;
    pushDocumentOps?(filePath: string, lineage: string | null, deltaJson: string): Promise<boolean>;
    pushTextAdopt?(filePath: string, text: string): Promise<boolean>;
    flushDocumentOps?(filePath: string): void;
    pullUpdates(filePath: string, identity: string): Promise<ReplicaRemoteUpdate[]>;
    /** D2: fetch the pending delivery, distinguishing additive deltas from a replace
     * delivery (out-of-band deletion re-bootstrap). Defaults to wrapping pullUpdates. */
    pullDelivery?(filePath: string, identity: string): Promise<ReplicaPullDelivery>;
    ackUpdate(
        filePath: string,
        identity: string,
        patchId: string,
        generation: number,
        contentHash?: string,
    ): Promise<boolean>;
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

export interface ReplicaLocalChangeAdmission {
    operatorEdit: boolean;
    projectionEpoch: number;
    pendingReserved: boolean;
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

export type TemplateStructureProjectionState = 'exact' | 'repair-required' | 'invalid';

export function templateStructureProjectionState(
    text: string,
    normalized: string | null,
): TemplateStructureProjectionState {
    if (normalized == null) return 'invalid';
    return normalized === text ? 'exact' : 'repair-required';
}

export type RemoteTemplateProjectionDecision =
    | 'queue-remote'
    | 'adopt-exact-editor-baseline'
    | 'retry-fail-closed';

export function remoteTemplateProjectionDecision(
    remoteState: TemplateStructureProjectionState,
    editorState: TemplateStructureProjectionState | null,
    editorMatchesExpected: boolean,
    recoveryInFlight: boolean,
): RemoteTemplateProjectionDecision {
    if (remoteState === 'exact') return 'queue-remote';
    if (recoveryInFlight) return 'retry-fail-closed';
    if (editorMatchesExpected && editorState === 'exact') return 'adopt-exact-editor-baseline';
    return 'retry-fail-closed';
}

export type ReplicaBaselineDecision =
    | 'apply-remote'
    | 'realign-shadow'
    | 'adopt-exact-editor'
    | 'retry-fail-closed';

export function replicaBaselineDecision(
    editorState: TemplateStructureProjectionState | null,
    editorMatchesExpected: boolean,
    replicaMatchesExpected: boolean,
    replicaMatchesEditor: boolean,
    recoveryInFlight: boolean,
): ReplicaBaselineDecision {
    if (recoveryInFlight || editorState !== 'exact') return 'retry-fail-closed';
    if (editorMatchesExpected && replicaMatchesExpected) return 'apply-remote';
    if (replicaMatchesEditor) return 'realign-shadow';
    return 'adopt-exact-editor';
}

export function shouldForwardLocalDelta(replicaText: string | null, shadowText: string): boolean {
    return replicaText === shadowText;
}

interface PendingRemoteAck {
    forwarder: CrdtReplicaForwarder;
    update: ReplicaRemoteUpdate;
}

export interface RemoteAckReplayPlan {
    candidate: ReplicaRemoteUpdate;
    acknowledgedThroughGeneration: number;
}

/**
 * Select an ACK carrier only when the visible editor hash proves a retained
 * delivery frontier. An unrelated visible hash must remain a retry, never a
 * controller rebootstrap request.
 */
export function remoteAckReplayPlan(
    updates: readonly ReplicaRemoteUpdate[],
    visibleContentHash: string,
): RemoteAckReplayPlan | null {
    const matching = updates.filter((update) => update.expectedContentHash === visibleContentHash);
    if (matching.length === 0) return null;
    const acknowledgedThroughGeneration = Math.max(...matching.map((update) => update.generation));
    const candidate = updates
        .filter((update) => update.generation <= acknowledgedThroughGeneration)
        .sort((a, b) => a.generation - b.generation || a.patchId.localeCompare(b.patchId))[0];
    return candidate ? { candidate, acknowledgedThroughGeneration } : null;
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
        lineage: typeof response.data.lineage === 'string' ? response.data.lineage : null,
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
        const expectedContentHash = typeof entry.expected_content_hash === 'string'
            ? entry.expected_content_hash
            : undefined;
        const update = decodeBase64(entry.update_b64);
        if (!patchId || generation == null || update == null) return [];
        return [{ patchId, origin, target, generation, expectedContentHash, update }];
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
    | { kind: 'replace'; text: string }
    | { kind: 'unavailable'; reason: string };

export function parsePullDelivery(response: ControllerResponse): ReplicaPullDelivery {
    if (!response.ok) {
        return { kind: 'unavailable', reason: response.error ?? 'controller_rejected_replica_pull' };
    }
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

    async pushDocumentOps(filePath: string, lineage: string | null, deltaJson: string): Promise<boolean> {
        const payload = lineage == null ? deltaJson : JSON.stringify({ lineage, delta_json: deltaJson });
        return reliableSyncDocumentOpPush(this.projectRoot, filePath, payload);
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
        return response
            ? parsePullDelivery(response)
            : { kind: 'unavailable', reason: 'controller_socket_unavailable' };
    }

    async ackUpdate(
        filePath: string,
        identity: string,
        patchId: string,
        generation: number,
        contentHash?: string,
    ): Promise<boolean> {
        const response = await this.send(this.controllerRequest('replica_ack', filePath, identity, {
            patch_id: patchId,
            generation,
            ...(contentHash ? { content_hash: contentHash } : {}),
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
    private lineage: string | null = null;

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
        this.lineage = ack.lineage ?? null;
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
            ? await this.transport.pushDocumentOps(
                this.filePath,
                this.lineage,
                Buffer.from(update).toString('utf8'),
            )
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

    ackRemoteUpdate(update: ReplicaRemoteUpdate, appliedText?: string): Promise<boolean> {
        if (!this.attached) return Promise.resolve(false);
        return this.transport.ackUpdate(
            this.filePath,
            this.identity,
            update.patchId,
            update.generation,
            appliedText === undefined ? undefined : sha256(appliedText),
        );
    }

    async deregister(): Promise<void> {
        if (!this.attached) return;
        await this.transport.deregister(this.filePath, this.identity);
        this.node.close(this.clientId);
        this.pushedVersion = null;
        this.lineage = null;
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
    resolveDeferredReconnectContent?: (filePath: string, editorText: string) => string | null;
    settleDeferredReconnectContent?: (filePath: string, editorText: string) => void;
    normalizeTemplateStructure?: (text: string) => string | null;
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
    private readonly pendingRemoteAcks = new Map<string, Map<string, PendingRemoteAck>>();
    private readonly templateGuardRecovering = new Set<string>();
    private readonly replicaRetryTimers = new Map<string, ReturnType<typeof setTimeout>>();
    private readonly replicaRetryFailureCounts = new Map<string, number>();
    private readonly pendingLocalEdits = new Map<string, number>();
    private readonly nonOperatorProjectionEpochs = new Map<string, number>();
    private readonly drainRequestedPaths = new Set<string>();
    private drainAllRequested = false;
    private drainQueued = false;
    private drainTimer: ReturnType<typeof setTimeout> | undefined;
    private refreshConnectionEpoch = 0;
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
        for (const timer of this.replicaRetryTimers.values()) clearTimeout(timer);
        this.replicaRetryTimers.clear();
        this.replicaRetryFailureCounts.clear();
        this.templateGuardRecovering.clear();
        this.pendingRemoteAcks.clear();
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
        let registrationText = text ?? this.currentEditorText(filePath) ?? this.shadows.get(filePath);
        let rebootstrapProven = false;
        if (forceRefresh && registrationText !== undefined) {
            const recovered = this.options.resolveDeferredReconnectContent?.(
                filePath,
                registrationText,
            );
            if (recovered != null) {
                rebootstrapProven = true;
            }
            if (recovered != null && recovered !== registrationText) {
                if (this.hasPendingLocal(filePath)) return false;
                this.advanceNonOperatorProjectionEpoch(filePath);
                this.applyingRemote.add(filePath);
                let installed = false;
                try {
                    installed = await this.options.applyText(filePath, recovered, registrationText);
                } finally {
                    this.applyingRemote.delete(filePath);
                }
                if (!installed) {
                    this.logger.warn(
                        `[crdt-replica] deferred reconnect target lost editor CAS for ${filePath}; keeping live editor authority`,
                    );
                    return false;
                }
                registrationText = recovered;
            }
        }
        if (rebootstrapProven) {
            const staleForwarder = this.forwarders.get(filePath);
            if (staleForwarder) {
                this.forwarders.delete(filePath);
                await staleForwarder.deregister();
            }
        }
        if (registrationText !== undefined) this.seedDocument(filePath, registrationText);
        const forwarder = await this.forwarderFor(filePath);
        // A null resolver result means either no retained target or a pending
        // external-disk/editor decision. In both cases the exact editor buffer
        // remains authoritative and is never republished as an unproven reset.
        if (forwarder) {
            if (rebootstrapProven && registrationText !== undefined) {
                this.options.settleDeferredReconnectContent?.(filePath, registrationText);
            }
            this.requestRemoteDrain(filePath);
        }
        return forwarder != null;
    }

    isApplyingRemote(filePath: string): boolean {
        return this.applyingRemote.has(filePath);
    }

    captureLocalChange(filePath: string, operatorEdit: boolean): ReplicaLocalChangeAdmission {
        if (!operatorEdit) this.advanceNonOperatorProjectionEpoch(filePath);
        const admission = {
            operatorEdit,
            projectionEpoch: this.nonOperatorProjectionEpochs.get(filePath) ?? 0,
            pendingReserved: operatorEdit,
        };
        if (operatorEdit) this.markLocalPending(filePath);
        return admission;
    }

    async handleDocumentClosed(filePath: string): Promise<void> {
        this.shadows.delete(filePath);
        this.attaching.delete(filePath);
        this.clearPendingRemoteAcks(filePath);
        this.clearReplicaRetryBackoff(filePath);
        this.templateGuardRecovering.delete(filePath);
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
        admission?: ReplicaLocalChangeAdmission,
    ): Promise<void> {
        const admitted = admission ?? this.captureLocalChange(filePath, true);
        const finish = () => {
            if (admitted.pendingReserved) this.clearLocalPending(filePath);
            this.requestRemoteDrain(filePath);
        };
        if (
            !admitted.operatorEdit ||
            this.applyingRemote.has(filePath) ||
            admitted.projectionEpoch !== (this.nonOperatorProjectionEpochs.get(filePath) ?? 0)
        ) {
            finish();
            return;
        }
        const oldText = this.shadows.get(filePath);
        if (oldText === undefined || changes.length !== 1) {
            finish();
            return;
        }

        const change = changes[0];
        const newText = applyReplicaTextChange(oldText, change);
        if (newText == null) {
            this.requestRemoteDrain(filePath);
            finish();
            return;
        }
        this.shadows.set(filePath, newText);
        const { offset, deleteLen } = utf16RangeToCodePoints(
            oldText,
            change.rangeOffset,
            change.rangeLength,
        );
        try {
            const forwarder = await this.forwarderFor(filePath);
            if (forwarder) {
                const replicaText = forwarder.replicaText();
                if (shouldForwardLocalDelta(replicaText, oldText)) {
                    await forwarder.forwardLocalDelta(offset, deleteLen, change.text);
                } else {
                    this.logger.warn(
                        `[crdt-replica] local delta found a stale native baseline for ${filePath}; ` +
                        `shadow_hash=${sha256(oldText)} ` +
                        `replica_hash=${replicaText == null ? 'missing' : sha256(replicaText)} ` +
                        'recovery=exact_editor_adopt_then_atomic_reregister',
                    );
                    await this.adoptExactEditorBaseline(
                        filePath,
                        newText,
                        forwarder,
                        true,
                        'local-delta-baseline-diverged',
                    );
                }
            }
        } finally {
            finish();
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
        this.advanceNonOperatorProjectionEpoch(filePath);
        this.applyingRemote.add(filePath);
        let installed = false;
        try {
            installed = await this.options.applyText(filePath, canonical, expectedText);
            if (installed) this.shadows.set(filePath, canonical);
        } finally {
            this.applyingRemote.delete(filePath);
        }
        if (!installed) {
            const current = this.currentEditorText(filePath);
            if (current != null) {
                await this.adoptExactEditorBaseline(
                    filePath,
                    current,
                    forwarder,
                    false,
                    'replace-delivery-editor-diverged',
                );
            }
        }
        if (installed) {
            // Re-bootstrap from canonical state instead of locally editing a
            // divergent replica until its text happens to match. The latter
            // mints duplicate ops and can re-corrupt canonical on publish.
            await forwarder.deregister();
            if (this.forwarders.get(filePath) === forwarder) {
                this.forwarders.delete(filePath);
            }
            if (!(await this.forwarderFor(filePath))) {
                this.logger.warn(`[crdt-replica] canonical re-bootstrap could not reattach ${filePath}; the next document event will retry`);
            }
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

    /** Full text adopt is allowed only to recover a proven unsynced user edit. */
    async handleReattachRequest(filePath: string, hasUnsyncedOperatorEdit = false): Promise<void> {
        if (this.reattachRecovering.has(filePath)) return;
        if (!hasUnsyncedOperatorEdit) {
            this.logger.warn(`[crdt-replica] refused full editor text adopt for ${filePath}; no unsynced operator edit proves editor-origin content`);
            this.requestRemoteDrain(filePath);
            return;
        }
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
            await this.replayPendingRemoteAcks(filePath, forwarder);
            if (this.pendingRemoteAckCount(filePath, forwarder) > 0) {
                this.logger.debug(`[crdt-replica] remote pull deferred for ${filePath}; retained delivery ACK is still pending`);
                continue;
            }
            // D2: a replace delivery (out-of-band deletion re-bootstrap) installs the
            // corrected canonical wholesale; a normal delta batch merges before one
            // editor projection so duplicate pull/apply loops cannot amplify work.
            const delivery = await forwarder.pullRemoteDelivery();
            if (delivery.kind === 'unavailable') {
                await this.refreshReplicaAfterTransportLoss(filePath, forwarder, delivery.reason);
                continue;
            }
            if (delivery.kind === 'replace') {
                await this.applyReplaceDelivery(filePath, forwarder, delivery.text);
                continue;
            }
            const updates = delivery.updates;
            if (this.hasPendingLocal(filePath)) continue;
            const expectedText = this.shadows.get(filePath);
            if (expectedText === undefined) continue;
            if (!(await this.editorReplicaBaselineMatches(filePath, forwarder, expectedText))) continue;
            const peerUpdates: ReplicaRemoteUpdate[] = [];
            let converged: string | null = null;
            for (const update of updates) {
                if (this.hasPendingLocal(filePath)) break;
                if (!shouldApplyRemoteUpdate(update, forwarder.currentClientId)) {
                    const visibleText = this.currentEditorText(filePath) ?? this.shadows.get(filePath);
                    if (!(await forwarder.ackRemoteUpdate(update, visibleText))) {
                        this.rememberPendingRemoteAck(filePath, { forwarder, update });
                    }
                    continue;
                }
                converged = forwarder.applyRemoteUpdate(update.update);
                if (converged == null) break;
                peerUpdates.push(update);
            }
            if (converged == null || peerUpdates.length === 0 || this.hasPendingLocal(filePath)) continue;

            const remoteState = this.templateStructureState(converged);
            if (remoteState !== 'exact') {
                await this.recoverRejectedRemoteCanonical(
                    filePath,
                    expectedText,
                    converged,
                    forwarder,
                    remoteState,
                );
                continue;
            }

            this.advanceNonOperatorProjectionEpoch(filePath);
            this.applyingRemote.add(filePath);
            try {
                let projected = await this.options.applyText(filePath, converged, expectedText);
                if (!projected) {
                    const current = this.currentEditorText(filePath);
                    if (current != null) {
                        projected = await this.applyCanonicalProjection(filePath, converged, current);
                    }
                }
                const visibleText = this.currentEditorText(filePath);
                if (projected && visibleText === converged) {
                    this.shadows.set(filePath, converged);
                    for (const update of peerUpdates) {
                        this.rememberPendingRemoteAck(filePath, { forwarder, update });
                    }
                    await this.replayPendingRemoteAcks(filePath, forwarder, converged);
                }
            } finally {
                this.applyingRemote.delete(filePath);
            }
        }
    }

    private remoteAckKey(update: ReplicaRemoteUpdate): string {
        return `${update.patchId}:${update.generation}`;
    }

    private rememberPendingRemoteAck(filePath: string, ack: PendingRemoteAck): void {
        let pending = this.pendingRemoteAcks.get(filePath);
        if (!pending) {
            pending = new Map<string, PendingRemoteAck>();
            this.pendingRemoteAcks.set(filePath, pending);
        }
        pending.set(this.remoteAckKey(ack.update), ack);
        this.scheduleReplicaRetry(filePath, 'delivery-ack-retained');
    }

    private pendingRemoteAckCount(filePath: string, forwarder: CrdtReplicaForwarder): number {
        const pending = this.pendingRemoteAcks.get(filePath);
        if (!pending) return 0;
        return Array.from(pending.values()).filter((ack) => ack.forwarder === forwarder).length;
    }

    private clearPendingRemoteAcks(filePath: string): number {
        const pending = this.pendingRemoteAcks.get(filePath);
        this.pendingRemoteAcks.delete(filePath);
        return pending?.size ?? 0;
    }

    private async replayPendingRemoteAcks(
        filePath: string,
        forwarder: CrdtReplicaForwarder,
        knownVisibleText?: string,
    ): Promise<number> {
        const pending = this.pendingRemoteAcks.get(filePath);
        if (!pending) return 0;
        for (const [key, ack] of pending) {
            if (ack.forwarder !== forwarder) pending.delete(key);
        }
        if (pending.size === 0) {
            this.pendingRemoteAcks.delete(filePath);
            return 0;
        }
        const visibleText = knownVisibleText ?? this.currentEditorText(filePath);
        if (visibleText == null) {
            this.scheduleReplicaRetry(filePath, 'delivery-ack-editor-unavailable');
            return 0;
        }
        const visibleHash = sha256(visibleText);
        const updates = Array.from(pending.values()).map((ack) => ack.update);
        const plan = remoteAckReplayPlan(updates, visibleHash);
        if (!plan) {
            this.scheduleReplicaRetry(filePath, 'delivery-ack-not-visible');
            return 0;
        }
        if (!(await forwarder.ackRemoteUpdate(plan.candidate, visibleText))) {
            this.scheduleReplicaRetry(filePath, 'delivery-ack-pending');
            return 0;
        }
        let acknowledged = 0;
        for (const [key, ack] of pending) {
            if (ack.update.generation <= plan.acknowledgedThroughGeneration) {
                pending.delete(key);
                acknowledged += 1;
            }
        }
        if (pending.size === 0) this.pendingRemoteAcks.delete(filePath);
        if (this.pendingRemoteAckCount(filePath, forwarder) === 0) this.clearReplicaRetryBackoff(filePath);
        return acknowledged;
    }

    private templateStructureState(text: string): TemplateStructureProjectionState {
        const normalized = this.options.normalizeTemplateStructure
            ? this.options.normalizeTemplateStructure(text)
            : text;
        return templateStructureProjectionState(text, normalized);
    }

    private async recoverRejectedRemoteCanonical(
        filePath: string,
        expectedText: string,
        remoteText: string,
        staleForwarder: CrdtReplicaForwarder,
        remoteState: TemplateStructureProjectionState,
    ): Promise<boolean> {
        const editorText = this.currentEditorText(filePath);
        const editorState = editorText == null ? null : this.templateStructureState(editorText);
        const decision = remoteTemplateProjectionDecision(
            remoteState,
            editorState,
            editorText === expectedText,
            this.templateGuardRecovering.has(filePath),
        );
        if (decision !== 'adopt-exact-editor-baseline' || editorText == null) {
            this.logger.warn(
                `[crdt-replica] template-guard recovery deferred for ${filePath}; ` +
                `remote_state=${remoteState} editor_state=${editorState ?? 'missing'} ` +
                `editor_matches_expected=${editorText === expectedText} ` +
                `recovery_in_flight=${this.templateGuardRecovering.has(filePath)} ` +
                `remote_hash=${sha256(remoteText)}`,
            );
            this.scheduleReplicaRetry(filePath, 'template-guard-proof-missing');
            return false;
        }
        this.templateGuardRecovering.add(filePath);
        try {
            if (
                this.forwarders.get(filePath) !== staleForwarder ||
                this.hasPendingLocal(filePath) ||
                this.currentEditorText(filePath) !== editorText
            ) {
                this.scheduleReplicaRetry(filePath, 'template-guard-adopt-fence-raced');
                return false;
            }
            if (!(await staleForwarder.pushTextAdopt(editorText))) {
                this.scheduleReplicaRetry(filePath, 'template-guard-adopt-push-failed');
                return false;
            }
            if (this.hasPendingLocal(filePath) || this.currentEditorText(filePath) !== editorText) {
                this.scheduleReplicaRetry(filePath, 'template-guard-adopt-editor-advanced');
                return false;
            }
            const replacement = await this.replaceForwarder(filePath, staleForwarder, editorText);
            if (!replacement) {
                this.scheduleReplicaRetry(filePath, 'template-guard-reregister-failed');
                return false;
            }
            this.shadows.set(filePath, editorText);
            this.clearReplicaRetryBackoff(filePath);
            this.logger.warn(
                `[crdt-replica] recovered rejected remote canonical for ${filePath}; ` +
                `remote_state=${remoteState} editor_chars=${editorText.length} ` +
                `remote_hash=${sha256(remoteText)} editor_hash=${sha256(editorText)} ` +
                'recovery=exact_editor_adopt_then_atomic_reregister',
            );
            this.requestRemoteDrain(filePath);
            return true;
        } finally {
            this.templateGuardRecovering.delete(filePath);
        }
    }

    private scheduleReplicaRetry(filePath: string, reason: string): void {
        if (this.replicaRetryTimers.has(filePath) || this.disposed) return;
        const failures = (this.replicaRetryFailureCounts.get(filePath) ?? 0) + 1;
        this.replicaRetryFailureCounts.set(filePath, failures);
        const delayMs = Math.min(250 * (2 ** Math.min(failures - 1, 12)), 30_000);
        this.logger.debug(
            `[crdt-replica] recovery retry scheduled for ${filePath}; ` +
            `reason=${reason} delay_ms=${delayMs} failures=${failures}`,
        );
        const timer = setTimeout(() => {
            this.replicaRetryTimers.delete(filePath);
            if (!this.disposed) this.requestRemoteDrain(filePath);
        }, delayMs);
        this.replicaRetryTimers.set(filePath, timer);
    }

    private async adoptExactEditorBaseline(
        filePath: string,
        editorText: string,
        staleForwarder: CrdtReplicaForwarder,
        allowPendingLocal: boolean,
        reason: string,
    ): Promise<boolean> {
        if (this.templateStructureState(editorText) !== 'exact') {
            this.scheduleReplicaRetry(filePath, `${reason}-editor-structure-not-exact`);
            return false;
        }
        if (
            this.forwarders.get(filePath) !== staleForwarder ||
            (!allowPendingLocal && this.hasPendingLocal(filePath)) ||
            this.currentEditorText(filePath) !== editorText
        ) {
            this.scheduleReplicaRetry(filePath, `${reason}-adopt-fence-raced`);
            return false;
        }
        if (!(await staleForwarder.pushTextAdopt(editorText))) {
            this.scheduleReplicaRetry(filePath, `${reason}-adopt-push-failed`);
            return false;
        }
        if (
            this.forwarders.get(filePath) !== staleForwarder ||
            (!allowPendingLocal && this.hasPendingLocal(filePath)) ||
            this.currentEditorText(filePath) !== editorText
        ) {
            this.scheduleReplicaRetry(filePath, `${reason}-editor-advanced`);
            return false;
        }
        const replacement = await this.replaceForwarder(
            filePath,
            staleForwarder,
            editorText,
            allowPendingLocal,
        );
        if (!replacement) {
            this.scheduleReplicaRetry(filePath, `${reason}-reregister-failed`);
            return false;
        }
        this.shadows.set(filePath, editorText);
        this.clearReplicaRetryBackoff(filePath);
        this.logger.warn(
            `[crdt-replica] adopted exact live editor baseline for ${filePath}; ` +
            `reason=${reason} editor_hash=${sha256(editorText)} ` +
            'recovery=exact_editor_adopt_then_atomic_reregister',
        );
        this.requestRemoteDrain(filePath);
        return true;
    }

    private clearReplicaRetryBackoff(filePath: string): void {
        this.replicaRetryFailureCounts.delete(filePath);
        const timer = this.replicaRetryTimers.get(filePath);
        if (timer) clearTimeout(timer);
        this.replicaRetryTimers.delete(filePath);
    }

    private async refreshReplicaAfterTransportLoss(
        filePath: string,
        staleForwarder: CrdtReplicaForwarder,
        reason: string,
    ): Promise<void> {
        const editorText = this.currentEditorText(filePath);
        const expectedText = this.shadows.get(filePath);
        if (
            editorText == null ||
            expectedText === undefined ||
            editorText !== expectedText ||
            this.hasPendingLocal(filePath)
        ) {
            this.scheduleReplicaRetry(filePath, 'controller-transport-editor-not-stable');
            return;
        }
        const replacement = await this.replaceForwarder(filePath, staleForwarder, editorText);
        if (replacement) {
            this.clearReplicaRetryBackoff(filePath);
            this.logger.debug(`[crdt-replica] controller transport recovered for ${filePath}; reason=${reason}`);
            this.requestRemoteDrain(filePath);
        } else {
            this.scheduleReplicaRetry(filePath, 'controller-transport-reregister-failed');
        }
    }

    private async replaceForwarder(
        filePath: string,
        staleForwarder: CrdtReplicaForwarder,
        editorText: string,
        allowPendingLocal = false,
    ): Promise<CrdtReplicaForwarder | null> {
        if (this.forwarders.get(filePath) !== staleForwarder) return null;
        const identity = `${this.options.identity}:${filePath}:refresh-${++this.refreshConnectionEpoch}`;
        const replacement = new CrdtReplicaForwarder(
            filePath,
            identity,
            this.nodeFactory(),
            this.transport,
        );
        if (!(await replacement.register())) return null;
        await replacement.ensureEditorText(editorText);
        if (
            this.forwarders.get(filePath) !== staleForwarder ||
            this.currentEditorText(filePath) !== editorText ||
            (!allowPendingLocal && this.hasPendingLocal(filePath))
        ) {
            // Registration is asynchronous; fence the actual swap, not only
            // the earlier adopt proof, against a newly-arrived editor event.
            await replacement.deregister();
            return null;
        }
        this.forwarders.set(filePath, replacement);
        const retiredPendingAcks = this.clearPendingRemoteAcks(filePath);
        await staleForwarder.deregister();
        this.logger.debug(
            `[crdt-replica] atomically replaced cached forwarder for ${filePath}; ` +
            `retired_pending_acks=${retiredPendingAcks}`,
        );
        return replacement;
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
        const editorState = this.templateStructureState(editorText);
        const decision = replicaBaselineDecision(
            editorState,
            editorText === expectedText,
            replicaText === expectedText,
            replicaText === editorText,
            this.templateGuardRecovering.has(filePath),
        );
        if (decision === 'apply-remote') return true;
        if (decision === 'realign-shadow') {
            this.logger.warn(
                `[crdt-replica] incoming update deferred after shadow realignment for ${filePath}: ` +
                `editor_hash=${sha256(editorText)} expected_hash=${sha256(expectedText)} ` +
                `replica_hash=${replicaText == null ? 'missing' : sha256(replicaText)}`,
            );
            this.shadows.set(filePath, editorText);
            this.requestRemoteDrain(filePath);
            return false;
        }
        if (decision === 'adopt-exact-editor') {
            this.logger.warn(
                `[crdt-replica] incoming update deferred while the exact editor baseline replaces a stale native replica for ${filePath}: ` +
                `editor_hash=${sha256(editorText)} expected_hash=${sha256(expectedText)} ` +
                `replica_hash=${replicaText == null ? 'missing' : sha256(replicaText)}`,
            );
            await this.adoptExactEditorBaseline(
                filePath,
                editorText,
                forwarder,
                false,
                'remote-delivery-baseline-diverged',
            );
            return false;
        }
        this.logger.warn(
            `[crdt-replica] incoming update deferred because editor adoption lacks a stable exact proof for ${filePath}: ` +
            `editor_state=${editorState} editor_hash=${sha256(editorText)} expected_hash=${sha256(expectedText)} ` +
            `replica_hash=${replicaText == null ? 'missing' : sha256(replicaText)}`,
        );
        this.scheduleReplicaRetry(filePath, 'editor-baseline-proof-missing');
        return false;
    }

    private async applyCanonicalProjection(
        filePath: string,
        canonical: string,
        expectedEditorText: string,
    ): Promise<boolean> {
        if (this.hasPendingLocal(filePath)) return false;
        let projected = false;
        this.advanceNonOperatorProjectionEpoch(filePath);
        this.applyingRemote.add(filePath);
        try {
            if (await this.options.applyText(filePath, canonical, expectedEditorText)) {
                this.shadows.set(filePath, canonical);
                projected = true;
                return true;
            }
            return false;
        } finally {
            this.applyingRemote.delete(filePath);
            if (!projected) this.scheduleReplicaRetry(filePath, 'editor-projection-raced');
        }
    }

    private advanceNonOperatorProjectionEpoch(filePath: string): number {
        const next = (this.nonOperatorProjectionEpochs.get(filePath) ?? 0) + 1;
        this.nonOperatorProjectionEpochs.set(filePath, next);
        return next;
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
