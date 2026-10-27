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
    koffiModuleAvailable,
    OWNED_C_STRING_POINTER,
    OWNED_C_STRING_STRUCT_FIELDS,
    freeOwnedCString,
} from './native.js';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';

// ESM has no `__dirname`; derive it from the module URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

async function importLazilyStateProjection(): Promise<any> {
  // This module is ESM now, so a plain dynamic import resolves the package
  // export directly. The old `new Function('return import(...)')` indirection
  // existed only to reach ESM from CommonJS, and it resolved bare specifiers
  // against `out/` instead of the package.
  return import('@lazily-hub/lazily-js/state-projection');
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

describe('koffi ESM resolution (#crdtauth6)', () => {
    it('resolves the packaged native dependency from compiled ESM', () => {
        assert.strictEqual(koffiModuleAvailable(), true);
    });

    it('keeps Rust-owned C strings opaque until the wrapper frees them', () => {
        assert.strictEqual(OWNED_C_STRING_POINTER, 'void*');
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

describe('Rust-owned C string ownership at the koffi boundary (#vscodekoffisegv)', () => {
    const koffi = createRequire(import.meta.url)('koffi');

    it('binds every owned result-struct member as an opaque pointer', () => {
        for (const [name, fields] of Object.entries(OWNED_C_STRING_STRUCT_FIELDS)) {
            for (const [field, type] of Object.entries(fields)) {
                assert.strictEqual(type, OWNED_C_STRING_POINTER, `${name}.${field}`);
            }
        }
    });

    it('pins why: koffi decodes a char* member to a JS string copy, a void* member to the pointer', () => {
        const owned = koffi.alloc('char', 6);
        try {
            koffi.encode(owned, 'char', 'hello', 6);
            const slot = Buffer.alloc(2 * koffi.sizeof('void*'));
            koffi.encode(slot, 0, 'void*', owned);
            const asCharStar = koffi.decode(slot, koffi.struct({ text: 'char*', error: 'char*' }));
            // A string here cannot be freed: the Rust allocation is already lost.
            assert.strictEqual(typeof asCharStar.text, 'string');
            const asOwned = koffi.decode(slot, koffi.struct(OWNED_C_STRING_STRUCT_FIELDS.patch));
            assert.notStrictEqual(typeof asOwned.text, 'string');
            assert.strictEqual(asOwned.error, null);
            assert.strictEqual(koffi.decode(asOwned.text, 'char', -1), 'hello');
        } finally {
            koffi.free(owned);
        }
    });

    it('never hands a decoded JS string to agent_doc_free_string', () => {
        const freed: unknown[] = [];
        const free = (p: unknown) => { freed.push(p); };
        const errors: unknown[] = [];
        const originalError = console.error;
        console.error = (...args: unknown[]) => { errors.push(args.join(' ')); };
        try {
            assert.strictEqual(freeOwnedCString('decoded text', free), false);
            assert.strictEqual(freeOwnedCString(42, free), false);
        } finally {
            console.error = originalError;
        }
        assert.deepStrictEqual(freed, []);
        assert.strictEqual(errors.length, 2);

        assert.strictEqual(freeOwnedCString(null, free), false);
        assert.strictEqual(freeOwnedCString(undefined, free), false);
        assert.strictEqual(freeOwnedCString(0n, free), false);
        assert.strictEqual(freeOwnedCString(0x1000n, free), true);
        assert.deepStrictEqual(freed, [0x1000n]);
    });

    // Drives the real wrappers against the built cdylib in a child process, so
    // the pre-fix SIGSEGV in `free` (Rust freeing Koffi's temporary string copy)
    // fails this test instead of killing the runner.
    const libName = process.platform === 'darwin' ? 'libagent_doc.dylib' : 'libagent_doc.so';
    const repoRoot = path.resolve(__dirname, '..', '..', '..');
    const libPath = [
        process.env.AGENT_DOC_LIB_PATH,
        path.join(repoRoot, 'target', 'debug', libName),
        path.join(repoRoot, 'target', 'release', libName),
    ].find((candidate) => candidate && fs.existsSync(candidate));

    it('round-trips owned strings through the built library without crashing', { skip: libPath ? false : 'no built libagent_doc (cargo build --lib)' }, () => {
        const nativeUrl = pathToFileURL(path.join(__dirname, 'native.js')).href;
        const script = `
            const n = await import(${JSON.stringify(nativeUrl)});
            const doc = '<!-- agent:exchange -->\\nhi\\n<!-- /agent:exchange -->\\n';
            const out = {};
            for (let i = 0; i < 25; i++) {
                out.normalize = n.normalizeTemplateStructure(doc);
                out.reposition = n.repositionBoundaryToEnd(doc);
                out.preserveHead = n.repositionBoundaryToEndPreserveHead(doc);
                out.nodePatches = n.applyNodePatches(doc, []);
                out.nodePatchesError = n.applyNodePatches(doc, [{ bogus: 1 }]);
                out.deriveError = n.editorDeriveJson('{}');
                out.resolved = n.resolveProjectPath(${JSON.stringify(path.join(__dirname, 'native.js'))});
            }
            process.stdout.write('RESULT ' + JSON.stringify(out) + '\\n');
        `;
        const child = spawnSync(process.execPath, ['--input-type=module', '-e', script], {
            env: { ...process.env, AGENT_DOC_LIB_PATH: libPath },
            encoding: 'utf-8',
            timeout: 60_000,
        });
        assert.strictEqual(child.signal, null, `child died by ${child.signal}: ${child.stderr}`);
        assert.strictEqual(child.status, 0, child.stderr);
        assert.doesNotMatch(child.stderr, /refusing to free/);
        const line = child.stdout.split('\n').find((l) => l.startsWith('RESULT '));
        assert.ok(line, child.stdout);
        const out = JSON.parse(line.slice('RESULT '.length));
        const doc = '<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n';
        assert.strictEqual(out.normalize, doc);
        assert.strictEqual(out.nodePatches, doc);
        assert.match(out.reposition, /agent:boundary:/);
        assert.match(out.preserveHead, /agent:boundary:/);
        assert.strictEqual(out.nodePatchesError, null);
        assert.strictEqual(out.deriveError, null);
        assert.strictEqual(out.resolved?.relativePath?.endsWith('native.js'), true);
    });
});
