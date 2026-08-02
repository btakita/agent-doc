import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
  CrdtReplicaForwarder,
  CrdtReplicaManager,
  coalescedReplicaTextChange,
  localReplicaBaselineDecision,
    parsePullResponse,
    parseRegisterResponse,
    matchingRemoteTargetGeneration,
    replicaBaselineDecision,
    remoteTemplateProjectionDecision,
    shouldApplyRemoteUpdate,
    shouldForwardLocalDelta,
    templateStructureProjectionState,
    type ReplicaNode,
    type ReplicaPullDelivery,
    type ReplicaRemoteUpdate,
    type ReplicaTransport,
    utf16RangeToCodePoints,
} from './crdtReplica.js';

class FakeNode implements ReplicaNode {
    opened: number | null = null;
    openedState: Uint8Array | null | undefined;
    locals: Array<{ clientId: number; offset: number; deleteLen: number; insert: string }> = [];
    updates: Uint8Array[] = [];
    closed: number[] = [];
    state = Buffer.from([1, 2, 3]);
    remoteText = 'remote text';

    constructor(private current = 'remote text') {}

    open(clientId: number, initState?: Uint8Array | null): boolean {
        this.opened = clientId;
        this.openedState = initState == null ? initState : Buffer.from(initState);
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
    projected: Array<{ contentHash: string; diskPersisted: boolean }> = [];
    deregistered: string[] = [];
    broadcastGate: Promise<void> | undefined;
    registerCount = 0;
    pullCount = 0;
    unavailablePulls = 0;
  projectionFailures = 0;
  registerFailures = 0;

    async register(
        _filePath: string,
        _identity: string,
        _stateVector?: Uint8Array | null,
  ): Promise<{ clientId: number; bootstrap?: Uint8Array | null }> {
    this.registerCount += 1;
    if (this.registerFailures > 0) {
      this.registerFailures -= 1;
      throw new Error('register unavailable');
    }
    return { clientId: 41 + this.registerCount, bootstrap: Buffer.from([9]) };
    }

    async broadcastUpdate(filePath: string, identity: string, update: Uint8Array): Promise<void> {
        if (this.broadcastGate) await this.broadcastGate;
        this.broadcasts.push({ filePath, identity, update: Buffer.from(update) });
    }

    async pullUpdates(): Promise<ReplicaRemoteUpdate[]> {
        this.pullCount += 1;
        return this.pending;
    }

    async pullDelivery(): Promise<ReplicaPullDelivery> {
        this.pullCount += 1;
        if (this.unavailablePulls > 0) {
            this.unavailablePulls -= 1;
            return { kind: 'unavailable', reason: 'controller_socket_unavailable' };
        }
        return { kind: 'deltas', updates: this.pending };
    }

    async projectState(
        _filePath: string,
        _identity: string,
        contentHash: string,
        diskPersisted: boolean,
    ): Promise<boolean> {
        if (this.projectionFailures > 0) {
            this.projectionFailures -= 1;
            return false;
    }
    this.projected.push({ contentHash, diskPersisted });
    this.pending = this.pending.filter(
      (update) =>
        update.expectedContentHash != null
        && update.expectedContentHash !== contentHash,
    );
        return true;
    }

    async deregister(filePath: string): Promise<void> {
        this.deregistered.push(filePath);
    }
}

describe('crdt replica manager', () => {
    it('replacement registration resumes local state and applies only the canonical delta', async () => {
        const node = new FakeNode('retained');
        const transport = new FakeTransport();
        let registeredStateVector: Uint8Array | null | undefined;
        transport.register = async (
            _filePath: string,
            _identity: string,
            stateVector?: Uint8Array | null,
        ) => {
            registeredStateVector = stateVector;
            return {
                clientId: 77,
                bootstrap: Buffer.from([8, 9]),
                bootstrapKind: 'delta' as const,
                canonicalStateVector: Buffer.from([4, 5]),
            };
        };
        const resumeState = {
            encodedState: Buffer.from([1, 2, 3]),
            stateVector: Buffer.from([6, 7]),
        };
        const forwarder = new CrdtReplicaForwarder(
            '/work/plan.md',
            'vscode-test:refresh-1',
            node,
            transport,
            resumeState,
        );

        assert.strictEqual(await forwarder.register(), true);
        assert.strictEqual(forwarder.ownershipPhase, 'editor_owns_buffer');
        assert.deepStrictEqual(Array.from(node.openedState ?? []), [1, 2, 3]);
        assert.deepStrictEqual(Array.from(registeredStateVector ?? []), [6, 7]);
        assert.deepStrictEqual(Array.from(node.updates[0] ?? []), [8, 9]);
        assert.strictEqual(
            transport.broadcasts.length,
            1,
            'the retained local suffix must be published from the canonical frontier',
        );
        await forwarder.deregister();
        assert.strictEqual(forwarder.ownershipPhase, 'detached');
    });

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

  it('types the stale local baseline as a canonical rebootstrap transition', () => {
    assert.strictEqual(localReplicaBaselineDecision('base', 'base'), 'forward-local');
    assert.strictEqual(
      localReplicaBaselineDecision('stale', 'base'),
      'rebootstrap-canonical-then-forward',
    );
    assert.deepStrictEqual(coalescedReplicaTextChange('a😀b', 'aZb'), {
      offset: 1,
      deleteLen: 1,
      insert: 'Z',
      resultingText: 'aZb',
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

  it('rebases an unsaved queue edit onto exact controller canonical without whole-editor adoption', async () => {
    const staleNode = new FakeNode('base');
    const replacementNode = new FakeNode('base');
        const nodes = [staleNode, replacementNode];
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => nodes.shift()!,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async () => true,
        });

        manager.seedDocument(filePath, 'base');
        assert.strictEqual(await manager.attachDocument(filePath), true);
        staleNode.remoteText = 'stale native text';
        staleNode.applyUpdate(42, Buffer.from([7]));
        editorText = 'base\n- queue item';
    await manager.handleLocalChangeDelta(filePath, [
      { rangeOffset: 4, rangeLength: 0, text: '\n- queue item' },
    ]);

    assert.deepStrictEqual(staleNode.locals, []);
    assert.deepStrictEqual(replacementNode.locals, [
      { clientId: 43, offset: 4, deleteLen: 0, insert: '\n- queue item' },
    ]);
    assert.strictEqual(transport.registerCount, 2);
    assert.strictEqual(transport.deregistered.length, 1);
    assert.strictEqual(transport.projected.length, 1);
    assert.strictEqual(editorText.match(/- queue item/g)?.length, 1);
    manager.dispose();
  });

  it('retains successive queue edits while a stale native baseline awaits canonical projection', async () => {
        const firstEdit = 'base\n- queue item';
        const latestEdit = `${firstEdit}\n- later item`;
    const staleNode = new FakeNode('base');
    const failedReplacementNode = new FakeNode('base');
    const replacementNode = new FakeNode('base');
    const nodes = [staleNode, failedReplacementNode, replacementNode];
    const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
      nodeFactory: () => nodes.shift()!,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async () => true,
        });

        manager.seedDocument(filePath, editorText);
        assert.strictEqual(await manager.attachDocument(filePath), true);
    staleNode.remoteText = 'stale native text';
    staleNode.applyUpdate(42, Buffer.from([7]));
    transport.registerFailures = 1;
    editorText = firstEdit;
        await manager.handleLocalChangeDelta(filePath, [
            { rangeOffset: 4, rangeLength: 0, text: '\n- queue item' },
        ]);
        editorText = latestEdit;
        await manager.handleLocalChangeDelta(filePath, [
            { rangeOffset: firstEdit.length, rangeLength: 0, text: '\n- later item' },
        ]);

    assert.strictEqual(transport.registerCount, 3);
    assert.deepStrictEqual(replacementNode.locals, [
      {
        clientId: 44,
        offset: 4,
        deleteLen: 0,
        insert: '\n- queue item\n- later item',
      },
    ]);
    assert.strictEqual(editorText.match(/- queue item/g)?.length, 1);
        assert.strictEqual(editorText.match(/- later item/g)?.length, 1);
        manager.dispose();
    });

    it('projects visible state only after the converged text is applied', async () => {
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
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
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
        assert.deepStrictEqual(transport.projected, [{
            contentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
            diskPersisted: false,
        }]);
    });

    it('retained canonical projection blocks stale whole editor adoption', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        let projectionStarted!: () => void;
        let releaseProjection!: () => void;
        const started = new Promise<void>((resolve) => {
            projectionStarted = resolve;
        });
        const projectionGate = new Promise<void>((resolve) => {
            releaseProjection = resolve;
        });
        transport.pending = [{
            patchId: 'crdt:1:42:4',
            origin: 1,
            target: 42,
            generation: 4,
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
            update: Buffer.from([9]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async (_file, text) => {
                projectionStarted();
                await projectionGate;
                editorText = text;
                return true;
            },
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        const drain = manager.drainRemoteUpdates();
        await started;
        manager.requestRemoteDrain(filePath);
        assert.strictEqual(
            transport.registerCount,
            1,
            'an unsynced-edit flag cannot replace the controller-owned projection',
        );
        releaseProjection();
        await drain;
        manager.dispose();
    });

    it('reprojects visible state after transient projection transport failure', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        transport.projectionFailures = 2;
        transport.pending = [{
            patchId: 'crdt:1:42:13',
            origin: 1,
            target: 42,
            generation: 13,
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
            update: Buffer.from([13]),
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
        assert.strictEqual(transport.pullCount, 1);
        assert.strictEqual(node.updates.length, 1);

        await manager.drainRemoteUpdates();
        assert.strictEqual(transport.pullCount, 2);
        assert.strictEqual(node.updates.length, 1);

        await manager.drainRemoteUpdates();
        assert.strictEqual(transport.pullCount, 3);
        assert.strictEqual(node.updates.length, 1);
        manager.dispose();
    });

    it('rebootstraps a stale replica before acknowledging an exact visible target', async () => {
        const stale = new FakeNode('base');
        const replacement = new FakeNode('remote text');
        const nodes = [stale, replacement];
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        transport.pending = [{
            patchId: 'crdt:1:42:21',
            origin: 1,
            target: 42,
            generation: 21,
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
            update: Buffer.from([21]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => nodes.shift() ?? new FakeNode('remote text'),
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async () => {
                assert.fail('the exact visible target must not be written into the editor again');
            },
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        editorText = 'remote text';
        await manager.drainRemoteUpdates();

        assert.strictEqual(transport.registerCount, 2);
        assert.strictEqual(stale.updates.length, 0);
        assert.strictEqual(stale.closed.length, 1);
        assert.strictEqual(transport.projected.length, 1);
        assert.strictEqual(transport.pending.length, 0);
        manager.dispose();
    });

    it('rejects visible-target acknowledgement when the editor races canonical rebootstrap', async () => {
        const stale = new FakeNode('base');
        const replacement = new FakeNode('remote text');
        const nodes = [stale, replacement];
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        const register = transport.register.bind(transport);
        transport.register = async (...args) => {
            const ack = await register(...args);
            if (transport.registerCount === 2) editorText = 'operator edit';
            return ack;
        };
        transport.pending = [{
            patchId: 'crdt:1:42:22',
            origin: 1,
            target: 42,
            generation: 22,
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
            update: Buffer.from([22]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => nodes.shift() ?? new FakeNode('remote text'),
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async () => false,
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        editorText = 'remote text';
        await manager.drainRemoteUpdates();

        assert.strictEqual(transport.registerCount, 2);
        assert.strictEqual(stale.closed.length, 0);
        assert.strictEqual(replacement.closed.length, 1);
        assert.strictEqual(transport.projected.length, 0);
        assert.strictEqual(transport.pending.length, 1);
        manager.dispose();
    });

    it('rejects malformed canonical without adopting the editor baseline', async () => {
        const first = new FakeNode('base');
        first.remoteText = 'INVALID_CANONICAL';
        const replacement = new FakeNode('base');
        const nodes = [first, replacement];
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        transport.pending = [{
            patchId: 'crdt:1:42:14',
            origin: 1,
            target: 42,
            generation: 14,
            update: Buffer.from([14]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => nodes.shift() ?? new FakeNode('base'),
            listDocuments: () => [],
            currentText: () => 'base',
            applyText: async () => {
                assert.fail('invalid remote canonical must not reach editor projection');
            },
            normalizeTemplateStructure: (text) => text === 'INVALID_CANONICAL' ? null : text,
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        await manager.drainRemoteUpdates();

        assert.strictEqual(transport.registerCount, 1);
        assert.deepStrictEqual(transport.deregistered, []);
        manager.dispose();
    });

    it('re-registers an open replica after controller transport loss', async () => {
        const transport = new FakeTransport();
        transport.unavailablePulls = 1;
        const filePath = '/work/plan.md';
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => new FakeNode('base'),
            listDocuments: () => [],
            currentText: () => 'base',
            applyText: async () => true,
        });

        assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
        await manager.drainRemoteUpdates();

        assert.strictEqual(transport.registerCount, 2);
        assert.deepStrictEqual(transport.deregistered, [filePath]);
        manager.dispose();
    });

    it('uses the same typed template and baseline decisions as JetBrains', () => {
        assert.strictEqual(templateStructureProjectionState('raw', 'raw'), 'exact');
        assert.strictEqual(templateStructureProjectionState('raw', 'fixed'), 'repair-required');
        assert.strictEqual(templateStructureProjectionState('raw', null), 'invalid');
        assert.strictEqual(
            remoteTemplateProjectionDecision('invalid', 'exact', true, false),
            'retry-fail-closed',
        );
        assert.strictEqual(
            remoteTemplateProjectionDecision('invalid', 'exact', false, false),
            'retry-fail-closed',
        );
        assert.strictEqual(
            remoteTemplateProjectionDecision('repair-required', 'exact', true, true),
            'retry-fail-closed',
        );
        assert.strictEqual(
            replicaBaselineDecision('exact', true, false, false, false, true, false),
            'replay-remote-target',
        );
        assert.strictEqual(
            matchingRemoteTargetGeneration([
                {
                    patchId: 'one',
                    origin: 1,
                    target: 2,
                    generation: 1,
                    expectedContentHash: 'first',
                    update: Buffer.from([1]),
                },
                {
                    patchId: 'two',
                    origin: 1,
                    target: 2,
                    generation: 2,
                    expectedContentHash: 'target',
                    update: Buffer.from([2]),
                },
            ], 'target'),
            2,
        );
        assert.strictEqual(
            replicaBaselineDecision('exact', false, false, true, true, true, false),
            'project-remote-target',
        );
        assert.strictEqual(
            replicaBaselineDecision('exact', true, false, false, true, false, false),
            'rebootstrap-visible-remote-target',
        );
        assert.strictEqual(
            replicaBaselineDecision('exact', true, false, false, true, false, true),
            'retry-fail-closed',
        );
        assert.strictEqual(
            replicaBaselineDecision('exact', false, false, true, false, false, false),
            'realign-shadow',
        );
        assert.strictEqual(
            replicaBaselineDecision('repair-required', true, false, false, false, false, false),
            'apply-remote-repair',
        );
        assert.strictEqual(
            replicaBaselineDecision('invalid', false, false, false, false, false, false),
            'retry-fail-closed',
        );
        assert.strictEqual(shouldForwardLocalDelta('before', 'before'), true);
        assert.strictEqual(shouldForwardLocalDelta('stale', 'before'), false);
    });

    it('reprojects CPC state over unproven buffer divergence before publishing state', async () => {
  const node = new FakeNode('base');
  const transport = new FakeTransport();
  const filePath = '/work/plan.md';
  let editorText = 'base';
  const projections: Array<{ text: string; expectedText: string | undefined }> = [];
        transport.pending = [{
            patchId: 'crdt:1:42:8',
            origin: 1,
            target: 42,
            generation: 8,
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
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
      projections.push({ text: targetText, expectedText });
      if (expectedText === 'base') editorText = 'stale cache projection';
      if (editorText !== expectedText) return false;
      editorText = targetText;
      return true;
            },
        });

  assert.strictEqual(await manager.attachDocument(filePath, editorText), true);
  await manager.drainRemoteUpdates();

  assert.strictEqual(editorText, 'remote text');
  assert.deepStrictEqual(projections, [
    { text: 'remote text', expectedText: 'base' },
    { text: 'remote text', expectedText: 'stale cache projection' },
  ]);
  assert.deepStrictEqual(transport.projected, [{
    contentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
    diskPersisted: false,
  }]);
        manager.dispose();
    });

    it('projects an already-applied native target without decoding the retained delta twice', async () => {
        const node = new FakeNode('base');
        const transport = new FakeTransport();
        const filePath = '/work/plan.md';
        let editorText = 'base';
        transport.pending = [{
            patchId: 'crdt:1:42:9',
            origin: 1,
            target: 42,
            generation: 9,
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
            update: Buffer.from([9]),
        }];
        const manager = new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            transport,
            nodeFactory: () => node,
            listDocuments: () => [],
            currentText: () => editorText,
            applyText: async (_file, text, expectedText) => {
                if (editorText !== expectedText) return false;
                editorText = text;
                return true;
            },
        });

        assert.strictEqual(await manager.attachDocument(filePath, editorText), true);
        assert.strictEqual(node.applyUpdate(42, Buffer.from([99])), true);
        node.updates = [];
        await manager.drainRemoteUpdates();

        assert.strictEqual(editorText, 'remote text');
        assert.deepStrictEqual(node.updates, []);
        assert.deepStrictEqual(transport.projected, [{
            contentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
            diskPersisted: false,
        }]);
        manager.dispose();
    });

    it('projects self-echo visible state without applying text', async () => {
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
        assert.deepStrictEqual(transport.projected, [{
            contentHash: 'cae662172fd450bb0cd710a769079c05bfc5d8e35efa6576edc7d0377afdd4a2',
            diskPersisted: false,
        }]);
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
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
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

    it('does not project a pulled remote update when editor application fails', async () => {
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

        assert.deepStrictEqual(transport.projected, []);
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
            expectedContentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
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
        assert.deepStrictEqual(transport.projected, []);

        releaseBroadcast();
        await localForward;
        await manager.drainRemoteUpdates();

        assert.deepStrictEqual(applied, ['remote text']);
        assert.deepStrictEqual(transport.projected, [
            {
                contentHash: 'ee7973f451eafb37642e8f6e3d8cfd2d806783c48748eebb66c46f781169815a',
                diskPersisted: false,
            },
            {
                contentHash: '3a3a8dbdec63746b4b7f8ac567d759ac146355398a5cbe9854cd9753379dd055',
                diskPersisted: false,
            },
        ]);
    });

it('force-refresh never republishes an unproven full editor snapshot', async () => {
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

        assert.strictEqual(transport.registerCount, 2);
        assert.deepStrictEqual(transport.deregistered, [filePath]);
        assert.deepStrictEqual(nodes.map((node) => node.opened), [42, 43]);
        assert.deepStrictEqual(nodes.flatMap((node) => node.locals), []);
        assert.strictEqual(transport.broadcasts.length, 0);
    });

    it('force-refresh reprojects controller canonical without a deferred editor request', async () => {
    const nodes: FakeNode[] = [];
    const transport = new FakeTransport();
    const filePath = '/work/plan.md';
    let editorText = 'base';
    const applied: Array<{ text: string; expected: string }> = [];
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
        currentText: () => editorText,
        applyText: async (_file, text, expected) => {
            applied.push({ text, expected });
            if (editorText !== expected) return false;
            editorText = text;
            return true;
        },
        });

    assert.strictEqual(await manager.attachDocument(filePath, editorText), true);
    assert.strictEqual(await manager.attachDocument(filePath, editorText, true), true);

        assert.deepStrictEqual(applied, []);
        assert.strictEqual(editorText, 'base');
        assert.strictEqual(transport.registerCount, 2);
        assert.strictEqual(transport.deregistered.length, 1);
        assert.strictEqual(nodes.length, 2);
        assert.deepStrictEqual(nodes[1].locals, []);
});

it('non-operator editor events fence queued deltas and never mutate canonical', async () => {
    const node = new FakeNode('base');
    const transport = new FakeTransport();
    const filePath = '/work/plan.md';
    const manager = new CrdtReplicaManager({
        projectRoot: '/work',
        identity: 'vscode-test',
        transport,
        nodeFactory: () => node,
        listDocuments: () => [],
        currentText: () => 'stale cache projection',
        applyText: async () => true,
    });

    assert.strictEqual(await manager.attachDocument(filePath, 'base'), true);
    const queuedUserEdit = manager.captureLocalChange(filePath, true);
    const cacheReload = manager.captureLocalChange(filePath, false);
    await manager.handleLocalChangeDelta(
        filePath,
        [{ rangeOffset: 4, rangeLength: 0, text: ' typed' }],
        queuedUserEdit,
    );
    await manager.handleLocalChangeDelta(
        filePath,
        [{ rangeOffset: 0, rangeLength: 4, text: 'stale cache projection' }],
        cacheReload,
    );

    assert.deepStrictEqual(node.locals, []);
    assert.deepStrictEqual(transport.broadcasts, []);
});
});

describe('crdt replica IPC response parsing', () => {
    it('parses register and pull responses with base64 update payloads', () => {
        const register = parseRegisterResponse({
            ok: true,
            data: {
                client_id: 42,
            bootstrap_b64: Buffer.from([1, 2]).toString('base64'),
            bootstrap_kind: 'delta',
            canonical_state_vector_b64: Buffer.from([5, 6]).toString('base64'),
            lineage: 'lineage-42',
            },
        });
        assert.deepStrictEqual(register && Array.from(register.bootstrap ?? []), [1, 2]);
        assert.strictEqual(register?.bootstrapKind, 'delta');
        assert.deepStrictEqual(register && Array.from(register.canonicalStateVector ?? []), [5, 6]);
        assert.strictEqual(register?.lineage, 'lineage-42');

        const pull = parsePullResponse({
            ok: true,
            data: {
                updates: [{
                    patch_id: 'crdt:1:42:5',
                    origin: 1,
                    target: 42,
                    generation: 5,
                    expected_content_hash: 'canonical-hash',
                    update_b64: Buffer.from([3, 4]).toString('base64'),
                }],
            },
        });
        assert.strictEqual(pull.length, 1);
        assert.deepStrictEqual(Array.from(pull[0].update), [3, 4]);
        assert.strictEqual(pull[0].expectedContentHash, 'canonical-hash');
    });
});
