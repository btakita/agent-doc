import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';

export function resolveAgentDocRootForFile(filePath: string): string | undefined {
    let dir = path.dirname(filePath);
    const root = path.parse(dir).root;

    while (dir && dir !== root) {
        if (fs.existsSync(path.join(dir, '.agent-doc'))) {
            return dir;
        }
        const parent = path.dirname(dir);
        if (parent === dir) break;
        dir = parent;
    }

    return undefined;
}

export function docHash(filePath: string): string {
    return crypto.createHash('sha256').update(filePath, 'utf8').digest('hex');
}

export function isPatchAlreadyApplied(filePath: string, patchFilePath: string): boolean {
    try {
        const root = resolveAgentDocRootForFile(filePath);
        if (!root) return false;
        const snapshotPath = path.join(root, '.agent-doc', 'snapshots', `${docHash(filePath)}.md`);
        if (!fs.existsSync(snapshotPath)) return false;
        return fs.statSync(snapshotPath).mtimeMs > fs.statSync(patchFilePath).mtimeMs;
    } catch {
        return false;
    }
}

export function consumeClaimedPatch(patchId: string | undefined, filePath: string): boolean {
    if (!patchId) return false;
    try {
        const root = resolveAgentDocRootForFile(filePath);
        if (!root) return false;
        const sentinel = path.join(root, '.agent-doc', 'claimed-patches', patchId);
        if (!fs.existsSync(sentinel)) return false;
        return true;
    } catch {
        return false;
    }
}

export interface EditorApplyProof {
    readonly content: string;
    readonly version: number;
}

export function createEditorApplyProof(content: string, version: number): EditorApplyProof {
    return { content, version };
}

export function isEditorApplyProofCurrent(
    proof: EditorApplyProof,
    currentContent: string,
    currentVersion: number,
): boolean {
    return proof.version === currentVersion && proof.content === currentContent;
}
