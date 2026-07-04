/**
 * Pure helpers for the `save-document.signal` handler (#jbeditorsavedrift-vscode).
 *
 * The binary writes `.agent-doc/patches/save-document.signal` (JSON `{ file,
 * patch_id }`) when it detects the editor buffer is ahead of disk after a commit
 * (carry-forward drift). The VS Code PatchWatcher flushes the matching editor
 * buffer to disk and publishes the saved content as a lazily receipt-backed
 * content projection.
 * These helpers hold the side-effect-free parts so they can be unit-tested
 * without the `vscode` API. Mirrors the JB plugin's socket `save_document` path.
 */

export interface SaveDocumentSignal {
    file: string;
    patchId?: string;
}

/**
 * Parse a `save-document.signal` payload. Returns null when the payload is not
 * valid JSON or carries no usable `file` path, so the caller can ignore it.
 */
export function parseSaveDocumentSignal(raw: string): SaveDocumentSignal | null {
    let payload: { file?: unknown; patch_id?: unknown };
    try {
        payload = JSON.parse(raw);
    } catch {
        return null;
    }
    if (typeof payload.file !== 'string' || payload.file.length === 0) {
        return null;
    }
    const patchId = typeof payload.patch_id === 'string' && payload.patch_id.length > 0
        ? payload.patch_id
        : undefined;
    return { file: payload.file, patchId };
}
