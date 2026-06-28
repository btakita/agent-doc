import * as fs from 'fs';
import * as net from 'net';
import * as path from 'path';
import { NativeReplicaNode } from './native';

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
    pullUpdates(filePath: string, identity: string): Promise<ReplicaRemoteUpdate[]>;
    ackUpdate(filePath: string, identity: string, patchId: string, generation: number): Promise<boolean>;
    deregister(filePath: string, identity: string): Promise<void>;
}

export interface ReplicaNode {
    open(clientId: number, initState?: Uint8Array | null): boolean;
    applyLocal(clientId: number, offset: number, deleteLen: number, insert: string): boolean;
    applyUpdate(clientId: number, update: Uint8Array): boolean;
    encodeState(): Uint8Array | null;
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

interface SupervisorResponse {
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

export function parseRegisterResponse(response: SupervisorResponse): ReplicaRegisterAck | null {
    if (!response.ok || !isRecord(response.data)) return null;
    const clientId = asNumber(response.data.client_id);
    if (clientId == null) return null;
    return {
        clientId,
        bootstrap: decodeBase64(response.data.bootstrap_b64),
    };
}

export function parsePullResponse(response: SupervisorResponse): ReplicaRemoteUpdate[] {
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

export class SupervisorSocketReplicaTransport implements ReplicaTransport {
    private cachedSocket: string | null = null;

    constructor(
        private readonly projectRoot: string,
        private readonly logger: ReplicaLogger = noopLogger,
    ) {}

    async register(filePath: string, identity: string): Promise<ReplicaRegisterAck | null> {
        const response = await this.send({
            method: 'replica_register',
            file: filePath,
            identity,
        });
        return response ? parseRegisterResponse(response) : null;
    }

    async broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void> {
        await this.send({
            method: 'replica_update',
            file: filePath,
            identity,
            update_b64: Buffer.from(update).toString('base64'),
        });
    }

    async pullUpdates(filePath: string, identity: string): Promise<ReplicaRemoteUpdate[]> {
        const response = await this.send({
            method: 'replica_pull',
            file: filePath,
            identity,
        });
        return response ? parsePullResponse(response) : [];
    }

    async ackUpdate(
        filePath: string,
        identity: string,
        patchId: string,
        generation: number,
    ): Promise<boolean> {
        const response = await this.send({
            method: 'replica_ack',
            file: filePath,
            identity,
            patch_id: patchId,
            generation,
        });
        return !!(response?.ok && isRecord(response.data) && response.data.acknowledged === true);
    }

    async deregister(filePath: string, identity: string): Promise<void> {
        await this.send({
            method: 'replica_deregister',
            file: filePath,
            identity,
        });
    }

    private async send(request: Record<string, unknown>): Promise<SupervisorResponse | null> {
        for (const socketPath of this.socketCandidates()) {
            try {
                const response = await this.sendToSocket(socketPath, request);
                this.cachedSocket = socketPath;
                return response;
            } catch (e: any) {
                if (socketPath === this.cachedSocket) this.cachedSocket = null;
                this.logger.debug(`[crdt-replica] supervisor socket ${socketPath} unavailable: ${e?.message ?? e}`);
            }
        }
        return null;
    }

    private socketCandidates(): string[] {
        const dir = path.join(this.projectRoot, '.agent-doc', 'supervisor');
        let discovered: string[] = [];
        try {
            discovered = fs.readdirSync(dir)
                .filter((name) => name.endsWith('.sock'))
                .map((name) => path.join(dir, name))
                .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
        } catch {
            discovered = [];
        }
        const cached = this.cachedSocket && fs.existsSync(this.cachedSocket) ? this.cachedSocket : null;
        return cached ? [cached, ...discovered.filter((candidate) => candidate !== cached)] : discovered;
    }

    private sendToSocket(socketPath: string, request: Record<string, unknown>): Promise<SupervisorResponse> {
        return new Promise((resolve, reject) => {
            const socket = net.createConnection(socketPath);
            let buffer = '';
            let settled = false;
            let timeout: ReturnType<typeof setTimeout> | undefined;

            const finish = (err: Error | null, response?: SupervisorResponse) => {
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
                    finish(null, JSON.parse(line) as SupervisorResponse);
                } catch (e: any) {
                    finish(new Error(`invalid supervisor response: ${e?.message ?? e}`));
                }
            };

            timeout = setTimeout(() => finish(new Error('timeout waiting for supervisor response')), 1_000);
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
        return true;
    }

    async forwardLocalDelta(offset: number, deleteLen: number, insert: string): Promise<void> {
        if (!this.attached) return;
        if (!this.node.applyLocal(this.clientId, offset, deleteLen, insert)) return;
        const update = this.node.encodeState();
        if (!update) return;
        await this.transport.broadcastUpdate(this.filePath, this.identity, update);
    }

    applyRemoteUpdate(update: Uint8Array): string | null {
        if (!this.attached) return null;
        if (!this.node.applyUpdate(this.clientId, update)) return null;
        return this.node.text();
    }

    pullRemoteUpdates(): Promise<ReplicaRemoteUpdate[]> {
        if (!this.attached) return Promise.resolve([]);
        return this.transport.pullUpdates(this.filePath, this.identity);
    }

    ackRemoteUpdate(update: ReplicaRemoteUpdate): Promise<boolean> {
        if (!this.attached) return Promise.resolve(false);
        return this.transport.ackUpdate(this.filePath, this.identity, update.patchId, update.generation);
    }

    async deregister(): Promise<void> {
        if (!this.attached) return;
        await this.transport.deregister(this.filePath, this.identity);
        this.node.close(this.clientId);
        this.attached = false;
    }
}

export interface CrdtReplicaManagerOptions {
    projectRoot: string;
    identity: string;
    transport?: ReplicaTransport;
    nodeFactory?: () => ReplicaNode;
    listDocuments: () => ReplicaDocumentSnapshot[];
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
    private readonly pendingLocalEdits = new Map<string, number>();
    private pollTimer: ReturnType<typeof setInterval> | undefined;

    constructor(private readonly options: CrdtReplicaManagerOptions) {
        this.logger = options.logger ?? noopLogger;
        this.transport = options.transport ?? new SupervisorSocketReplicaTransport(options.projectRoot, this.logger);
        this.nodeFactory = options.nodeFactory ?? (() => new NativeReplicaNode(options.projectRoot));
    }

    start(): void {
        for (const doc of this.options.listDocuments()) {
            this.seedDocument(doc.filePath, doc.text);
            void this.attachDocument(doc.filePath);
        }
        this.pollTimer = setInterval(() => {
            void this.pollRemoteUpdates();
        }, 250);
    }

    dispose(): void {
        if (this.pollTimer) clearInterval(this.pollTimer);
        this.pollTimer = undefined;
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

    async attachDocument(filePath: string, text?: string): Promise<boolean> {
        if (text !== undefined) this.seedDocument(filePath, text);
        return (await this.forwarderFor(filePath)) != null;
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
        }
    }

    async pollRemoteUpdates(): Promise<void> {
        for (const [filePath, forwarder] of Array.from(this.forwarders.entries())) {
            if (this.hasPendingLocal(filePath)) continue;
            const updates = await forwarder.pullRemoteUpdates();
            if (this.hasPendingLocal(filePath)) continue;
            for (const update of updates) {
                if (this.hasPendingLocal(filePath)) break;
                if (!shouldApplyRemoteUpdate(update, forwarder.currentClientId)) {
                    await forwarder.ackRemoteUpdate(update);
                    continue;
                }
                const expectedText = this.shadows.get(filePath);
                if (expectedText === undefined) continue;
                const converged = forwarder.applyRemoteUpdate(update.update);
                if (converged == null) continue;
                if (this.hasPendingLocal(filePath)) continue;
                this.applyingRemote.add(filePath);
                try {
                    const applied = await this.options.applyText(filePath, converged, expectedText);
                    if (applied) {
                        this.shadows.set(filePath, converged);
                        await forwarder.ackRemoteUpdate(update);
                    }
                } finally {
                    this.applyingRemote.delete(filePath);
                }
            }
        }
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
