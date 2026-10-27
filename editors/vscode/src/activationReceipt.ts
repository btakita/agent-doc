/**
 * The single-role `agent_doc_activation_receipt/v1` line this extension host
 * reports in its controller registration (`editors/SPEC.md` section 13).
 *
 * The format is the one `editors/jetbrains/SPEC.md` ("Restart-free
 * implementation generations") designs for the two JetBrains Remote Dev JVMs.
 * A VS Code extension runs whole in one extension host, chosen by
 * `extensionKind` (`["workspace"]`), so it reports one receipt with
 * `role=workspace` and nothing has to be compared across processes:
 *
 * - `jvm` is the reporting process instance: one id per extension activation.
 * - `shell` is the sha256 of the extension bundle the host loaded, so a VSIX
 *   that was installed after activation is not mistaken for the running one.
 * - `spi=0`: the shell reaches its swappable implementation, the native
 *   library, through the C ABI, not through a versioned SPI.
 * - `impl_version` / `impl_sha256` describe the loaded native library, or
 *   `none` before it loads.
 * - `generation` counts native loads in this host. The workspace host is the
 *   only role, so it assigns its own `epoch`, equal to `generation`.
 *
 * Pure apart from `fileSha256`, so the encoding is unit-testable without the
 * VS Code API. Kept in sync with
 * `agent_doc_reliable_sync_io::liveness::ActivationReceipt`.
 */

import * as crypto from 'crypto';
import * as fs from 'fs';

export const ACTIVATION_RECEIPT_PREFIX = 'agent_doc_activation_receipt/v1';
export const WORKSPACE_ACTIVATION_ROLE = 'workspace';
export const WORKSPACE_ACTIVATION_SPI = 0;
const NONE = 'none';

export interface WorkspaceActivation {
    /** Process instance id (wire key `jvm`). */
    instance: string;
    /** Digest of the loaded extension bundle, or null when it was unreadable. */
    shell: string | null;
    implVersion: string | null;
    implSha256: string | null;
    generation: number;
}

/** A receipt value is one non-empty token without spaces, or `none`. */
function token(value: string | null | undefined): string {
    const trimmed = (value ?? '').trim();
    if (!trimmed) return NONE;
    return trimmed.replace(/\s+/g, '_');
}

export function encodeWorkspaceActivationReceipt(activation: WorkspaceActivation): string {
    const generation = Math.max(0, Math.trunc(activation.generation));
    return [
        ACTIVATION_RECEIPT_PREFIX,
        `role=${WORKSPACE_ACTIVATION_ROLE}`,
        `jvm=${token(activation.instance)}`,
        `shell=${token(activation.shell)}`,
        `spi=${WORKSPACE_ACTIVATION_SPI}`,
        `impl_version=${token(activation.implVersion)}`,
        `impl_sha256=${token(activation.implSha256)}`,
        `generation=${generation}`,
        `epoch=${generation}`,
    ].join(' ');
}

/** Hex sha256 of a file, or null when it cannot be read. */
export function fileSha256(filePath: string | null | undefined): string | null {
    if (!filePath) return null;
    try {
        return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
    } catch {
        return null;
    }
}

const instance = crypto.randomUUID();
let shellDigest: string | null = null;

/**
 * Record the digest of the bundle this host loaded. Called once from
 * `activate`, before any registration is sent.
 */
export function recordActivatedShell(bundlePath: string | null | undefined): void {
    shellDigest = fileSha256(bundlePath);
}

/** The receipt for this host, given the native library facts at this moment. */
export function currentWorkspaceActivationReceipt(native: {
    version: string | null;
    sha256: string | null;
    generation: number;
}): string {
    return encodeWorkspaceActivationReceipt({
        instance,
        shell: shellDigest,
        implVersion: native.version,
        implSha256: native.sha256,
        generation: native.generation,
    });
}
