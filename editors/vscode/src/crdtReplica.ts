import * as net from 'net';
import * as path from 'path';
import { createHash } from 'crypto';
import {
    NativeReplicaNode,
    peerReplicasMissing,
    reliableSyncDocumentOpFlush,
    reliableSyncDocumentOpPush,
} from './native.js';
import {
    MergeOwnershipStateChart,
    type MergeOwnershipPhase,
} from './mergeOwnershipStateChart.js';
import { peerReplicaRebuildPaths } from './peerReplicaPull.js';

/**
 * `#ctrlkillreregister` Tier 3: minimum gap between whole-editor missing-replica
 * pulls. A controller kill makes every open document report transport loss at once,
 * and one pull already answers for all of them. Mirrors the JetBrains constant.
 */
const PEER_REPLICA_PULL_MIN_INTERVAL_MS = 5_000;

export interface ReplicaRegisterAck {
    clientId: number;
    bootstrap?: Uint8Array | null;
    lineage?: string | null;
    bootstrapKind?: 'full' | 'delta';
    canonicalStateVector?: Uint8Array | null;
}

export interface ReplicaResumeState {
    encodedState: Uint8Array;
    stateVector: Uint8Array;
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
    register(
        filePath: string,
        identity: string,
        stateVector?: Uint8Array | null,
    ): Promise<ReplicaRegisterAck | null>;
    broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void>;
    pushDocumentOps?(filePath: string, lineage: string | null, deltaJson: string): Promise<boolean>;
    flushDocumentOps?(filePath: string): void;
    pullUpdates(filePath: string, identity: string): Promise<ReplicaRemoteUpdate[]>;
    /** D2: fetch the pending delivery, distinguishing additive deltas from a replace
     * delivery (out-of-band deletion re-bootstrap). Defaults to wrapping pullUpdates. */
    pullDelivery?(filePath: string, identity: string): Promise<ReplicaPullDelivery>;
    projectState?(
        filePath: string,
        identity: string,
        contentHash: string,
        diskPersisted: boolean,
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
    | 'retry-fail-closed';

export function remoteTemplateProjectionDecision(
    remoteState: TemplateStructureProjectionState,
    _editorState: TemplateStructureProjectionState | null,
    _editorMatchesExpected: boolean,
    _recoveryInFlight: boolean,
): RemoteTemplateProjectionDecision {
    if (remoteState === 'exact') return 'queue-remote';
    return 'retry-fail-closed';
}

export type ReplicaBaselineDecision =
    | 'apply-remote'
    | 'apply-remote-repair'
    | 'project-remote-target'
    | 'replay-remote-target'
    | 'realign-shadow'
    | 'retry-fail-closed';

export function matchingRemoteTargetGeneration(
    updates: readonly ReplicaRemoteUpdate[],
    contentHash: string | null,
): number | null {
    if (contentHash == null) return null;
    const generations = updates
        .filter((update) => update.expectedContentHash === contentHash)
        .map((update) => update.generation);
    return generations.length === 0 ? null : Math.max(...generations);
}

export function replicaBaselineDecision(
    editorState: TemplateStructureProjectionState | null,
    editorMatchesExpected: boolean,
    replicaMatchesExpected: boolean,
    replicaMatchesEditor: boolean,
    editorMatchesRemoteTarget: boolean,
    replicaMatchesRemoteTarget: boolean,
    recoveryInFlight: boolean,
): ReplicaBaselineDecision {
    if (recoveryInFlight) return 'retry-fail-closed';
    if (editorState === 'exact' && editorMatchesRemoteTarget && replicaMatchesEditor) {
        return 'project-remote-target';
    }
    if (editorMatchesExpected && replicaMatchesRemoteTarget) return 'replay-remote-target';
    if (editorState !== 'exact' && editorMatchesExpected) return 'apply-remote-repair';
    if (editorState !== 'exact') return 'retry-fail-closed';
    if (editorMatchesExpected && replicaMatchesExpected) return 'apply-remote';
    if (replicaMatchesEditor) return 'realign-shadow';
    return 'retry-fail-closed';
}

export function shouldForwardLocalDelta(replicaText: string | null, shadowText: string): boolean {
    return replicaText === shadowText;
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
        bootstrapKind: response.data.bootstrap_kind === 'delta' ? 'delta' : 'full',
        canonicalStateVector: decodeBase64(response.data.canonical_state_vector_b64),
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

    async register(
        filePath: string,
        identity: string,
        stateVector?: Uint8Array | null,
    ): Promise<ReplicaRegisterAck | null> {
        const response = await this.send(this.controllerRequest('replica_register', filePath, identity, {
            ...(stateVector
                ? { state_vector_b64: Buffer.from(stateVector).toString('base64') }
                : {}),
        }));
        return response ? parseRegisterResponse(response) : null;
    }

  async broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void> {
    const response = await this.send(this.controllerRequest('replica_update', filePath, identity, {
      update_b64: Buffer.from(update).toString('base64'),
    }));
    if (!response?.ok) {
      throw new Error(
        `controller rejected replica_update: ${response?.error ?? 'controller_socket_unavailable'}`,
      );
    }
  }

    async pushDocumentOps(filePath: string, lineage: string | null, deltaJson: string): Promise<boolean> {
        const payload = lineage == null ? deltaJson : JSON.stringify({ lineage, delta_json: deltaJson });
        return reliableSyncDocumentOpPush(this.projectRoot, filePath, payload);
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

    async projectState(
        filePath: string,
        identity: string,
        contentHash: string,
        diskPersisted: boolean,
    ): Promise<boolean> {
        const response = await this.send(this.controllerRequest('replica_projection', filePath, identity, {
            content_hash: contentHash,
            disk_persisted: diskPersisted,
        }));
        return !!(response?.ok && isRecord(response.data) && response.data.projected === true);
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
    private clientId = 0;
    private pushedVersion: Uint8Array | null = null;
    private lineage: string | null = null;
    private readonly ownership = new MergeOwnershipStateChart();

    constructor(
        private readonly filePath: string,
        private readonly identity: string,
        private readonly node: ReplicaNode,
        private readonly transport: ReplicaTransport,
        private readonly resumeState: ReplicaResumeState | null = null,
    ) {}

    get currentClientId(): number {
        return this.clientId;
    }

    get attached(): boolean {
        return this.ownership.editorAttached;
    }

    get ownershipPhase(): MergeOwnershipPhase {
        return this.ownership.phase;
    }

    async register(): Promise<boolean> {
        const ack = await this.transport.register(
            this.filePath,
            this.identity,
            this.resumeState?.stateVector,
        );
        if (!ack) return false;
        this.clientId = ack.clientId;
        this.lineage = ack.lineage ?? null;
        const incremental = ack.bootstrapKind === 'delta';
        if (incremental && (!this.resumeState || !ack.canonicalStateVector)) {
            await this.transport.deregister(this.filePath, this.identity);
            return false;
        }
        const initialState = incremental ? this.resumeState?.encodedState : ack.bootstrap;
        if (!this.node.open(ack.clientId, initialState)) {
            await this.transport.deregister(this.filePath, this.identity);
            return false;
        }
        if (
            incremental &&
            ack.bootstrap &&
            ack.bootstrap.byteLength > 0 &&
            !this.node.applyUpdate(ack.clientId, ack.bootstrap)
        ) {
            this.node.close(ack.clientId);
            await this.transport.deregister(this.filePath, this.identity);
            return false;
        }
        if (!this.ownership.send('editor_attached')) {
            throw new Error(
                `merge-ownership chart rejected editor attach from ${this.ownership.phase}`,
            );
        }
        if (!this.ownership.send('editor_buffer_observed')) {
            throw new Error(
                `merge-ownership chart rejected buffer ownership from ${this.ownership.phase}`,
            );
        }
        this.pushedVersion = incremental
            ? ack.canonicalStateVector ?? new Uint8Array()
            : this.node.stateVector?.() ?? new Uint8Array();
        if (incremental) await this.publishIncremental();
        return true;
    }

    async forwardLocalDelta(offset: number, deleteLen: number, insert: string): Promise<void> {
        if (!this.attached) return;
        if (!this.node.applyLocal(this.clientId, offset, deleteLen, insert)) return;
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

    captureResumeState(): ReplicaResumeState | null {
        if (!this.attached) return null;
        const encodedState = this.node.encodeState();
        const stateVector = this.node.stateVector?.();
        if (!encodedState || !stateVector) return null;
        return {
            encodedState: encodedState.slice(),
            stateVector: stateVector.slice(),
        };
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

    projectVisibleState(text: string, diskPersisted = false): Promise<boolean> {
        if (!this.attached || !this.transport.projectState) return Promise.resolve(false);
        return this.transport.projectState(
            this.filePath,
            this.identity,
            sha256(text),
            diskPersisted,
        );
    }

    async deregister(): Promise<void> {
        if (!this.attached) return;
        await this.transport.deregister(this.filePath, this.identity);
        this.node.close(this.clientId);
        this.pushedVersion = null;
        this.lineage = null;
        if (!this.ownership.send('editor_detached')) {
            throw new Error(
                `merge-ownership chart rejected editor detach from ${this.ownership.phase}`,
            );
        }
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
    observeProjection?: (filePath: string) => void;
    normalizeTemplateStructure?: (text: string) => string | null;
    /**
     * `#ctrlkillreregister` Tier 3 — ask the controller which of this editor's
     * registrations it holds no replica for. Returns the raw FFI JSON, or null when
     * the question could not be asked. Injectable so the decision is testable
     * without a live controller; defaults to the native export.
     */
    peerReplicasMissing?: (pid: number, heldDocumentHashes: readonly string[]) => string | null;
    /** This editor process's pid; defaults to `process.pid`. */
    pid?: number;
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
    private readonly replicaRetryTimers = new Map<string, ReturnType<typeof setTimeout>>();
    private readonly replicaRetryFailureCounts = new Map<string, number>();
    private readonly pendingLocalEdits = new Map<string, number>();
    private readonly nonOperatorProjectionEpochs = new Map<string, number>();
    private readonly drainRequestedPaths = new Set<string>();
    private drainAllRequested = false;
    private drainQueued = false;
    private drainTimer: ReturnType<typeof setTimeout> | undefined;
    private refreshConnectionEpoch = 0;
    // `#ctrlkillreregister` Tier 3: transport loss is reported per document, but a
    // dead controller strands every document at once. One pull answers for all of
    // them, so the second and third file to notice must not each start their own.
    private lastPeerReplicaPullAtMs = 0;
    private disposed = false;

    constructor(private readonly options: CrdtReplicaManagerOptions) {
        this.logger = options.logger ?? noopLogger;
        this.transport = options.transport ?? new ControllerSocketReplicaTransport(options.projectRoot, this.logger);
        this.nodeFactory = options.nodeFactory ?? (() => new NativeReplicaNode(options.projectRoot));
    }

    start(): void {
        this.disposed = false;
        const attached: Promise<unknown>[] = [];
        for (const doc of this.options.listDocuments()) {
            this.seedDocument(doc.filePath, doc.text);
            attached.push(this.attachDocument(doc.filePath));
        }
        // `#ctrlkillreregister` Tier 3 safety net, AFTER the normal attach pass:
        // an editor whose registrations survived in the durable liveness plane can be
        // stranded in a controller that no longer holds their replicas, and a plain
        // attach of the currently-open tabs does not necessarily cover them. Running
        // it after the attaches means a healthy document is already registered by
        // then, so the controller reports nothing and no live baseline is rebuilt.
        void Promise.allSettled(attached).then(() => this.pullMissingReplicas('activation'));
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
        this.applyingRemote.clear();
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

    /**
     * `#ctrlkillreregister` Tier 3 — ask the controller which of this editor's
     * registrations it holds no replica for, and rebuild exactly those.
     *
     * `held` is deliberately EMPTY. This editor's own forwarder map is the wrong
     * evidence: after a controller kill the forwarders still look live here, and
     * passing them as held would suppress precisely the documents that need repair.
     * The controller subtracts what its process-local hub can actually serve, which is
     * the only fact that separates "registered" from "registered and backed".
     *
     * A null answer means the question could not be asked (old cdylib, controller
     * unreachable). There is no blind-sweep fallback here on purpose: unlike the
     * JetBrains startup path this replaces nothing, so the existing per-document
     * attach and retry paths remain the fallback and a redundant forced re-register
     * of healthy replicas would be strictly worse than doing nothing.
     */
    async pullMissingReplicas(reason: string): Promise<void> {
        if (this.disposed) return;
        const nowMs = Date.now();
        if (
            this.lastPeerReplicaPullAtMs > 0 &&
            nowMs - this.lastPeerReplicaPullAtMs < PEER_REPLICA_PULL_MIN_INTERVAL_MS
        ) {
            this.logger.debug(`[crdt-replica] coalesced peer replica pull; reason=${reason}`);
            return;
        }
        this.lastPeerReplicaPullAtMs = nowMs;
        const pid = this.options.pid ?? process.pid;
        const ask = this.options.peerReplicasMissing ??
            ((peerPid: number, held: readonly string[]) =>
                peerReplicasMissing(this.options.projectRoot, peerPid, held));
        const paths = peerReplicaRebuildPaths(ask(pid, []), pid);
        if (paths === null) {
            this.logger.debug(
                `[crdt-replica] peer replica pull unavailable; reason=${reason} ` +
                    'recovery=controller_tier1_fan_out',
            );
            return;
        }
        if (paths.length === 0) {
            this.logger.debug(`[crdt-replica] peer replica pull found nothing to rebuild; reason=${reason}`);
            return;
        }
        this.logger.warn(
            `[crdt-replica] peer replica pull names ${paths.length} stranded registration(s); reason=${reason}`,
        );
        for (const filePath of paths) {
            await this.attachDocument(filePath, undefined, true);
        }
    }

    async attachDocument(filePath: string, text?: string, forceRefresh = false): Promise<boolean> {
        const registrationText = text ?? this.currentEditorText(filePath) ?? this.shadows.get(filePath);
        if (forceRefresh) {
            if (this.hasPendingLocal(filePath)) return false;
            const staleForwarder = this.forwarders.get(filePath);
            if (staleForwarder) {
                this.forwarders.delete(filePath);
                await staleForwarder.deregister();
            }
        }
        if (registrationText !== undefined) this.seedDocument(filePath, registrationText);
        const forwarder = await this.forwarderFor(filePath);
        if (forwarder) {
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
        this.clearReplicaRetryBackoff(filePath);
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
                    'recovery=lazy-controller-canonical-projection',
                );
                this.scheduleReplicaRetry(filePath, 'local-delta-lazy-canonical-projection');
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
                this.logger.warn(
                    `[crdt-replica] replace delivery retained while the live editor diverges for ${filePath}; ` +
                    `editor_hash=${sha256(current)} canonical_hash=${sha256(canonical)}`,
                );
                this.scheduleReplicaRetry(filePath, 'replace-delivery-lazy-canonical-projection');
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
            if (!(await this.editorReplicaBaselineMatches(filePath, forwarder, expectedText, updates))) continue;
            const peerUpdates: ReplicaRemoteUpdate[] = [];
            let converged: string | null = null;
            for (const update of updates) {
                if (this.hasPendingLocal(filePath)) break;
                if (!shouldApplyRemoteUpdate(update, forwarder.currentClientId)) {
                    const visibleText = this.currentEditorText(filePath) ?? this.shadows.get(filePath);
                    if (visibleText != null) {
                        await forwarder.projectVisibleState(visibleText);
                    }
                    continue;
                }
                converged = forwarder.applyRemoteUpdate(update.update);
                if (converged == null) break;
                peerUpdates.push(update);
            }
            if (converged == null || peerUpdates.length === 0 || this.hasPendingLocal(filePath)) continue;

            await this.projectConvergedRemoteUpdates(
                filePath,
                expectedText,
                converged,
                forwarder,
                peerUpdates,
            );
        }
    }

    private async projectConvergedRemoteUpdates(
        filePath: string,
        expectedText: string,
        converged: string,
        forwarder: CrdtReplicaForwarder,
        updates: readonly ReplicaRemoteUpdate[],
    ): Promise<boolean> {
        const remoteState = this.templateStructureState(converged);
        if (remoteState !== 'exact') {
            await this.recoverRejectedRemoteCanonical(
                filePath,
                expectedText,
                converged,
                forwarder,
                remoteState,
            );
            return false;
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
            if (!projected || visibleText !== converged) return false;
            this.shadows.set(filePath, converged);
            await forwarder.projectVisibleState(converged);
            this.options.observeProjection?.(filePath);
            return true;
        } finally {
            this.applyingRemote.delete(filePath);
        }
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
        _staleForwarder: CrdtReplicaForwarder,
        remoteState: TemplateStructureProjectionState,
    ): Promise<boolean> {
        this.logger.warn(
            `[crdt-replica] rejected malformed remote projection for ${filePath}; ` +
            `remote_state=${remoteState} expected_hash=${sha256(expectedText)} ` +
            `remote_hash=${sha256(remoteText)} recovery=lazy-controller-canonical-projection`,
        );
        this.scheduleReplicaRetry(filePath, 'template-guard-lazy-canonical-projection');
        return false;
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
        timer.unref?.();
        this.replicaRetryTimers.set(filePath, timer);
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
            // `#ctrlkillreregister` Tier 3: this file recovered by noticing its own
            // loss, but a controller that lost its hub stranded every registration
            // this editor holds — including documents nothing is currently draining,
            // which would otherwise wait for an operator to touch them. Ask once,
            // about ourselves, and repair the rest now.
            await this.pullMissingReplicas('controller-transport-recovered');
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
            staleForwarder.captureResumeState(),
        );
        if (!(await replacement.register())) return null;
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
        await staleForwarder.deregister();
        this.logger.debug(
            `[crdt-replica] atomically replaced cached forwarder for ${filePath}`,
        );
        return replacement;
    }

    private async editorReplicaBaselineMatches(
        filePath: string,
        forwarder: CrdtReplicaForwarder,
        expectedText: string,
        updates: readonly ReplicaRemoteUpdate[],
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
        const editorHash = sha256(editorText);
        const replicaHash = replicaText == null ? null : sha256(replicaText);
        const editorRemoteGeneration = matchingRemoteTargetGeneration(updates, editorHash);
        const replicaRemoteGeneration = matchingRemoteTargetGeneration(updates, replicaHash);
        const decision = replicaBaselineDecision(
            editorState,
            editorText === expectedText,
            replicaText === expectedText,
            replicaText === editorText,
            editorRemoteGeneration != null,
            replicaRemoteGeneration != null,
            false,
        );
        if (decision === 'apply-remote' || decision === 'apply-remote-repair') {
            if (decision === 'apply-remote-repair') {
                this.logger.warn(
                    `[crdt-replica] applying a structurally exact remote correction over the exact expected editor baseline for ${filePath}: ` +
                    `editor_state=${editorState} editor_hash=${sha256(editorText)} expected_hash=${sha256(expectedText)}`,
                );
            }
            return true;
        }
        if (decision === 'project-remote-target' && editorRemoteGeneration != null) {
            this.shadows.set(filePath, editorText);
            await forwarder.projectVisibleState(editorText);
            this.options.observeProjection?.(filePath);
            this.logger.debug(
                `[crdt-replica] projected an already-visible remote target for ${filePath}: ` +
                `editor_hash=${editorHash} generation=${editorRemoteGeneration}`,
            );
            return false;
        }
        if (
            decision === 'replay-remote-target'
            && replicaText != null
            && replicaRemoteGeneration != null
        ) {
            const replayedUpdates = updates.filter(
                (update) => update.generation <= replicaRemoteGeneration,
            );
            this.logger.debug(
                `[crdt-replica] replaying a native remote target over its exact editor baseline for ${filePath}: ` +
                `editor_hash=${editorHash} replica_hash=${replicaHash} generation=${replicaRemoteGeneration}`,
            );
            await this.projectConvergedRemoteUpdates(
                filePath,
                expectedText,
                replicaText,
                forwarder,
                replayedUpdates,
            );
            return false;
        }
        if (decision === 'realign-shadow') {
            this.logger.warn(
                `[crdt-replica] incoming update deferred after shadow realignment for ${filePath}: ` +
                `editor_hash=${editorHash} expected_hash=${sha256(expectedText)} ` +
                `replica_hash=${replicaHash ?? 'missing'}`,
            );
            this.shadows.set(filePath, editorText);
            this.requestRemoteDrain(filePath);
            return false;
        }
        this.logger.warn(
            `[crdt-replica] incoming update retained for lazy canonical projection because baselines diverged for ${filePath}: ` +
            `editor_state=${editorState} editor_hash=${editorHash} expected_hash=${sha256(expectedText)} ` +
            `replica_hash=${replicaHash ?? 'missing'}`,
        );
        this.scheduleReplicaRetry(filePath, 'baseline-diverged-lazy-canonical-projection');
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
