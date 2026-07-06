import { describe, it } from 'node:test';
import assert from 'node:assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as crypto from 'crypto';
import { pathToFileURL } from 'url';
import {
  buildStateEvent,
  compactProjectionSummary,
  documentHash,
  libMtimeChanged,
  nativeShadowCopyPath,
  projectionSummary,
  writePidLock,
  removePidLock,
  utf16RangeToUtf8Bytes,
} from './native';

async function importLazilyStateProjection(): Promise<any> {
  const moduleUrl = pathToFileURL(
    path.resolve(__dirname, '../../../../lazily-js/src/state-projection.js'),
  ).href;
  const dynamicImport = new Function('specifier', 'return import(specifier)') as (
    specifier: string,
  ) => Promise<any>;
  return dynamicImport(moduleUrl);
}

describe('utf16RangeToUtf8Bytes (#qnodemerge4wire non-ASCII offset semantics)', () => {
    it('ASCII: byte units equal UTF-16 units', () => {
        // "hello world", insert at offset 5 (UTF-16) → byte offset 5, no delete.
        assert.deepStrictEqual(utf16RangeToUtf8Bytes('hello world', 5, 0), {
            byteOffset: 5,
            deleteBytes: 0,
        });
    });

    it('multibyte prefix: byte offset exceeds the UTF-16 offset', () => {
        // "café " — é is 2 UTF-8 bytes (1 UTF-16 unit). UTF-16 offset 5 (after
        // "café ") → 6 bytes. A delete of the next 2 UTF-16 units of "日本"
        // (each 3 bytes) → 6 delete bytes.
        const old = 'café 日本';
        assert.deepStrictEqual(utf16RangeToUtf8Bytes(old, 5, 2), {
            byteOffset: 6,
            deleteBytes: 6,
        });
    });

    it('astral plane (surrogate pairs): emoji counts its real UTF-8 bytes', () => {
        // "a😀b": 😀 is 1 codepoint = 2 UTF-16 units = 4 UTF-8 bytes. UTF-16
        // offset 1 is before the emoji → 1 byte; deleting the emoji's 2 UTF-16
        // units → 4 delete bytes.
        assert.deepStrictEqual(utf16RangeToUtf8Bytes('a😀b', 1, 2), {
            byteOffset: 1,
            deleteBytes: 4,
        });
    });
});

describe('state projection bridge (#lzstatewire1)', () => {
    it('documentHash uses canonical path sha256', () => {
        const tmp = path.join(os.tmpdir(), `agent_doc_state_${Date.now()}.md`);
        fs.writeFileSync(tmp, 'state');
        try {
            const expected = crypto.createHash('sha256')
                .update(fs.realpathSync(tmp), 'utf-8')
                .digest('hex');
            assert.strictEqual(documentHash(tmp), expected);
        } finally {
            fs.unlinkSync(tmp);
        }
    });

    it('buildStateEvent matches Rust state backbone serde shape', () => {
        assert.deepStrictEqual(
            buildStateEvent(
                'doc-a',
                'editor_patch_queued',
                { patch_id: 'patch-1', actor_generation: 7 },
                'editor-patch-queued-patch-1-7',
            ),
            {
                event_id: 'doc-a:editor-patch-queued-patch-1-7',
                fact: {
                    type: 'editor_patch_queued',
                    document_hash: 'doc-a',
                    patch_id: 'patch-1',
                    actor_generation: 7,
                },
            },
        );
    });

  it('projectionSummary renders route transport and proof slices', () => {
    const summary = projectionSummary({
      document_hash: 'doc-a',
      route: { generation: 3, pane_id: '%2', readiness: 'dispatch_proven' },
      transport: { patches: { 'patch-1': { phase: 'queued' }, 'patch-2': { phase: 'applied' } } },
      proof: { markers: { dispatch_start: { phase: 'observed', sources: ['route'] } } },
    });

    assert.deepStrictEqual(summary, {
      routeReadiness: 'dispatch_proven',
      routePaneId: '%2',
      latestTransportPatchId: 'patch-2',
      latestTransportPhase: 'applied',
      proofMarkers: 1,
    });
    assert.strictEqual(
      compactProjectionSummary(summary!),
      'route=dispatch_proven pane=%2 transport=patch-2:applied proof_markers=1',
    );
  });

  it('matches @lazily/js pure helper contract (#lzpkgwire)', async () => {
    const lazily = await importLazilyStateProjection();
    const tmp = path.join(os.tmpdir(), `agent_doc_state_${Date.now()}_parity.md`);
    fs.writeFileSync(tmp, 'state');
    try {
      assert.strictEqual(documentHash(tmp), lazily.documentHash(tmp));
    } finally {
      fs.unlinkSync(tmp);
    }

    const event = buildStateEvent(
      'doc-a',
      'editor_patch_queued',
      { patch_id: 'patch-1', actor_generation: 7 },
      'editor-patch-queued-patch-1-7',
    );
    assert.deepStrictEqual(
      event,
      lazily.buildStateEvent(
        'doc-a',
        'editor_patch_queued',
        { patch_id: 'patch-1', actor_generation: 7 },
        'editor-patch-queued-patch-1-7',
      ),
    );

    const projection = {
      document_hash: 'doc-a',
      route: { generation: 3, pane_id: '%2', readiness: 'dispatch_proven' },
      transport: { patches: { 'patch-1': { phase: 'queued' }, 'patch-2': { phase: 'applied' } } },
      proof: { markers: { dispatch_start: { phase: 'observed', sources: ['route'] } } },
    };
    const summary = projectionSummary(projection)!;
    assert.deepStrictEqual(summary, lazily.projectionSummary(projection));
    assert.strictEqual(
      compactProjectionSummary(summary),
      lazily.compactProjectionSummary(summary),
    );
  });
});

describe('libMtimeChanged', () => {
    it('returns false when mtime unchanged', () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'content');
        try {
            const mtime = fs.statSync(tmp).mtimeMs;
            assert.strictEqual(libMtimeChanged(tmp, mtime), false);
        } finally {
            fs.unlinkSync(tmp);
        }
    });

    it('returns true when file modified', async () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'original');
        try {
            const oldMtime = fs.statSync(tmp).mtimeMs;
            await new Promise(resolve => setTimeout(resolve, 1100));
            fs.writeFileSync(tmp, 'updated');
            assert.strictEqual(libMtimeChanged(tmp, oldMtime), true);
        } finally {
            fs.unlinkSync(tmp);
        }
    });

    it('returns false for nonexistent file', () => {
        assert.strictEqual(
            libMtimeChanged(`/tmp/nonexistent_lib_${Date.now()}.so`, 99999),
            false
        );
    });

    it('returns false when storedMtime matches current', () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'content');
        try {
            const mtime = fs.statSync(tmp).mtimeMs;
            assert.strictEqual(libMtimeChanged(tmp, mtime), false);
        } finally {
            fs.unlinkSync(tmp);
        }
    });
});

describe('pidLock', () => {
    it('writePidLock creates lock file and removePidLock removes it', () => {
        const tmp = path.join(os.tmpdir(), `libagent_doc_test_${Date.now()}.so`);
        fs.writeFileSync(tmp, 'content');
        const expectedLock = `${tmp}.pid.${process.pid}`;
        try {
            writePidLock(tmp);
            assert.strictEqual(fs.existsSync(expectedLock), true);
            removePidLock();
            assert.strictEqual(fs.existsSync(expectedLock), false);
        } finally {
            try { fs.unlinkSync(tmp); } catch {}
            try { fs.unlinkSync(expectedLock); } catch {}
        }
    });

    it('removePidLock is safe to call without prior write', () => {
        assert.doesNotThrow(() => removePidLock());
    });
});

describe('nativeShadowCopyPath (#nativehotreload — dlopen-fresh-inode reload)', () => {
    it('copies to a distinct per-mtime path with matching contents', () => {
        const root = path.join(os.tmpdir(), `agent-doc-native-test-${crypto.randomUUID()}`);
        const src = path.join(os.tmpdir(), `libagent_doc_src_${crypto.randomUUID()}.so`);
        try {
            fs.writeFileSync(src, 'native-code-v1');
            const mtime = fs.statSync(src).mtimeMs;
            const shadow = nativeShadowCopyPath(src, mtime, root);
            assert.ok(shadow);
            assert.notStrictEqual(shadow, src);
            assert.ok(shadow!.includes(`libagent_doc-${Math.floor(mtime)}`));
            assert.strictEqual(fs.readFileSync(shadow!, 'utf8'), 'native-code-v1');
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
            fs.rmSync(src, { force: true });
        }
    });

    it('prunes the prior stale copy on a new install', () => {
        const root = path.join(os.tmpdir(), `agent-doc-native-test-${crypto.randomUUID()}`);
        const src = path.join(os.tmpdir(), `libagent_doc_src_${crypto.randomUUID()}.so`);
        try {
            fs.writeFileSync(src, 'native-code-v1');
            const first = nativeShadowCopyPath(src, 1000000, root);
            assert.ok(first);
            fs.writeFileSync(src, 'native-code-v2-longer');
            const second = nativeShadowCopyPath(src, 2000000, root);
            assert.ok(second);
            assert.notStrictEqual(first, second);
            assert.strictEqual(fs.readFileSync(second!, 'utf8'), 'native-code-v2-longer');
            assert.strictEqual(fs.existsSync(first!), false);
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
            fs.rmSync(src, { force: true });
        }
    });

    it('reuses the same path for an unchanged install', () => {
        const root = path.join(os.tmpdir(), `agent-doc-native-test-${crypto.randomUUID()}`);
        const src = path.join(os.tmpdir(), `libagent_doc_src_${crypto.randomUUID()}.so`);
        try {
            fs.writeFileSync(src, 'native-code-v1');
            const mtime = fs.statSync(src).mtimeMs;
            const a = nativeShadowCopyPath(src, mtime, root);
            const b = nativeShadowCopyPath(src, mtime, root);
            assert.strictEqual(a, b);
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
            fs.rmSync(src, { force: true });
        }
    });
});
