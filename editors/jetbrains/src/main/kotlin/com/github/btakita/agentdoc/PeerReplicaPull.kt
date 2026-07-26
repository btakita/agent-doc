package com.github.btakita.agentdoc

import com.google.gson.JsonParser

/**
 * `#ctrlkillreregister` Tier 3 — the editor-side consumer of
 * `agent_doc_peer_replicas_missing`.
 *
 * Killing the controller strands a live editor: hydration restores the durable
 * liveness plane so the editor still reads as *registered*, but the relay hub holding
 * its replica is process-local, died with the old controller, and nothing rehydrates
 * it. Until now the controller pushed a rebuild request at each surviving editor
 * (Tier 1) — and a push has to *reach* its endpoint, which is the failure behind
 * `reload-lib reached 1/4 endpoints`.
 *
 * The pull inverts it. The editor is the only process that can create its own
 * replica, so it asks the converged liveness plane about *itself* and repairs. There
 * is no endpoint to fail to reach, because the asking process is provably alive — it
 * just called. And it is correct whichever side restarted: a controller that lost its
 * hub, an editor that reconnected, or a registration that arrived after any fan-out
 * already ran.
 *
 * A blind refresh of every open document is not an acceptable substitute. It
 * re-registers healthy replicas — dropping and rebuilding a live CRDT baseline is the
 * expensive, lossy operation this plugin spends the most care avoiding — and it still
 * misses documents this editor registered but does not currently have open.
 */
internal object PeerReplicaPull {
    /**
     * Paths this editor must re-register, parsed from the FFI's JSON array of
     * `EditorRegistration` objects.
     *
     * Pure so the decision is testable without a controller: [json] is exactly what
     * `agent_doc_peer_replicas_missing` returns. `null` means "could not ask" (ABI
     * missing, controller unreachable, malformed answer) and is deliberately distinct
     * from `[]` ("asked, nothing to do") — the caller falls back to the compatibility
     * refresh only in the first case, and must do nothing in the second.
     *
     * [pid] re-checks the peer scoping the controller already applied. The controller
     * is the authority on which registrations are stranded, but it is not the
     * authority on which process this is: re-registering another editor's document
     * from here would publish *this* buffer's text over theirs.
     */
    fun rebuildPaths(json: String?, pid: Long): List<String>? {
        if (json == null) return null
        val trimmed = json.trim()
        if (trimmed.isEmpty()) return null
        val array = try {
            JsonParser.parseString(trimmed).takeIf { it.isJsonArray }?.asJsonArray ?: return null
        } catch (_: Exception) {
            return null
        }
        val paths = LinkedHashSet<String>()
        for (element in array) {
            val entry = element.takeIf { it.isJsonObject }?.asJsonObject ?: continue
            val entryPid = entry.get("pid")?.takeIf { it.isJsonPrimitive }?.asLong ?: continue
            if (entryPid != pid) continue
            val path = entry.get("path")?.takeIf { it.isJsonPrimitive }?.asString ?: continue
            if (path.isNotEmpty()) paths.add(path)
        }
        return paths.toList()
    }

    /**
     * Ask the controller which of this editor's registrations lack a replica.
     *
     * [heldDocumentHashes] is what this editor believes it already holds; the
     * controller subtracts what it can actually serve, so a healthy document is never
     * named. Returns null when the question could not be asked at all.
     */
    fun missingRegistrationsJson(
        projectRoot: String,
        pid: Long,
        heldDocumentHashes: Collection<String>,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val heldJson = heldDocumentHashes.joinToString(
            separator = ",",
            prefix = "[",
            postfix = "]",
        ) { "\"${it.replace("\\", "\\\\").replace("\"", "\\\"")}\"" }
        val ptr = try {
            lib.agent_doc_peer_replicas_missing(projectRoot, pid, heldJson)
        } catch (_: UnsatisfiedLinkError) {
            // An older cdylib without the export. The controller's Tier 1 fan-out is
            // still in place for exactly this case, so the caller falls back rather
            // than leaving the editor stranded.
            null
        } ?: return null
        return try {
            ptr.getString(0)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }
}
