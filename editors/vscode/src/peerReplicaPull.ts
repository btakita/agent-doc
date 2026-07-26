/**
 * `#ctrlkillreregister` Tier 3 — the editor-side consumer of
 * `agent_doc_peer_replicas_missing`.
 *
 * Killing the controller strands a live editor: hydration restores the durable
 * liveness plane so the editor still reads as *registered*, but the relay hub holding
 * its replica is process-local, died with the old controller, and nothing rehydrates
 * it. The controller used to push a rebuild request at each survivor (Tier 1), and a
 * push has to *reach* its endpoint — the failure behind
 * `reload-lib reached 1/4 endpoints`.
 *
 * The pull inverts it. The extension is the only process that can create its own
 * replica, so it asks the converged liveness plane about *itself* and repairs. There
 * is no endpoint to fail to reach, because the asking process is provably alive — it
 * just called. And it is correct whichever side restarted: a controller that lost its
 * hub, an extension that reconnected, or a registration that arrived after any
 * fan-out already ran.
 *
 * Kept behaviourally identical to the JetBrains `PeerReplicaPull`; both are thin
 * consumers of the one derivation that lives in the binary (FFI-first).
 */

/**
 * Paths this editor must re-register, parsed from the FFI's JSON array of
 * `EditorRegistration` objects.
 *
 * Pure so the decision is testable without a controller: `json` is exactly what
 * `peerReplicasMissing` returns. `null` means "could not ask" (ABI missing,
 * controller unreachable, malformed answer) and is deliberately distinct from `[]`
 * ("asked, nothing to do") — the caller falls back to the compatibility refresh only
 * in the first case, and must do nothing in the second.
 *
 * `pid` re-checks the peer scoping the controller already applied. The controller is
 * the authority on which registrations are stranded, but it is not the authority on
 * which process this is: re-registering another editor's document from here would
 * publish *this* buffer's text over theirs.
 */
export function peerReplicaRebuildPaths(json: string | null, pid: number): string[] | null {
    if (json === null || json === undefined) return null;
    const trimmed = json.trim();
    if (trimmed === '') return null;
    let parsed: unknown;
    try {
        parsed = JSON.parse(trimmed);
    } catch {
        return null;
    }
    if (!Array.isArray(parsed)) return null;
    const paths: string[] = [];
    const seen = new Set<string>();
    for (const entry of parsed) {
        if (typeof entry !== 'object' || entry === null) continue;
        const record = entry as Record<string, unknown>;
        if (record.pid !== pid) continue;
        const path = record.path;
        if (typeof path !== 'string' || path === '') continue;
        if (seen.has(path)) continue;
        seen.add(path);
        paths.push(path);
    }
    return paths;
}
