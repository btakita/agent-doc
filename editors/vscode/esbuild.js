// Production bundle for the agent-doc VS Code extension.
//
// `vsce package` only ships the extension's own directory, so the lazily-js
// reactive `StateGraphMirror` (imported by `src/stateMirror.ts` from the
// `@lazily-hub/lazily-js`) must be bundled: `vsce package` ships only the
// extension's own directory. esbuild resolves the package at build time and
// INLINES lazily-js's ESM source into
// a single self-contained ESM `out/extension.js`. The extension is an ESM
// package (`"type": "module"`), so lazily-js — itself ESM — is consumed
// through its published package exports rather than a monorepo-relative path.
//
// `vscode` (host-provided) and `koffi` (native addon, loaded lazily via
// createRequire) stay external. Type-checking + the test build remain on tsc
// (`npm run compile` / `npm test`).

import esbuild from 'esbuild';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

function realtimeViewInjection() {
    try {
        require.resolve('@lazily-hub/lazily-js/realtime-view');
        return ['scripts/realtime-view-inject.js'];
    } catch (error) {
        if (error.code === 'ERR_PACKAGE_PATH_NOT_EXPORTED') return [];
        throw error;
    }
}

const production = process.argv.includes('--production');
const watch = process.argv.includes('--watch');

async function main() {
    const ctx = await esbuild.context({
        entryPoints: ['src/extension.ts'],
        inject: realtimeViewInjection(),
        bundle: true,
        format: 'esm',
        platform: 'node',
        target: 'node18',
        outfile: 'out/extension.js',
        external: ['vscode', 'koffi'],
        sourcemap: !production,
        minify: production,
        // Preserve class/function names through minification so the bundled
        // lazily-js `StateGraphMirror` stays identifiable (and `.name`-stable).
        keepNames: true,
        logLevel: 'info',
    });
    if (watch) {
        await ctx.watch();
    } else {
        await ctx.rebuild();
        await ctx.dispose();
    }
}

main().catch((err) => {
    console.error(err);
    process.exit(1);
});
