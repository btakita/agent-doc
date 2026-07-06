import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    CrdtReplicaManager,
    parsePullResponse,
    parseRegisterResponse,
    shouldApplyRemoteUpdate,
    type ReplicaNode,
    type ReplicaRemoteUpdate,
    type ReplicaTransport,
    utf16RangeToCodePoints,
} from './crdtReplica';

class FakeNode implements ReplicaNode {
    opened: number | null = null;
    locals: Array<{ clientId: number; offset: number; deleteLen: number; insert: string }> = [];
    updates: Uint8Array[] = [];
    closed: number[] = [];
    state = Buffer.from([1, 2, 3]);
    remoteText = 'remote text';

    constructor(private current = 'remote text') {}

    open(clientId: number): boolean {
        this.opened = clientId;
        return true;
    }

    applyLocal(clientId: number, offset: number, deleteLen: number, insert: string): boolean {
        this.locals.push({ clientId, offset, deleteLen, insert });
        const chars = Array.from(this.current);
        this.current = `${chars.slice(0, offset).join('')}${insert}${chars.slice(offset + deleteLen).join('')}`;
        return true;
    }

    applyUpdate(_clientId: number, update: Uint8Array): boolean {
        this.updates.push(Buffer.from(update));
        this.current = this.remoteText;
        return true;
    }

    encodeState(): Uint8Array | null {
        return this.state;
    }

    text(): string | null {
        return this.current;
    }

    close(clientId?: number): void {
        if (clientId != null) this.closed.push(clientId);
    }
}

class FakeTransport implements ReplicaTransport {
    broadcasts: Array<{ filePath: string; identity: string; update: Uint8Array }> = [];
    pending: ReplicaRemoteUpdate[] = [];
    acked: Array<{ patchId: string; generation: number }> = [];
    deregistered: string[] = [];
    broadcastGate: Promise<void> | undefined;
    registerCount = 0;

    async register(): Promise<{ clientId: number; bootstrap?: Uint8Array | null }> {
        this.registerCount += 1;
        return { clientId: 41 + this.registerCount, bootstrap: Buffer.from([9]) };
    }

    async broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void> {
        if (this.broadcastGate) await this.broadcastGate;
        this.broadcasts.push({ filePath, identity, update: Buffer.from(update) });
    }

    async pullUpdates(): Promise<ReplicaRemoteUpdate[]> {
        return this.pending;
    }

    async ackUpdate(
        _filePath: string,
        _identity: string,
        patchId: string,
        generation: number,
    ): Promise<boolean> {
        this.acked.push({ patchId, generation });
        this.pending = this.pending.filter((update) => update.patchId !== patchId);
        return true;
    }

    async deregister(filePath: string): Promise<void> {
        this.deregistered.push(filePath);
    }
}

describe('crdt replica manager', () => {
    it('applies peer remote updates but suppresses self echoes', () => {
        const peer: ReplicaRemoteUpdate = {
            patchId: 'crdt:1:42:1',
            origin: 1,
            target: 42,
            generation: 1,
            update: Buffer.from([1]),
        };
        const self: ReplicaRemoteUpdate = {
            patchId: 'crdt:42:42:2',
            origin: 42,
            target: 42,
            generation: 2,
            update: Buffer.from([2]),
        };

        assert.strictEqual(shouldApplyRemoteUpdate(peer, 42), true);
        assert.strictEqual(shouldApplyRemoteUpdate(self, 42), false);
    });

    it('converts VS Code UTF-16 ranges to CRDT codepoint units', () => {
        assert.deepStrictEqual(utf16RangeToCodePoints('a😀b', 1, 2), {
            offset: 1,
            deleteLen: 1,
        });
        assert.deepStrictEqual(utf16RangeToCodePoints('café', 4, 0), {
            offset: 4,
            deleteLen: 0,
        });
    });

    it('forwards a local editor delta through the registered replica', async () => {
        const node = new FakeNode('a😀b');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'a😀b';
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async () => true,
        });

        manager.seedDocument(filePath, 'a😀b');
        assert.strictEqual(await manager.attachDocument(filePath), true);
        editorText = 'ab';
        await manager.handleLocalChange(filePath, 'ab', [
            { rangeOffset: 1, rangeLength: 2, text: '' },
        ]);

        assert.deepStrictEqual(node.locals, [
            { clientId: 42, offset: 1, deleteLen: 1, insert: '' },
        ]);
        assert.strictEqual(transport.broadcasts.length, 1);
        assert.deepStrictEqual(Array.from(transport.broadcasts[0].update), [1, 2, 3]);
    });

    it('ACKs a pulled remote update only after the converged text is applied', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        const applied: Array<{ text: string; expectedText: string | undefined }> = [];
        transport.pending = [{
            patchId: 'crdt:1:2:3',
            origin: 1,
            target: 42,
            generation: 3,
            update: Buffer.from([7, 8]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async (_file, text, expectedText) => {
                applied.push({ text, expectedText });
                editorText = text;
                return true;
            },
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        await manager.drainRemoteUpdates();

        assert.deepStrictEqual(applied, [{ text: 'remote text', expectedText: 'base' }]);
        assert.deepStrictEqual(transport.acked, [{ patchId: 'crdt:1:2:3', generation: 3 }]);
    });

    it('passes expected editor text so stale CRDT remote targets are not ACKed over typing', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        transport.pending = [{
            patchId: 'crdt:1:42:8',
            origin: 1,
            target: 42,
            generation: 8,
            update: Buffer.from([8]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async (_file, targetText, expectedText) => {
                assert.strictEqual(expectedText, 'base');
                editorText = 'base typed';
                if (editorText !== expectedText) return false;
                editorText = targetText;
                return true;
            },
        });

        assert.strictEqual(await manager.attachDocument(filePath, editorText), true);
        await manager.drainRemoteUpdates();

        assert.strictEqual(editorText, 'base typed');
        assert.deepStrictEqual(transport.acked, []);
        manager.dispose();
    });

    it('ACKs self-echo remote updates without applying text', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const applied: string[] = [];
        transport.pending = [{
            patchId: 'crdt:42:42:5',
            origin: 42,
            target: 42,
            generation: 5,
            update: Buffer.from([7, 8]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => 'base',
            applyText: async (_file, text) => {
                applied.push(text);
                return true;
            },
        });

        assert.strictEqual(await manager.attachDocument('/work/plan.md', 'base'), true);
        await manager.drainRemoteUpdates();

        assert.deepStrictEqual(applied, []);
        assert.deepStrictEqual(node.updates, []);
        assert.deepStrictEqual(transport.acked, [{ patchId: 'crdt:42:42:5', generation: 5 }]);
    });

    it('forwards undo of an applied remote update as a local delta', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        transport.pending = [{
            patchId: 'crdt:1:42:6',
            origin: 1,
            target: 42,
            generation: 6,
            update: Buffer.from([7, 8]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async (_file, text) => {
                editorText = text;
                return true;
            },
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        await manager.drainRemoteUpdates();
        editorText = 'base';
        await manager.handleLocalChange(filePath, 'base', [
            { rangeOffset: 0, rangeLength: 'remote text'.length, text: 'base' },
        ]);

        assert.deepStrictEqual(node.locals, [
            { clientId: 42, offset: 0, deleteLen: 11, insert: 'base' },
        ]);
        assert.strictEqual(transport.broadcasts.length, 1);
    });

    it('does not ACK a pulled remote update when editor application fails', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        let editorText = 'base';
        transport.pending = [{
            patchId: 'crdt:1:2:4',
            origin: 1,
            target: 42,
            generation: 4,
            update: Buffer.from([9]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async () => false,
        });

        assert.strictEqual(await manager.attachDocument('/work/plan.md', 'base'), true);
        await manager.drainRemoteUpdates();

        assert.deepStrictEqual(transport.acked, []);
        manager.dispose();
    });

    it('defers remote apply while a local delta is still forwarding', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        const applied: string[] = [];
        let editorText = 'base';
        let releaseBroadcast!: () => void;
        transport.broadcastGate = new Promise<void>((resolve) => {
            releaseBroadcast = resolve;
        });
        transport.pending = [{
            patchId: 'crdt:1:42:7',
            origin: 1,
            target: 42,
            generation: 7,
            update: Buffer.from([7]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async (_file, text) => {
                applied.push(text);
                editorText = text;
                return true;
            },
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        editorText = 'base typed';
        const localForward = manager.handleLocalChangeDelta(filePath, [
            { rangeOffset: 4, rangeLength: 0, text: ' typed' },
        ]);

        await manager.drainRemoteUpdates();
        assert.deepStrictEqual(applied, []);
        assert.deepStrictEqual(transport.acked, []);

        releaseBroadcast();
        await localForward;
        await manager.drainRemoteUpdates();

        assert.deepStrictEqual(applied, ['remote text']);
        assert.deepStrictEqual(transport.acked, [{ patchId: 'crdt:1:42:7', generation: 7 }]);
    });

    it('force-refresh republishes editor text through the cached replica', async () => {
        const nodes: FakeNode[] = [];
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => {
                const node = new FakeNode();
                nodes.push(node);
                return node;
            },
            listDocuments: () => [],
            currentText: () => 'base updated',
            applyText: async () => true,
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        assert.strictEqual(await manager.attachDocument(filePath, 'base updated', true), true);

        assert.strictEqual(transport.registerCount, 1);
        assert.deepStrictEqual(transport.deregistered, []);
        assert.deepStrictEqual(nodes.map((node) => node.opened), [42]);
        assert.deepStrictEqual(nodes[0].locals.map((local) => local.insert), ['base', 'base updated']);
        assert.strictEqual(transport.broadcasts.length, 2);
    });
});

describe('crdt replica IPC response parsing', () => {
    it('parses register and pull responses with base64 update payloads', () => {
        const register = parseRegisterResponse({
            ok: true,
            data: {
                client_id: 42,
                bootstrap_b64: Buffer.from([1, 2]).toString('base64'),
            },
        });
        assert.deepStrictEqual(register && Array.from(register.bootstrap ?? []), [1, 2]);

        const pull = parsePullResponse({
            ok: true,
            data: {
                updates: [{
                    patch_id: 'crdt:1:42:5',
                    origin: 1,
                    target: 42,
                    generation: 5,
                    update_b64: Buffer.from([3, 4]).toString('base64'),
                }],
            },
        });
        assert.strictEqual(pull.length, 1);
        assert.deepStrictEqual(Array.from(pull[0].update), [3, 4]);
    });
});
