// Stage koffi's prebuilt native modules into the VSIX (`#vscodekoffivsix`).
//
// koffi 3 ships its `.node` binaries as per-platform optional packages
// (`@koromix/koffi-<platform>-<arch>`), and `npm ci` installs only the build
// host's one. A VSIX built from that tree would carry one platform's FFI, so a
// VSIX built on Linux x64 and installed on macOS, Windows or a Linux arm64
// workspace host (Coder) could not load libagent_doc at all.
//
// This script copies the `<triplet>/koffi.node` directories of every platform
// agent-doc publishes libagent_doc for into `node_modules/koffi/build/koffi/`,
// the bundled-prebuild root koffi's own loader searches after the optional
// package. `.vscodeignore` ships that directory with koffi's runtime JS and
// drops `node_modules/@koromix/**`, so the VSIX is the same on every build
// host. The host's package is copied from `node_modules`; the others are
// fetched with `npm pack` (served from the npm cache once seen), and each
// tarball must match the `integrity` package-lock.json pins.
//
// `AGENT_DOC_KOFFI_PLATFORMS=linux-x64` narrows the set for an offline local
// build; the packaging test reads the same variable.

import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Keep in step with the libagent_doc targets of .github/workflows/release.yml
// (x86_64/aarch64 Linux gnu+musl, x86_64/aarch64 macOS, x86_64 Windows).
export const DEFAULT_KOFFI_PLATFORMS = ['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64', 'win32-x64'];

export const EXTENSION_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function koffiPlatforms(env = process.env) {
    const raw = env.AGENT_DOC_KOFFI_PLATFORMS?.trim();
    if (!raw) return DEFAULT_KOFFI_PLATFORMS;
    return raw.split(',').map((p) => p.trim()).filter(Boolean);
}

function sriOf(file) {
    return 'sha512-' + crypto.createHash('sha512').update(fs.readFileSync(file)).digest('base64');
}

function npm(args, cwd) {
    return execFileSync('npm', args, { cwd, encoding: 'utf8', shell: process.platform === 'win32', stdio: ['ignore', 'ignore', 'inherit'] });
}

function packageDir(root, platform, version, integrity, scratch) {
    const name = `@koromix/koffi-${platform}`;
    const installed = path.join(root, 'node_modules', name);
    const installedPkg = path.join(installed, 'package.json');
    if (fs.existsSync(installedPkg) && JSON.parse(fs.readFileSync(installedPkg, 'utf8')).version === version) {
        return installed;
    }
    const dest = fs.mkdtempSync(path.join(scratch, `${platform}-`));
    // `npm pack --json` changed shape across npm majors (array vs keyed
    // object), so read the one tarball it wrote instead.
    npm(['pack', `${name}@${version}`, '--prefer-offline', '--loglevel=error', '--pack-destination', dest], root);
    const tarballs = fs.readdirSync(dest).filter((f) => f.endsWith('.tgz'));
    if (tarballs.length !== 1) throw new Error(`npm pack ${name}@${version} wrote ${tarballs.length} tarballs`);
    const tarball = path.join(dest, tarballs[0]);
    const actual = sriOf(tarball);
    if (actual !== integrity) {
        throw new Error(`${name}@${version}: tarball integrity ${actual} does not match package-lock.json ${integrity}`);
    }
    execFileSync('tar', ['-xzf', tarball, '-C', dest]);
    return path.join(dest, 'package');
}

export function stageKoffi({ root = EXTENSION_ROOT, platforms = koffiPlatforms() } = {}) {
    const lock = JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'), 'utf8'));
    const version = lock.packages?.['node_modules/koffi']?.version;
    if (!version) throw new Error('package-lock.json does not lock koffi');
    const koffiDir = path.join(root, 'node_modules', 'koffi');
    const installed = JSON.parse(fs.readFileSync(path.join(koffiDir, 'package.json'), 'utf8')).version;
    if (installed !== version) {
        throw new Error(`node_modules/koffi is ${installed}, package-lock.json locks ${version}; run npm ci`);
    }

    const buildDir = path.join(koffiDir, 'build', 'koffi');
    fs.rmSync(buildDir, { recursive: true, force: true });
    fs.mkdirSync(buildDir, { recursive: true });
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-doc-koffi-'));
    const staged = [];
    try {
        for (const platform of platforms) {
            const entry = lock.packages[`node_modules/@koromix/koffi-${platform}`];
            if (!entry || entry.version !== version || !entry.integrity) {
                throw new Error(`package-lock.json has no @koromix/koffi-${platform}@${version} with an integrity`);
            }
            const src = packageDir(root, platform, version, entry.integrity, scratch);
            const triplets = fs.readdirSync(src).filter((d) => fs.existsSync(path.join(src, d, 'koffi.node')));
            if (triplets.length === 0) throw new Error(`@koromix/koffi-${platform}@${version} has no <triplet>/koffi.node`);
            for (const triplet of triplets) {
                fs.mkdirSync(path.join(buildDir, triplet), { recursive: true });
                fs.copyFileSync(path.join(src, triplet, 'koffi.node'), path.join(buildDir, triplet, 'koffi.node'));
                staged.push(triplet);
            }
        }
    } finally {
        fs.rmSync(scratch, { recursive: true, force: true });
    }
    return { version, staged: staged.sort() };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const { version, staged } = stageKoffi();
    console.log(`[stage-koffi] koffi ${version}: ${staged.join(', ')}`);
}
