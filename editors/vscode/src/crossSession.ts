// Parser for the structured cross-session-reject marker emitted by
// `agent-doc claim` (claim.rs `cross_session_reject_marker`). Kept free of the
// `vscode` module so it can be unit-tested under plain `node --test`.

export interface CrossSessionReject {
    paneId: string;
    paneSession: string;
    configured: string;
}

export interface CrossSessionClaimOptions {
    force?: boolean;
    newPane?: boolean;
}

export const CROSS_SESSION_REJECT_MARKER = '[claim] cross-session-reject';

/**
 * Parse the cross-session-reject marker out of merged claim stdout/stderr.
 * Returns undefined when no marker is present or any field is missing. The
 * marker field order is stable (`pane_id`, `pane_session`, `configured`) but
 * this parser does not assume it.
 */
export function parseCrossSessionReject(output: string): CrossSessionReject | undefined {
    const line = output.split('\n').find((l) => l.includes(CROSS_SESSION_REJECT_MARKER));
    if (!line) return undefined;
    const tail = line.slice(line.indexOf(CROSS_SESSION_REJECT_MARKER) + CROSS_SESSION_REJECT_MARKER.length).trim();
    const fields: Record<string, string> = {};
    for (const tok of tail.split(/\s+/)) {
        const eq = tok.indexOf('=');
        if (eq > 0) fields[tok.slice(0, eq)] = tok.slice(eq + 1);
    }
    if (!fields.pane_id || !fields.pane_session || !fields.configured) return undefined;
    return {
        paneId: fields.pane_id,
        paneSession: fields.pane_session,
        configured: fields.configured,
    };
}

/** Build the binary recovery command without mixing new-pane and reuse targeting. */
export function buildCrossSessionClaimArgs(
    rel: string,
    position: string | undefined,
    opts: CrossSessionClaimOptions,
): string[] {
    const args = ['claim', rel];
    if (opts.newPane) {
        args.push('--new-pane');
    } else {
        if (opts.force) args.push('--force');
        if (position) args.push('--position', position);
    }
    return args;
}
