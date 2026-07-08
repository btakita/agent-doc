package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.util.Base64

/**
 * Plugin-local lazily-spec wire shapes consumed by [StateGraphMirror]
 * (`#lazilystatesync3`).
 *
 * These mirror `agent-doc-orchestration/src/state_wire.rs` and the lazily-kt
 * `StateGraphMirror.kt` conformance pin 1:1. Per `#lzpkgwire`, the JB plugin
 * keeps a plugin-local canonical copy because the plugin is built with the
 * IntelliJ/Kotlin 1.9/JBR17 toolchain while lazily-kt is a standalone Kotlin
 * 2/JVM21 package. Pure apply behavior stays pinned to lazily-kt's
 * `StateGraphMirrorTest`, so a drift here is a review catch, not a silent skew.
 *
 * Parsing uses Gson (the plugin's existing JSON library); field names match the
 * Rust serde snake_case shape directly.
 */
internal object AgentDocNodeType {
    const val ROUTE = "agent_doc.route"
    const val QUEUE = "agent_doc.queue"
    const val QUEUE_HEAD = "agent_doc.queue.head"
    const val CLOSEOUT_CYCLE = "agent_doc.closeout.cycle"
    const val TRANSPORT_PATCH = "agent_doc.transport.patch"
    const val SUPERVISOR_OWNER = "agent_doc.supervisor.owner"
    const val DOCUMENT_BASELINE = "agent_doc.document.baseline"
    const val DOCUMENT_AUTHORITY = "agent_doc.document.authority"
    const val PROOF_MARKER = "agent_doc.proof.marker"
}

/**
 * One tracked cell in the plugin mirror graph.
 * `payload` is `base64(serde_json(struct))`, exactly as the FFI emits it.
 */
data class MirrorNode(
    val slotId: Long,
    val typeTag: String,
    val payload: String?,
)

/**
 * The pure, FFI-free mirror graph the JB plugin holds per document.
 *
 * The binary owns the authoritative reactive graph (`#lazilystatesync2`);
 * this mirror applies the `agent_doc_state_subscribe` snapshot/delta messages
 * so editor UI (dispatch-ready / busy / queue indicators) reads tracked cells
 * instead of re-rendering the full projection JSON on every observed event.
 *
 * Because the projection is a pure fold of deduped events, delta application is
 * deterministic and idempotent — a no-op delta (re-emit) leaves the mirror
 * unchanged (`#qdedupsync` property).
 */
class StateGraphMirror {
    private val nodes = LinkedHashMap<Long, MirrorNode>()
    private val edges = LinkedHashSet<Pair<Long, Long>>()

    /** Monotonic frontier — the highest lazily-spec epoch applied so far. */
    var epoch: Long = 0
        private set

    val documentHash: String? get() = declaredHash
    private var declaredHash: String? = null

    /** True until at least one snapshot/delta has been applied. */
    val isInitialized: Boolean get() = declaredHash != null

    val nodeCount: Int get() = nodes.size

    /** Apply a cold-read snapshot JSON, replacing the whole graph image. */
    fun applySnapshotJson(raw: String): Boolean {
        val root = JsonParser.parseString(raw).takeIf { it.isJsonObject }?.asJsonObject ?: return false
        declaredHash = root.get("document_hash")?.asString ?: declaredHash
        nodes.clear()
        edges.clear()
        root.getAsJsonArray("nodes")?.forEach { element ->
            val node = element.asJsonObject
            val slotId = node.get("slot_id").asLong
            val typeTag = node.get("type_tag").asString
            val payload = node.get("payload")?.takeIf { !it.isJsonNull }?.asString
            nodes[slotId] = MirrorNode(slotId, typeTag, payload)
        }
        root.getAsJsonArray("edges")?.forEach { element ->
            val edge = element.asJsonObject
            edges.add(edge.get("dependent").asLong to edge.get("dependency").asLong)
        }
        epoch = root.get("epoch")?.asLong ?: epoch
        return true
    }

    /**
     * Apply a warm delta JSON. Ops are applied verbatim in emission order; the
     * frontier advances to the delta's epoch. A no-op delta (empty ops) only
     * advances the epoch.
     */
    fun applyDeltaJson(raw: String): Boolean {
        val root = JsonParser.parseString(raw).takeIf { it.isJsonObject }?.asJsonObject ?: return false
        root.get("document_hash")?.asString?.let { declaredHash = it }
        root.getAsJsonArray("ops")?.forEach { element ->
            val op = element.asJsonObject
            val opType = op.get("op").asString
            when (opType) {
                "node_add" -> {
                    val slotId = op.get("slot_id").asLong
                    val typeTag = op.get("type_tag").asString
                    val payload = op.get("payload")?.takeIf { !it.isJsonNull }?.asString
                    nodes[slotId] = MirrorNode(slotId, typeTag, payload)
                }
                "cell_set", "slot_value" -> {
                    val slotId = op.get("slot_id").asLong
                    val payload = op.get("payload")?.takeIf { !it.isJsonNull }?.asString
                    nodes[slotId]?.let { nodes[slotId] = it.copy(payload = payload) }
                }
                "invalidate" -> { /* derived recompute is plugin-side; mirror holds the stale payload until a cell_set arrives */ }
                "node_remove" -> {
                    val slotId = op.get("slot_id").asLong
                    nodes.remove(slotId)
                }
                "edge_add" -> edges.add(op.get("dependent").asLong to op.get("dependency").asLong)
                "edge_remove" -> edges.remove(op.get("dependent").asLong to op.get("dependency").asLong)
            }
        }
        epoch = maxOf(epoch, root.get("epoch")?.asLong ?: epoch)
        return true
    }

    /**
     * Apply a raw `agent_doc_state_subscribe` message, dispatching on the
     * lazily-spec `"type"` discriminator (`snapshot` or `delta`).
     * Returns false when the message cannot be parsed.
     */
    fun applyMessage(raw: String): Boolean {
        val root = JsonParser.parseString(raw).takeIf { it.isJsonObject }?.asJsonObject ?: return false
        val type = root.get("type")?.asString ?: return false
        return when (type) {
            "snapshot" -> applySnapshotJson(raw)
            "delta" -> applyDeltaJson(raw)
            else -> false
        }
    }

    /** All tracked nodes of [typeTag] (stable insertion order). */
    fun nodesOfType(typeTag: String): List<MirrorNode> =
        nodes.values.filter { it.typeTag == typeTag }

    /** The single document-level node for [typeTag], or null. */
    fun singletonNode(typeTag: String): MirrorNode? =
        nodes.values.firstOrNull { it.typeTag == typeTag }

    /**
     * Decode a node payload (`base64(serde_json(struct))`) as a JSON object, or
     * null when the node is missing or its payload is unset.
     */
    fun payloadObject(typeTag: String): JsonObject? {
        val node = singletonNode(typeTag) ?: return null
        return decodePayload(node.payload)
    }

    companion object {
        /** Decode a `base64(serde_json(struct))` payload to a JSON object, or null on failure. */
        fun decodePayload(payload: String?): JsonObject? {
            if (payload.isNullOrEmpty()) return null
            return try {
                val json = String(Base64.getDecoder().decode(payload), Charsets.UTF_8)
                JsonParser.parseString(json).takeIf { it.isJsonObject }?.asJsonObject
            } catch (_: Exception) {
                null
            }
        }
    }
}

/**
 * Reactive projection summary derived from a [StateGraphMirror]'s tracked cells
 * instead of re-parsing the full projection JSON (`#lazilystatesync3`).
 *
 * Reads `agent_doc.route`, `agent_doc.transport.patch`, and
 * `agent_doc.proof.marker` nodes. The wire node does not carry the transport
 * patch `entity_key` (patch_id); the binary's full-snapshot `transport` section
 * does, so [latestTransportPatchId] stays null from the warm path (the cold-read
 * [StateProjectionBridge.projectionSummary] supplies it). The phase — the
 * signal that drives busy/dispatch-ready — is always available.
 */
data class MirrorProjectionSummary(
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

    companion object {
        fun fromMirror(mirror: StateGraphMirror): MirrorProjectionSummary {
            val route = mirror.payloadObject(AgentDocNodeType.ROUTE)
            val readiness = route?.stringField("readiness")
            val paneId = route?.stringField("pane_id")

            val patches = mirror.nodesOfType(AgentDocNodeType.TRANSPORT_PATCH)
            val latest = patches.maxByOrNull { it.slotId }
            val phase = latest?.payload?.let(StateGraphMirror::decodePayload)?.stringField("phase")

            val proofMarkers = mirror.nodesOfType(AgentDocNodeType.PROOF_MARKER).size

            return MirrorProjectionSummary(
                routeReadiness = readiness,
                routePaneId = paneId,
                latestTransportPatchId = null,
                latestTransportPhase = phase,
                proofMarkers = proofMarkers,
            )
        }
    }
}

data class MirrorTurnProjection(
    val state: String,
    val turnInFlight: Boolean,
) {
    fun toJsonString(): String {
        val root = JsonObject()
        root.addProperty("state", state)
        root.addProperty("turn_in_flight", turnInFlight)
        root.addProperty("transition_authority", "project_controller")
        return root.toString()
    }

    companion object {
        fun fromMirror(mirror: StateGraphMirror): MirrorTurnProjection {
            val closeout = mirror.payloadObject(AgentDocNodeType.CLOSEOUT_CYCLE)
            return fromPhase(closeout?.stringField("phase"))
        }

        fun fromPhase(phase: String?): MirrorTurnProjection = when (phase) {
            "preflight_started" -> MirrorTurnProjection("awaiting_response", true)
            "response_captured", "write_applied" -> MirrorTurnProjection("persisting", true)
            else -> MirrorTurnProjection("idle", false)
        }
    }
}

private fun JsonObject.stringField(key: String): String? =
    this.get(key)?.takeIf { it.isJsonPrimitive && it.asJsonPrimitive.isString }?.asString
