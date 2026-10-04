import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
    aboutMismatches,
    nativeAboutFacts,
    parseAgentDocVersionText,
    parseBuildInfoJson,
    renderAboutReport,
    type AboutAgentDocFacts,
    type AgentDocBuildFacts,
} from './about.js';

const cliJson = JSON.stringify({
    schema: 'agent-doc-build-info-v1',
    component: 'binary',
    version: '0.35.451',
    build_id: '0.35.451+abc',
    executable: '/home/u/.cargo/bin/agent-doc',
    library: '/home/u/.cargo/bin/libagent_doc.so',
    expected_plugins: { jetbrains: '0.2.487', vscode: '0.2.79', zed: null },
});

function facts(overrides: Partial<AboutAgentDocFacts> = {}): AboutAgentDocFacts {
    const native: AgentDocBuildFacts = {
        version: '0.35.451',
        buildId: '0.35.451+abc',
        library: '/lib.so',
        expectedPluginVersion: '0.2.79',
    };
    return {
        pluginVersion: '0.2.79',
        cliCommand: '/home/u/.cargo/bin/agent-doc',
        cli: parseBuildInfoJson(cliJson),
        cliNote: null,
        nativePath: '/lib.so',
        native,
        nativeNote: null,
        ...overrides,
    };
}

describe('about', () => {
    it('parses build info json for the vscode expectation', () => {
        const parsed = parseBuildInfoJson(cliJson)!;
        assert.strictEqual(parsed.version, '0.35.451');
        assert.strictEqual(parsed.buildId, '0.35.451+abc');
        assert.strictEqual(parsed.executable, '/home/u/.cargo/bin/agent-doc');
        assert.strictEqual(parsed.library, '/home/u/.cargo/bin/libagent_doc.so');
        assert.strictEqual(parsed.expectedPluginVersion, '0.2.79');
        assert.strictEqual(parseBuildInfoJson(cliJson, 'zed')!.expectedPluginVersion, null);
    });

    it('rejects output that is not the build info contract', () => {
        assert.strictEqual(parseBuildInfoJson('agent-doc 0.35.451'), null);
        assert.strictEqual(parseBuildInfoJson('{"version":"0.35.451"}'), null);
        assert.strictEqual(parseBuildInfoJson('[]'), null);
    });

    it('parses legacy version text', () => {
        assert.strictEqual(parseAgentDocVersionText('agent-doc 0.35.449\n'), '0.35.449');
        assert.strictEqual(parseAgentDocVersionText(''), null);
        assert.strictEqual(parseAgentDocVersionText('bash 5.2'), null);
        assert.strictEqual(parseAgentDocVersionText('agent-doc'), null);
    });

    it('reports agreement when plugin, cli and native library match', () => {
        const f = facts();
        assert.deepStrictEqual(aboutMismatches(f), []);
        const report = renderAboutReport(f);
        assert(report.startsWith('Agent Doc extension (VS Code): 0.2.79'), report);
        assert(report.includes('agent-doc CLI: 0.35.451'), report);
        assert(report.includes('build id: 0.35.451+abc'), report);
        assert(report.endsWith('Plugin, CLI, and native library agree.'), report);
    });

    it('warns when the cli expects a different plugin version', () => {
        const mismatches = aboutMismatches(facts({ pluginVersion: '0.2.78' }));
        assert.strictEqual(mismatches.length, 1, mismatches.join('\n'));
        assert(mismatches[0].includes('Plugin 0.2.78'), mismatches[0]);
        assert(mismatches[0].includes('expects (0.2.79)'), mismatches[0]);
        assert(renderAboutReport(facts({ pluginVersion: '0.2.78' })).includes('Version mismatch:'));
    });

    it('warns when cli and native library are different builds', () => {
        const native = { version: '0.35.451', buildId: '0.35.451+def', expectedPluginVersion: '0.2.79' };
        const mismatches = aboutMismatches(facts({ native }));
        assert.strictEqual(mismatches.length, 1);
        assert(mismatches[0].includes('0.35.451+abc') && mismatches[0].includes('0.35.451+def'));
    });

    it('falls back to versions when a build id is unknown', () => {
        const old = { version: '0.35.440', buildId: null, expectedPluginVersion: null };
        assert.strictEqual(aboutMismatches(facts({ native: old })).length, 1);
        const same = { version: '0.35.451', buildId: null, expectedPluginVersion: null };
        assert.deepStrictEqual(aboutMismatches(facts({ native: same })), []);
    });

    it('treats an unqueryable cli as a warning and an unloaded library as a note', () => {
        const f = facts({ cli: null, cliNote: 'ENOENT', native: null, nativePath: null, nativeNote: 'not loaded yet' });
        const mismatches = aboutMismatches(f);
        assert.strictEqual(mismatches.length, 1);
        assert(mismatches[0].includes('ENOENT'));
        const report = renderAboutReport(f);
        assert(report.includes('agent-doc CLI: unavailable'), report);
        assert(report.includes('Native library: not loaded'), report);
        assert(report.includes('note: not loaded yet'), report);
    });

    it('derives native facts from the loaded library snapshot', () => {
        const old = nativeAboutFacts({ path: '/lib.so', version: '0.35.440', buildInfoJson: null });
        assert.strictEqual(old.facts?.version, '0.35.440');
        assert.strictEqual(old.facts?.buildId, null);
        assert(old.note?.includes('build id unavailable'));

        const json = cliJson.replace('"binary"', '"native_library"');
        const current = nativeAboutFacts({ path: '/lib.so', version: '0.35.451', buildInfoJson: json });
        assert.strictEqual(current.facts?.buildId, '0.35.451+abc');
        assert.strictEqual(current.facts?.library, '/lib.so');
        assert.strictEqual(current.facts?.expectedPluginVersion, '0.2.79');
        assert.strictEqual(current.note, null);

        const unloaded = nativeAboutFacts({ path: null, version: null, buildInfoJson: null });
        assert.strictEqual(unloaded.facts, null);
    });
});
