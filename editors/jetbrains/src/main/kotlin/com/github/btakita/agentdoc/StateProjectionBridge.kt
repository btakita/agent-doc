package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * Thin editor-side bridge for the Rust-owned state backbone.
 *
 * The plugin only reports observed facts and renders the projection JSON. The
 * durable state machines and stale-generation checks remain in the binary.
 *
 * #lzpkgwire: JetBrains keeps this plugin-local bridge canonical for runtime
 * packaging because the plugin is built with the IntelliJ/Kotlin 1.9/JBR17
 * toolchain while lazily-kt is a standalone Kotlin 2/JVM21 package. Keep the
 * pure helper behavior pinned to lazily-kt's StateProjectionBridgeSupport tests.
 */
object StateProjectionBridge {
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(StateProjectionBridge::class.java)
    private val generations = ConcurrentHashMap<String, AtomicLong>()
    /** Per-document reactive mirror (`#lazilystatesync3`), advanced by [subscribeMirrorForFile]. */
    private val mirrors = ConcurrentHashMap<String, StateGraphMirror>()

    data class ProjectionSummary(
        val routeReadiness: String?,
        val routePaneId: String?,
        val latestTransportPatchId: String?,
        val latestTransportPhase: String?,
        val proofMarkers: Int,
    ) {
        fun compact(): String =
            "route=${routeReadiness ?: "unknown"} pane=${routePaneId ?: "-"} " +
                "transport=${latestTransportPatchId ?: "-"}:${latestTransportPhase ?: "-"} " +
                "proof_markers=$proofMarkers"
    }

    fun documentHash(filePath: String): String {
        val canonical = try {
            File(filePath).canonicalPath
        } catch (_: Exception) {
            File(filePath).absolutePath
        }
        val digest = MessageDigest.getInstance("SHA-256")
        return digest.digest(canonical.toByteArray(Charsets.UTF_8)).joinToString("") {
            "%02x".format(it)
        }
    }

    fun projectionJsonForFile(filePath: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val ptr = try {
            lib.agent_doc_state_projection(documentHash(filePath))
        } catch (e: Throwable) {
            LOG.debug("[state-projection] projection unavailable: ${e.message}")
            return null
        }
        try {
            val raw = ptr?.getString(0) ?: return null
            return raw.takeUnless { it == "null" }
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    fun projectionSummaryForFile(filePath: String): ProjectionSummary? =
        projectionJsonForFile(filePath)?.let(::projectionSummary)

    fun projectionSummary(json: String): ProjectionSummary? = try {
        val root = JsonParser.parseString(json).asJsonObject
        val route = root.getAsJsonObject("route")
        val transport = root.getAsJsonObject("transport")
        val proof = root.getAsJsonObject("proof")
        val patches = transport?.getAsJsonObject("patches")
        val latestPatch = patches
            ?.entrySet()
            ?.maxByOrNull { it.key }
        ProjectionSummary(
            routeReadiness = route?.get("readiness")?.asString,
            routePaneId = route?.get("pane_id")?.asString,
            latestTransportPatchId = latestPatch?.key,
            latestTransportPhase = latestPatch?.value?.asJsonObject?.get("phase")?.asString,
            proofMarkers = proof?.getAsJsonObject("markers")?.entrySet()?.size ?: 0,
        )
    } catch (e: Exception) {
        LOG.debug("[state-projection] projection summary parse failed: ${e.message}")
        null
    }

    /**
     * Subscribe the per-document [StateGraphMirror] to lazily-spec snapshot/delta
     * messages from `agent_doc_state_subscribe` (`#lazilystatesync3`).
     *
     * First call requests a cold snapshot (the mirror is uninitialized);
     * subsequent calls request a delta since the mirror's current epoch. The
     * mirror converges identically to a full re-render because the projection is
     * a pure fold of deduped events. Returns the applied message type
     * (`"snapshot"`/`"delta"`) or null when FFI is unavailable / no state yet.
     */
    fun subscribeMirrorForFile(filePath: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val docHash = documentHash(filePath)
        val mirror = mirrors.computeIfAbsent(docHash) { StateGraphMirror() }
        val lastEpoch = if (mirror.isInitialized) mirror.epoch else 0L
        val ptr = try {
            lib.agent_doc_state_subscribe(docHash, lastEpoch)
        } catch (e: Throwable) {
            LOG.debug("[state-projection] subscribe unavailable: ${e.message}")
            return null
        }
        val raw = try {
            ptr?.getString(0)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
        if (raw == null || raw == "null" || raw.isEmpty()) return null
        return if (mirror.applyMessage(raw)) {
            messageKind(raw)
        } else {
            null
        }
    }

    /**
     * Reactive summary derived from the per-document [StateGraphMirror]'s
     * tracked cells (`#lazilystatesync3`). Call after [subscribeMirrorForFile].
     * Returns null when the mirror has not been initialized yet.
     */
    fun mirrorSummaryForFile(filePath: String): MirrorProjectionSummary? {
        val mirror = mirrors[documentHash(filePath)] ?: return null
        if (!mirror.isInitialized) return null
        return MirrorProjectionSummary.fromMirror(mirror)
    }

    /** The current mirror epoch for [filePath], or null if never initialized. */
    fun mirrorEpochForFile(filePath: String): Long? =
        mirrors[documentHash(filePath)]?.takeIf { it.isInitialized }?.epoch

    private fun messageKind(raw: String): String? = try {
        JsonParser.parseString(raw).takeIf { it.isJsonObject }?.asJsonObject?.get("type")?.asString
    } catch (_: Exception) {
        null
    }

    fun recordEditorPatchQueued(filePath: String, patchId: String?): Long? {
        val patch = patchId ?: return null
        val generation = nextGeneration(filePath, "editor_ipc_bridge")
        recordOwnerGeneration(filePath, "editor_ipc_bridge", generation)
        recordFact(
            filePath,
            "editor_patch_queued",
            mapOf("patch_id" to patch, "actor_generation" to generation),
            eventSuffix = "editor-patch-queued-$patch-$generation",
        )
        return generation
    }

    fun recordEditorAckObserved(filePath: String, patchId: String?, generation: Long?) {
        val patch = patchId ?: return
        val gen = generation ?: return
        recordFact(
            filePath,
            "editor_ack_observed",
            mapOf("patch_id" to patch, "actor_generation" to gen),
            eventSuffix = "editor-ack-$patch-$gen",
        )
    }

    fun recordEditorRetryRequested(filePath: String, patchId: String?, generation: Long?, reason: String) {
        val patch = patchId ?: return
        val gen = generation ?: return
        recordFact(
            filePath,
            "editor_patch_retry_requested",
            mapOf("patch_id" to patch, "actor_generation" to gen, "reason" to reason),
            eventSuffix = "editor-retry-$patch-$gen-${reason.hashCode()}",
        )
    }

    fun recordRouteDispatchStarted(filePath: String, routeKey: String): Long {
        val generation = nextGeneration(filePath, "route_dispatch")
        recordOwnerGeneration(filePath, "route_dispatch", generation)
        recordFact(
            filePath,
            "route_readiness_observed",
            mapOf("actor_generation" to generation, "event" to "dispatch_authorized"),
            eventSuffix = "route-authorized-${routeKey.hashCode()}-$generation",
        )
        return generation
    }

    fun recordRouteDispatchProven(filePath: String, generation: Long, proofId: String) {
        recordFact(
            filePath,
            "route_readiness_observed",
            mapOf("actor_generation" to generation, "event" to "dispatch_accepted"),
            eventSuffix = "route-accepted-$proofId-$generation",
        )
        recordFact(
            filePath,
            "dispatch_proof_observed",
            mapOf("actor_generation" to generation, "proof_id" to proofId),
            eventSuffix = "route-proof-$proofId-$generation",
        )
    }

    fun recordRouteBlocked(filePath: String, generation: Long?, reason: String) {
        val gen = generation ?: return
        recordFact(
            filePath,
            "route_readiness_observed",
            mapOf("actor_generation" to gen, "event" to "blocked"),
            eventSuffix = "route-blocked-$gen-${reason.hashCode()}",
        )
        recordFact(
            filePath,
            "proof_marker_disproved",
            mapOf("marker" to "dispatch_start", "source" to reason.take(160)),
            eventSuffix = "route-proof-disproved-$gen-${reason.hashCode()}",
        )
    }

    private fun nextGeneration(filePath: String, owner: String): Long {
        val key = "${documentHash(filePath)}:$owner"
        return generations.computeIfAbsent(key) { AtomicLong(0) }.incrementAndGet()
    }

    private fun recordOwnerGeneration(filePath: String, owner: String, generation: Long) {
        recordFact(
            filePath,
            "owner_generation_changed",
            mapOf("owner" to owner, "generation" to generation),
            eventSuffix = "owner-$owner-$generation",
        )
    }

    private fun recordFact(
        filePath: String,
        type: String,
        fields: Map<String, Any>,
        eventSuffix: String,
    ): Boolean {
        val lib = AgentDocLib.get() ?: return false
        val documentHash = documentHash(filePath)
        val event = stateEventJson(documentHash, type, fields, eventSuffix)
        return try {
            lib.agent_doc_record_state_event(documentHash, event) == 1
        } catch (e: Throwable) {
            LOG.debug("[state-projection] record_state_event unavailable: ${e.message}")
            false
        }
    }

    fun stateEventJson(
        documentHash: String,
        type: String,
        fields: Map<String, Any>,
        eventSuffix: String,
    ): String {
        val fact = JsonObject()
        fact.addProperty("type", type)
        fact.addProperty("document_hash", documentHash)
        for ((key, value) in fields) {
            when (value) {
                is Boolean -> fact.addProperty(key, value)
                is Number -> fact.addProperty(key, value)
                else -> fact.addProperty(key, value.toString())
            }
        }
        val event = JsonObject()
        event.addProperty("event_id", "$documentHash:$eventSuffix")
        event.add("fact", fact)
        return event.toString()
    }
}
