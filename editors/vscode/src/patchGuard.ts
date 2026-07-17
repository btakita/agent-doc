import * as crypto from 'crypto';

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

export function contentSha256Hex(content: string): string {
    return crypto.createHash('sha256').update(content, 'utf8').digest('hex');
}

export function isFullContentExpectedBufferCurrent(
    currentContent: string,
    expectedHash?: string,
    expectedLen?: number,
): boolean {
    if (!expectedHash) return true;
    if (expectedLen !== undefined && Buffer.byteLength(currentContent, 'utf8') !== expectedLen) {
        return false;
    }
    return contentSha256Hex(currentContent) === expectedHash;
}
