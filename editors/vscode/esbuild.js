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

const production = process.argv.includes('--production');
const watch = process.argv.includes('--watch');

async function main() {
    const ctx = await esbuild.context({
        entryPoints: ['src/extension.ts'],
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
