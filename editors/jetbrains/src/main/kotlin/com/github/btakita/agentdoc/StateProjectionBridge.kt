package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.github.lazily.GraphView
import io.github.lazily.IpcMessage
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * Thin editor-side bridge for the Rust-owned state backbone.
 *
 * The plugin only reports observed facts and renders the projection. The durable
 * state machines and stale-generation checks remain in the binary.
 *
 * `#lzsync` 3B clean split: the reactive read path folds the canonical lazily wire
 * (native `IpcMessage` Snapshot/Delta, `NodeId`/`IpcValue`) through a **generic**
 * lazily [GraphView] and layers agent-doc's own [AgentDocProjection] /
 * [AgentDocTurnProjection] domain projections on top. The old base64-`WireDelta`
 * `StateGraphMirror` is retired — agent-doc is a peer product surface over lazily's
 * view, sibling to signal-space's patchboard surface, reading only its own
 * `agent_doc.*` node vocabulary.
 */
object StateProjectionBridge {
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(StateProjectionBridge::class.java)
    private val generations = ConcurrentHashMap<String, AtomicLong>()
    /** Per-document generic materialized view (`#lzsync` 3B), advanced by [subscribeMirrorForFile]. */
    private val mirrors = ConcurrentHashMap<String, GraphView>()

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
     * Subscribe the per-document [GraphView] to native lazily snapshot/delta
     * messages from `agent_doc_state_subscribe` (`#lzsync` 3B).
     *
     * First call requests a cold snapshot (the view is uninitialized); subsequent
     * calls request a delta since the view's current epoch. The view converges
     * identically to a full re-render because the fold is a pure, idempotent replay
     * of deduped events. Returns the applied message kind (`"snapshot"`/`"delta"`)
     * or null when FFI is unavailable / no state yet.
     */
    fun subscribeMirrorForFile(filePath: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val docHash = documentHash(filePath)
        val view = mirrors.computeIfAbsent(docHash) { GraphView() }
        val lastEpoch = if (view.isInitialized) view.epoch else 0L
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
        return if (applyNativeMessage(view, raw)) {
            messageKind(raw)
        } else {
            null
        }
    }

    fun subscribeMirrorForFileViaProjectController(filePath: String, projectRoot: File): String? {
        val docHash = documentHash(filePath)
        val view = mirrors.computeIfAbsent(docHash) { GraphView() }
        val lastEpoch = if (view.isInitialized) view.epoch else 0L
        val response = CpRouteClient.stateSubscribe(
            projectRoot = projectRoot.path,
            filePath = filePath,
            documentHash = docHash,
            lastEpoch = lastEpoch,
        )
        if (response.documentHash != docHash) {
            LOG.debug("[state-projection] Project Controller returned state for ${response.documentHash}, expected $docHash")
            return null
        }
        return if (applyNativeMessage(view, response.messageJson)) {
            messageKind(response.messageJson)
        } else {
            null
        }
    }

    /**
     * Reactive summary derived from the per-document [GraphView]'s folded nodes
     * (`#lzsync` 3B). Call after [subscribeMirrorForFile]. Returns null when the
     * view has not been initialized yet.
     */
    fun mirrorSummaryForFile(filePath: String): AgentDocProjection? {
        val view = mirrors[documentHash(filePath)] ?: return null
        if (!view.isInitialized) return null
        return AgentDocProjection.fromView(view)
    }

    fun mirrorTurnProjectionJsonForFile(filePath: String): String? {
        val view = mirrors[documentHash(filePath)] ?: return null
        if (!view.isInitialized) return null
        return AgentDocTurnProjection.fromView(view).toJsonString()
    }

    /** The current view epoch for [filePath], or null if never initialized. */
    fun mirrorEpochForFile(filePath: String): Long? =
        mirrors[documentHash(filePath)]?.takeIf { it.isInitialized }?.epoch

    /**
     * Reactive read for consumers (`#lzsync` 3B): advance the per-document view by
     * absorbing any FFI deltas since its current epoch, then derive the domain
     * projection from the folded nodes.
     *
     * This is the drop-in replacement for the cold [projectionSummaryForFile] pull
     * on the read path. Recording a fact (e.g. [recordRouteDispatchStarted] /
     * [recordRouteDispatchProven]) appends to the binary's ledger; this call pulls
     * those accepted events forward as a `delta` (or a cold `snapshot` on the first
     * read) so the projection reflects the just-recorded state without a full
     * re-render.
     *
     * Cold-pull fallback is intentional and narrow: when the view never initializes
     * (FFI unavailable, or no recorded state yet), fall back to the cold
     * [projectionSummaryForFile] so a cold-start read still surfaces a projection.
     */
    fun reactiveSummaryForFile(filePath: String): AgentDocProjection? {
        // Drive the view forward (snapshot on first read, delta thereafter).
        subscribeMirrorForFile(filePath)
        mirrorSummaryForFile(filePath)?.let { return it }
        // Cold-start fallback: the view never initialized (no FFI / no state yet).
        return projectionSummaryForFile(filePath)?.let {
            AgentDocProjection(
                routeReadiness = it.routeReadiness,
                routePaneId = it.routePaneId,
                latestTransportPhase = it.latestTransportPhase,
                proofMarkers = it.proofMarkers,
            )
        }
    }

    /**
     * Evict the per-document view and owner-generation counters for [filePath]
     * (`#jbmirrorevict` / `#nsq2`). Called when the editor tab/document closes so a
     * reused path (move/symlink/reopen) does not surface the prior document's stale
     * projection state — the leak behind the "sessions working on other open
     * documents" symptom. The [mirrors] and [generations] maps are keyed by
     * `documentHash` (SHA-256 of the canonical path) and otherwise grow
     * monotonically over the IDE lifetime. Re-subscription lazily re-creates the
     * view from a fresh cold snapshot, so aggressive eviction is safe.
     */
    fun evictForFile(filePath: String) {
        val docHash = documentHash(filePath)
        mirrors.remove(docHash)
        val genPrefix = "$docHash:"
        generations.keys.filter { it.startsWith(genPrefix) }.forEach { generations.remove(it) }
    }

    /** Test-only: live view + generation entry counts (for eviction coverage). */
    internal fun debugEntryCounts(): Pair<Int, Int> = mirrors.size to generations.size

    /**
     * Test-only seam (`#lzsync` 3B): seed the per-document view by applying a native
     * lazily snapshot/delta message directly, bypassing the FFI subscribe call. Lets
     * consumer-observation tests assert that the read path derives the projection
     * from the folded view rather than the cold pull (FFI is unavailable in plugin
     * unit tests). Returns whether the message applied.
     */
    internal fun seedMirrorMessageForTest(filePath: String, message: String): Boolean {
        val view = mirrors.computeIfAbsent(documentHash(filePath)) { GraphView() }
        return applyNativeMessage(view, message)
    }

    /**
     * Fold one native `agent_doc_state_subscribe` message into [view], dispatching
     * on the externally-tagged `IpcMessage` variant (`Snapshot`/`Delta`). Control
     * frames and malformed JSON are ignored. Returns whether a graph message
     * applied.
     */
    private fun applyNativeMessage(view: GraphView, raw: String): Boolean {
        val message = try {
            IpcMessage.decodeJson(raw)
        } catch (e: Exception) {
            LOG.debug("[state-projection] native IpcMessage decode failed: ${e.message}")
            return false
        }
        return when (message) {
            is IpcMessage.SnapshotMessage -> {
                view.applySnapshot(message.snapshot)
                true
            }
            is IpcMessage.DeltaMessage -> {
                view.applyDelta(message.delta)
                true
            }
            else -> false
        }
    }

    /** Detect the native `IpcMessage` variant key (`Snapshot`/`Delta`) → `snapshot`/`delta`. */
    private fun messageKind(raw: String): String? = try {
        val obj = JsonParser.parseString(raw).takeIf { it.isJsonObject }?.asJsonObject
        when {
            obj == null -> null
            obj.has("Snapshot") -> "snapshot"
            obj.has("Delta") -> "delta"
            else -> null
        }
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

    fun recordEditorPatchApplied(filePath: String, patchId: String?, generation: Long?) {
        val patch = patchId ?: return
        val gen = generation ?: return
        val lib = AgentDocLib.get() ?: run {
            LOG.warn("[state-projection] incompatible agent-doc native library: missing lazily receipt ABI")
            return
        }
        try {
            if (lib.agent_doc_editor_patch_applied(filePath, patch, gen) != 1) {
                LOG.warn("[state-projection] editor_patch_applied receipt rejected for patch_id $patch")
            }
        } catch (_: UnsatisfiedLinkError) {
            LOG.warn("[state-projection] incompatible agent-doc native library: missing agent_doc_editor_patch_applied; reinstall the plugin/native library")
        } catch (_: NoSuchMethodError) {
            LOG.warn("[state-projection] incompatible agent-doc native library: missing agent_doc_editor_patch_applied; reinstall the plugin/native library")
        }
    }

    fun recordEditorPatchRejected(filePath: String, patchId: String?, generation: Long?, reason: String) {
        val patch = patchId ?: return
        val gen = generation ?: return
        val lib = AgentDocLib.get() ?: run {
            LOG.warn("[state-projection] incompatible agent-doc native library: missing lazily receipt ABI")
            return
        }
        try {
            if (lib.agent_doc_editor_patch_rejected(filePath, patch, gen, reason) != 1) {
                LOG.warn("[state-projection] editor_patch_rejected receipt rejected for patch_id $patch")
            }
        } catch (_: UnsatisfiedLinkError) {
            LOG.warn("[state-projection] incompatible agent-doc native library: missing agent_doc_editor_patch_rejected; reinstall the plugin/native library")
        } catch (_: NoSuchMethodError) {
            LOG.warn("[state-projection] incompatible agent-doc native library: missing agent_doc_editor_patch_rejected; reinstall the plugin/native library")
        }
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
