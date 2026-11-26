/** Pure helpers for Claim Artifact Session. No `vscode` import, so node:test can load them. */

export function normalizeSessionPath(value: string): string {
    return value.trim().replace(/\\/g, '/').replace(/^\.\//, '');
}

export function defaultControlSessionPath(artifactRel: string): string {
    const normalized = normalizeSessionPath(artifactRel);
    const stem = normalized.toLowerCase().endsWith('.md') ? normalized.slice(0, -3) : normalized;
    return `${stem}.session.md`;
}

export function acceptableControlPath(controlRel: string, artifactRel: string): boolean {
    const control = normalizeSessionPath(controlRel);
    const artifact = normalizeSessionPath(artifactRel);
    return control.length > 0
        && control.toLowerCase().endsWith('.md')
        && control !== artifact
        && !control.startsWith('/')
        && !control.includes('\n')
        && !control.includes('\r');
}

export function buildArtifactInitArgs(controlRel: string, artifactRel: string): string[] {
    return ['init', controlRel, '--artifact', artifactRel];
}
