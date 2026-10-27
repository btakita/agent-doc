// Packaging test for `#vscodekoffivsix`: the VSIX must carry the koffi FFI
// runtime for every published platform, and koffi must load from the unpacked
// extension alone. A VSIX without it activates with the native library off, so
// nothing registers with the Project Controller.
//
// With AGENT_DOC_VSIX=<path> it checks that VSIX (the release workflow and
// `make install-editor-plugins` pass the one they built); otherwise it runs
// `vsce package` into a temporary directory first.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';

import { EXTENSION_ROOT, koffiPlatforms } from './stage-koffi.mjs';

// The `<triplet>` directories each @koromix/koffi-<platform> package carries.
const TRIPLETS = {
    'linux-x64': ['linux_x64', 'musl_x64'],
    'linux-arm64': ['linux_arm64', 'musl_arm64'],
    'darwin-x64': ['darwin_x64'],
    'darwin-arm64': ['darwin_arm64'],
    'win32-x64': ['win32_x64'],
};

// Executable-format magic for each triplet's OS, so a mislabelled or truncated
// native is caught without running it.
function expectedMagic(triplet) {
    if (triplet.startsWith('darwin_')) return Buffer.from([0xcf, 0xfa, 0xed, 0xfe]); // Mach-O 64-bit LE
    if (triplet.startsWith('win32_')) return Buffer.from('MZ');
    return Buffer.from([0x7f, 0x45, 0x4c, 0x46]); // ELF
}

let scratch;
let extensionDir;

before(() => {
    scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-doc-vsix-test-'));
    let vsix = process.env.AGENT_DOC_VSIX;
    if (vsix) {
        vsix = path.resolve(vsix);
    } else {
        vsix = path.join(scratch, 'agent-doc.vsix');
        const vsce = path.join(EXTENSION_ROOT, 'node_modules', '.bin', process.platform === 'win32' ? 'vsce.cmd' : 'vsce');
        execFileSync(vsce, ['package', '--skip-license', '--out', vsix], {
            cwd: EXTENSION_ROOT,
            stdio: ['ignore', 'ignore', 'inherit'],
            shell: process.platform === 'win32',
        });
    }
    assert.ok(fs.existsSync(vsix), `VSIX not found: ${vsix}`);
    // Unpack outside the repository so `require('koffi')` cannot fall back to
    // the checkout's node_modules.
    execFileSync('unzip', ['-q', vsix, '-d', path.join(scratch, 'unpacked')]);
    extensionDir = path.join(scratch, 'unpacked', 'extension');
});

after(() => {
    if (scratch) fs.rmSync(scratch, { recursive: true, force: true });
});

function packagedFiles(dir, prefix = '') {
    return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
        const rel = prefix ? `${prefix}/${e.name}` : e.name;
        return e.isDirectory() ? packagedFiles(path.join(dir, e.name), rel) : [rel];
    });
}

test('VSIX carries koffi runtime JavaScript and license', () => {
    for (const f of ['package.json', 'LICENSE.txt', 'index.cjs', 'src/koffi/index.cjs', 'src/koffi/src/static.cjs']) {
        assert.ok(fs.existsSync(path.join(extensionDir, 'node_modules', 'koffi', f)), `missing node_modules/koffi/${f}`);
    }
});

test('VSIX carries a koffi native for every published platform', () => {
    for (const platform of koffiPlatforms()) {
        const triplets = TRIPLETS[platform];
        assert.ok(triplets, `no expected triplets recorded for ${platform}`);
        for (const triplet of triplets) {
            const file = path.join(extensionDir, 'node_modules', 'koffi', 'build', 'koffi', triplet, 'koffi.node');
            assert.ok(fs.existsSync(file), `missing koffi native for ${platform}: ${triplet}/koffi.node`);
            const magic = expectedMagic(triplet);
            const head = Buffer.alloc(magic.length);
            const fd = fs.openSync(file, 'r');
            try {
                fs.readSync(fd, head, 0, magic.length, 0);
            } finally {
                fs.closeSync(fd);
            }
            assert.deepEqual(head, magic, `${triplet}/koffi.node is not a ${triplet} executable`);
        }
    }
});

test('VSIX ships no other node_modules content', () => {
    const files = packagedFiles(path.join(extensionDir, 'node_modules'));
    const allowed = /^koffi\/(package\.json|LICENSE\.txt|index\.cjs|src\/koffi\/index\.cjs|src\/koffi\/src\/static\.cjs|build\/koffi\/[^/]+\/koffi\.node)$/;
    assert.deepEqual(files.filter((f) => !allowed.test(f)), []);
});

test('koffi loads from the unpacked extension and calls into the host C library', () => {
    const entry = path.join(extensionDir, 'out', 'extension.js');
    assert.ok(fs.existsSync(entry), 'missing out/extension.js');
    // The extension resolves koffi with createRequire(import.meta.url) from
    // out/extension.js (src/native.ts); do the same in a fresh process.
    const probe = `
        const { createRequire } = require('node:module');
        const req = createRequire(process.argv[1]);
        const koffi = req('koffi');
        const natives = Object.keys(req.cache).filter((f) => f.endsWith('.node'));
        const [lib, decl] = process.platform === 'win32'
            ? ['kernel32.dll', 'uint32 __stdcall GetCurrentProcessId()']
            : [process.platform === 'darwin' ? 'libSystem.dylib' : 'libc.so.6', 'int getpid()'];
        const pid = koffi.load(lib).func(decl)();
        console.log(JSON.stringify({ version: koffi.version, natives, pid, expected: process.pid }));
    `;
    const out = JSON.parse(execFileSync(process.execPath, ['-e', probe, entry], { cwd: os.tmpdir(), encoding: 'utf8' }));
    const lockedVersion = JSON.parse(fs.readFileSync(path.join(extensionDir, 'node_modules', 'koffi', 'package.json'), 'utf8')).version;
    assert.equal(out.version, lockedVersion);
    assert.equal(out.natives.length, 1, `expected one koffi native, loaded ${JSON.stringify(out.natives)}`);
    const nativeRoot = path.join(fs.realpathSync(extensionDir), 'node_modules', 'koffi', 'build', 'koffi') + path.sep;
    assert.ok(fs.realpathSync(out.natives[0]).startsWith(nativeRoot), `koffi native came from outside the VSIX: ${out.natives[0]}`);
    assert.equal(out.pid, out.expected);
});
