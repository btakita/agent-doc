/**
 * `editoractionmenu`: About Agent Doc. Pure parsing, mismatch, and report
 * logic, shared shape with the JetBrains `AboutAgentDocAction`.
 */

export const ABOUT_EDITOR_KIND = 'vscode';

/** One agent-doc component's build identity (`agent-doc-build-info-v1`, or version text). */
export interface AgentDocBuildFacts {
    version: string | null;
    buildId: string | null;
    executable?: string | null;
    library?: string | null;
    expectedPluginVersion: string | null;
}

export interface AboutAgentDocFacts {
    pluginVersion: string;
    cliCommand: string;
    cli: AgentDocBuildFacts | null;
    cliNote: string | null;
    nativePath: string | null;
    native: AgentDocBuildFacts | null;
    nativeNote: string | null;
}

function stringOrNull(value: unknown): string | null {
    return typeof value === 'string' && value.trim() !== '' ? value : null;
}

/** Parse `agent-doc version --json` / `agent_doc_build_info_json()`; null when not that contract. */
export function parseBuildInfoJson(
    json: string,
    editorKind: string = ABOUT_EDITOR_KIND,
): AgentDocBuildFacts | null {
    let root: unknown;
    try {
        root = JSON.parse(json.trim());
    } catch {
        return null;
    }
    if (!root || typeof root !== 'object' || Array.isArray(root)) return null;
    const obj = root as Record<string, unknown>;
    if (!stringOrNull(obj.schema)?.startsWith('agent-doc-build-info-')) return null;
    const expected = obj.expected_plugins;
    const expectedObj =
        expected && typeof expected === 'object' && !Array.isArray(expected)
            ? (expected as Record<string, unknown>)
            : {};
    return {
        version: stringOrNull(obj.version),
        buildId: stringOrNull(obj.build_id),
        executable: stringOrNull(obj.executable),
        library: stringOrNull(obj.library),
        expectedPluginVersion: stringOrNull(expectedObj[editorKind]),
    };
}

/** Parse `agent-doc --version` (`agent-doc 0.35.451`), the fallback for older binaries. */
export function parseAgentDocVersionText(text: string): string | null {
    const line = text.split('\n').map(l => l.trim()).find(l => l !== '');
    if (!line) return null;
    const parts = line.split(/\s+/);
    if (parts.length < 2 || !parts[0].startsWith('agent-doc')) return null;
    return /^\d/.test(parts[1]) ? parts[1] : null;
}

/** Native side of the facts, from the library the extension already loaded. */
export function nativeAboutFacts(snapshot: {
    path: string | null;
    version: string | null;
    buildInfoJson: string | null;
}): { facts: AgentDocBuildFacts | null; note: string | null } {
    if (!snapshot.version) {
        return { facts: null, note: 'loads when an agent-doc markdown document is opened' };
    }
    const parsed = snapshot.buildInfoJson ? parseBuildInfoJson(snapshot.buildInfoJson) : null;
    return {
        facts: {
            version: parsed?.version ?? snapshot.version,
            buildId: parsed?.buildId ?? null,
            library: snapshot.path,
            expectedPluginVersion: parsed?.expectedPluginVersion ?? null,
        },
        note: parsed ? null : 'library predates agent_doc_build_info_json; build id unavailable',
    };
}

/** Mismatches worth a warning: each one names both sides and what to do. */
export function aboutMismatches(facts: AboutAgentDocFacts): string[] {
    const out: string[] = [];
    const { cli, native } = facts;
    if (!cli) {
        out.push(`The agent-doc CLI (${facts.cliCommand}) could not be queried: ${facts.cliNote ?? 'unknown error'}.`);
    }
    const cliExpected = cli?.expectedPluginVersion ?? null;
    if (cliExpected && cliExpected !== facts.pluginVersion) {
        out.push(
            `Plugin ${facts.pluginVersion} is not the version agent-doc CLI ${cli?.version ?? '?'} expects `
            + `(${cliExpected}). Update the extension or the binary so they come from the same release.`,
        );
    }
    const nativeExpected = native?.expectedPluginVersion ?? null;
    if (nativeExpected && nativeExpected !== facts.pluginVersion && nativeExpected !== cliExpected) {
        out.push(
            `Plugin ${facts.pluginVersion} is not the version the loaded native library `
            + `${native?.version ?? '?'} expects (${nativeExpected}).`,
        );
    }
    if (cli && native) {
        if (cli.buildId && native.buildId) {
            if (cli.buildId !== native.buildId) {
                out.push(
                    `The agent-doc CLI (build ${cli.buildId}) and the loaded native library `
                    + `(build ${native.buildId}) are different builds; the IPC handshake rejects `
                    + 'mismatched builds. Reinstall agent-doc, then reload the window.',
                );
            }
        } else if (cli.version && native.version && cli.version !== native.version) {
            out.push(
                `The agent-doc CLI is ${cli.version} but the loaded native library is ${native.version}. `
                + 'Reinstall agent-doc, then reload the window.',
            );
        }
    }
    return out;
}

/** The report body: versions first, then warnings. */
export function renderAboutReport(
    facts: AboutAgentDocFacts,
    mismatches: string[] = aboutMismatches(facts),
): string {
    const lines: string[] = [];
    lines.push(`Agent Doc extension (VS Code): ${facts.pluginVersion}`, '');
    lines.push(`agent-doc CLI: ${facts.cli?.version ?? 'unavailable'}`);
    lines.push(`  binary: ${facts.cli?.executable ?? facts.cliCommand}`);
    lines.push(`  build id: ${facts.cli?.buildId ?? 'unknown'}`);
    if (facts.cli?.library) lines.push(`  paired native library: ${facts.cli.library}`);
    if (facts.cli?.expectedPluginVersion) lines.push(`  expects plugin: ${facts.cli.expectedPluginVersion}`);
    if (facts.cli && facts.cliNote) lines.push(`  note: ${facts.cliNote}`);
    lines.push('');
    lines.push(`Native library: ${facts.native?.version ?? 'not loaded'}`);
    lines.push(`  path: ${facts.nativePath ?? 'unresolved'}`);
    if (facts.native) {
        lines.push(`  build id: ${facts.native.buildId ?? 'unknown'}`);
        if (facts.native.expectedPluginVersion) {
            lines.push(`  expects plugin: ${facts.native.expectedPluginVersion}`);
        }
    }
    if (facts.nativeNote) lines.push(`  note: ${facts.nativeNote}`);
    lines.push('');
    if (mismatches.length === 0) {
        lines.push('Plugin, CLI, and native library agree.');
    } else {
        lines.push('Version mismatch:');
        for (const m of mismatches) lines.push(`- ${m}`);
    }
    return lines.join('\n');
}
