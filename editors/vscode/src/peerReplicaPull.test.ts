import { describe, it } from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';
import { peerReplicaRebuildPaths } from './peerReplicaPull.js';
import { CrdtReplicaManager, type ReplicaNode, type ReplicaTransport } from './crdtReplica.js';
import { EDITOR_CAPABILITY_LIST } from './native.js';

function registration(pid: number, path: string): string {
    return JSON.stringify({
        document_hash: `h-${pid}`,
        pid,
        path,
        editor_id: `vscode-${pid}`,
        editor_kind: 'vscode',
        editor_version: '0.2.57',
        capabilities: [],
        timestamp_ms: 1,
    });
}

/**
 * `#ctrlkillreregister` Tier 3 — the editor-side pull decision.
 *
 * These assert the two distinctions the caller's correctness hangs on: "could not
 * ask" is not "nothing to do", and another peer's stranded registration is not this
 * editor's to rebuild.
 */
describe('peerReplicaRebuildPaths', () => {
    it('reports nothing to rebuild for an up-to-date editor', () => {
        assert.deepStrictEqual(peerReplicaRebuildPaths('[]', 42), []);
    });

    it('keeps an unanswerable pull distinct from an empty answer', () => {
        // Null must NOT collapse into "nothing to do": the caller relies on the
        // controller's compatibility fan-out when the pull cannot be asked, and
        // silently doing nothing would leave the editor stranded exactly then.
        assert.strictEqual(peerReplicaRebuildPaths(null, 42), null);
        assert.strictEqual(peerReplicaRebuildPaths('', 42), null);
        assert.strictEqual(peerReplicaRebuildPaths('   ', 42), null);
        assert.strictEqual(peerReplicaRebuildPaths('{not json', 42), null);
        assert.strictEqual(peerReplicaRebuildPaths('{"pid":42}', 42), null);

        assert.notStrictEqual(peerReplicaRebuildPaths('[]', 42), null);
    });

    it('rebuilds only this editor process’s own stranded registrations', () => {
        const json = `[${registration(42, '/proj/mine.md')},${registration(7, '/proj/theirs.md')}]`;

        assert.deepStrictEqual(
            peerReplicaRebuildPaths(json, 42),
            ['/proj/mine.md'],
            'rebuilding another editor’s document would publish this buffer’s text over theirs',
        );
        assert.deepStrictEqual(peerReplicaRebuildPaths(json, 7), ['/proj/theirs.md']);
        assert.deepStrictEqual(peerReplicaRebuildPaths(json, 9999), []);
    });

    it('skips duplicate and malformed entries', () => {
        const json = `[${[
            registration(42, '/proj/a.md'),
            registration(42, '/proj/a.md'),
            '{"pid":42}',
            '{"path":"/proj/b.md"}',
            '{"pid":42,"path":""}',
            '"not-an-object"',
            'null',
        ].join(',')}]`;

        assert.deepStrictEqual(peerReplicaRebuildPaths(json, 42), ['/proj/a.md']);
    });
});

class SilentNode implements ReplicaNode {
    open(): boolean {
        return true;
    }
    applyLocal(): boolean {
        return true;
    }
    applyUpdate(): boolean {
        return true;
    }
    encodeState(): Uint8Array {
        return Buffer.from([1]);
    }
    text(): string {
        return 'base';
    }
    close(): void {}
}

class SilentTransport implements ReplicaTransport {
    registered: string[] = [];
    async register(filePath: string) {
        this.registered.push(filePath);
        return { clientId: 42 };
    }
    async broadcastUpdate(): Promise<void> {}
    async pullUpdates() {
        return [];
    }
    async deregister(): Promise<void> {}
}

function manager(
    peerReplicasMissing: (pid: number, held: readonly string[]) => string | null,
    transport = new SilentTransport(),
): { manager: CrdtReplicaManager; transport: SilentTransport; asked: string[][] } {
    const asked: string[][] = [];
    return {
        transport,
        asked,
        manager: new CrdtReplicaManager({
            projectRoot: '/work',
            identity: 'vscode-test',
            pid: 42,
            transport,
            nodeFactory: () => new SilentNode(),
            listDocuments: () => [],
            currentText: () => 'base',
            applyText: async () => true,
            peerReplicasMissing: (pid, held) => {
                asked.push([...held]);
                return peerReplicasMissing(pid, held);
            },
        }),
    };
}

describe('CrdtReplicaManager.pullMissingReplicas', () => {
    it('re-registers exactly the registrations the controller says it cannot serve', async () => {
        const { manager: mgr, transport } = manager(() =>
            `[${registration(42, '/work/stranded.md')}]`,
        );

        mgr.seedDocument('/work/stranded.md', 'base');
        await mgr.pullMissingReplicas('test');

        assert.deepStrictEqual(transport.registered, ['/work/stranded.md']);
    });

    it('rebuilds nothing when the editor is up to date', async () => {
        const { manager: mgr, transport } = manager(() => '[]');

        await mgr.pullMissingReplicas('test');

        assert.deepStrictEqual(
            transport.registered,
            [],
            'a healthy replica must never be dropped and rebuilt',
        );
    });

    it('asks with an empty held set so stale local forwarders cannot suppress the repair', async () => {
        // After a controller kill this editor’s forwarders still look live here, so
        // passing them as `held` would suppress precisely the documents that need
        // repair. The controller subtracts what its own hub can serve instead.
        const { manager: mgr, asked } = manager(() => '[]');

        mgr.seedDocument('/work/looks-live.md', 'base');
        await mgr.attachDocument('/work/looks-live.md');
        await mgr.pullMissingReplicas('test');

        assert.deepStrictEqual(asked, [[]]);
    });

    it('coalesces concurrent pulls: one controller death is one question', async () => {
        let calls = 0;
        const { manager: mgr } = manager(() => {
            calls += 1;
            return '[]';
        });

        await mgr.pullMissingReplicas('transport-loss-doc-a');
        await mgr.pullMissingReplicas('transport-loss-doc-b');
        await mgr.pullMissingReplicas('transport-loss-doc-c');

        assert.strictEqual(calls, 1, 'every open document reports the same controller death');
    });

    it('does not force-refresh healthy replicas when the pull cannot be asked', async () => {
        const { manager: mgr, transport } = manager(() => null);

        mgr.seedDocument('/work/plan.md', 'base');
        await mgr.pullMissingReplicas('test');

        assert.deepStrictEqual(
            transport.registered,
            [],
            'null means "could not ask"; the existing attach/retry paths remain the fallback',
        );
    });
});

describe('peer replica pull capability', () => {
    /**
     * The capability token is the controller’s retirement condition for the Tier 1
     * fan-out. Advertising it without calling the pull would silence the push while
     * nothing repaired — strictly worse than either tier alone.
     */
    it('is advertised only alongside the code that actually pulls', () => {
        assert.ok(EDITOR_CAPABILITY_LIST.includes('peer_replica_pull_v1'));

        const nativeSource = readFileSync(new URL('../src/native.ts', import.meta.url), 'utf8');
        assert.ok(nativeSource.includes("'agent_doc_peer_replicas_missing'"));

        const replicaSource = readFileSync(new URL('../src/crdtReplica.ts', import.meta.url), 'utf8');
        assert.ok(
            replicaSource.includes("this.pullMissingReplicas('controller-transport-recovered')"),
            'reconnect must pull for the whole editor, not just the file that noticed',
        );
        assert.ok(
            replicaSource.includes("this.pullMissingReplicas('activation')"),
            'activation must pull as a safety net after the normal attach pass',
        );
    });
});
