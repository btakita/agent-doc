package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.github.lazily.WireDelta
import io.github.lazily.WireDeltaOp
import io.github.lazily.WireEdgeSnapshot
import io.github.lazily.WireNodeSnapshot
import io.github.lazily.WireSnapshot
import java.util.Base64

/**
 * Agent-doc node type tags for the reactive state projection (`#lazilystatesync3`).
 *
 * These mirror `agent-doc-orchestration/src/state_wire.rs` 1:1.
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
 *
 * This is the plugin's public adapter type; it is a Gson-friendly projection of
 * the canonical lazily-kt `io.github.lazily.StateGraphMirror.Node`.
 */
data class MirrorNode(
    val slotId: Long,
    val typeTag: String,
    val payload: String?,
)

/**
 * Per-document reactive mirror the JB plugin holds, delegating to the canonical
 * lazily-kt reactive [io.github.lazily.StateGraphMirror] (S5 / `#lzpkgwire`).
 *
 * The binary owns the authoritative reactive graph (`#lazilystatesync2`); this
 * mirror applies the `agent_doc_state_subscribe` snapshot/delta messages so
 * editor UI (dispatch-ready / busy / queue indicators) reads tracked cells
 * instead of re-rendering the full projection JSON on every observed event.
 *
 * The snapshot/delta node-fold now lives in lazily-kt (pinned by its
 * `StateGraphMirrorTest`); this class is a thin adapter that preserves the
 * plugin's public surface (Gson [MirrorNode] / [JsonObject] payloads) so the
 * rest of the plugin ([StateProjectionBridge] and friends) is unchanged.
 *
 * Because the projection is a pure fold of deduped events, delta application is
 * deterministic and idempotent — a no-op delta (re-emit) leaves the mirror
 * unchanged (`#qdedupsync` property).
 */
class StateGraphMirror {
    /** Canonical lazily-kt reactive mirror; owns the node/edge fold. */
    private val inner = io.github.lazily.StateGraphMirror()

    /** Monotonic frontier — the highest lazily-spec epoch applied so far. */
    val epoch: Long get() = inner.epoch

    val documentHash: String? get() = inner.documentHash

    /** True until at least one snapshot/delta has been applied. */
    val isInitialized: Boolean get() = inner.isInitialized

    val nodeCount: Int get() = inner.nodeCount

    /**
     * Apply a raw `agent_doc_state_subscribe` message, dispatching on the
     * lazily-spec `"type"` discriminator (`snapshot`/`delta`).
     *
     * lazily-kt's `decodeSubscribe`/`Json` decode path is `internal`, so the wire
     * is parsed here with the plugin's existing Gson (avoiding a kotlinx
     * serialization runtime dependency on the hot path) and folded into [inner]
     * through the public `applySnapshot`/`applyDelta` API. Returns false when the
     * message cannot be parsed or carries an unknown discriminator.
     */
    fun applyMessage(raw: String): Boolean {
        val root = try {
            JsonParser.parseString(raw).takeIf { it.isJsonObject }?.asJsonObject
        } catch (_: Exception) {
            null
        } ?: return false
        return when (root.get("type")?.takeIf { it.isJsonPrimitive }?.asString) {
            "snapshot" -> { inner.applySnapshot(parseSnapshot(root)); true }
            "delta" -> { inner.applyDelta(parseDelta(root)); true }
            else -> false
        }
    }

    /** All tracked nodes of [typeTag] (stable insertion order). */
    fun nodesOfType(typeTag: String): List<MirrorNode> =
        inner.nodesOfType(typeTag).map { MirrorNode(it.slotId, it.typeTag, it.payload) }

    /** The single document-level node for [typeTag], or null. */
    fun singletonNode(typeTag: String): MirrorNode? =
        inner.singletonNode(typeTag)?.let { MirrorNode(it.slotId, it.typeTag, it.payload) }

    /**
     * Decode a node payload (`base64(serde_json(struct))`) as a Gson JSON object,
     * or null when the node is missing or its payload is unset.
     */
    fun payloadObject(typeTag: String): JsonObject? =
        singletonNode(typeTag)?.payload?.let { decodePayload(it) }

    /** Build a lazily-kt [WireSnapshot] from the Gson-parsed snapshot message. */
    private fun parseSnapshot(root: JsonObject): WireSnapshot {
        val nodes = root.getAsJsonArray("nodes")?.map { element ->
            val node = element.asJsonObject
            WireNodeSnapshot(
                slotId = node.get("slot_id").asLong,
                typeTag = node.get("type_tag").asString,
                state = node.get("state")?.takeIf { !it.isJsonNull }?.asString ?: "resolved",
                payload = node.get("payload")?.takeIf { !it.isJsonNull }?.asString,
            )
        } ?: emptyList()
        val edges = root.getAsJsonArray("edges")?.map { element ->
            val edge = element.asJsonObject
            WireEdgeSnapshot(edge.get("dependent").asLong, edge.get("dependency").asLong)
        } ?: emptyList()
        val roots = root.getAsJsonArray("roots")?.map { it.asLong } ?: emptyList()
        return WireSnapshot(
            epoch = root.get("epoch")?.asLong ?: 0L,
            documentHash = root.get("document_hash")?.asString ?: "",
            nodes = nodes,
            edges = edges,
            roots = roots,
        )
    }

    /** Build a lazily-kt [WireDelta] from the Gson-parsed delta message. */
    private fun parseDelta(root: JsonObject): WireDelta {
        val ops = root.getAsJsonArray("ops")?.mapNotNull { element ->
            parseOp(element.asJsonObject)
        } ?: emptyList()
        return WireDelta(
            baseEpoch = root.get("base_epoch")?.asLong ?: 0L,
            epoch = root.get("epoch")?.asLong ?: 0L,
            documentHash = root.get("document_hash")?.asString ?: "",
            ops = ops,
        )
    }

    /** Translate one agent-doc delta op object into a lazily-kt [WireDeltaOp]. */
    private fun parseOp(op: JsonObject): WireDeltaOp? {
        val slot: () -> Long = { op.get("slot_id").asLong }
        val payload: () -> String = { op.get("payload").asString }
        return when (op.get("op")?.takeIf { it.isJsonPrimitive }?.asString) {
            "cell_set" -> WireDeltaOp.CellSet(slot(), payload())
            "slot_value" -> WireDeltaOp.SlotValue(slot(), payload())
            "invalidate" -> WireDeltaOp.Invalidate(slot())
            "node_add" -> WireDeltaOp.NodeAdd(
                slot(),
                op.get("type_tag").asString,
                op.get("payload")?.takeIf { !it.isJsonNull }?.asString,
            )
            "node_remove" -> WireDeltaOp.NodeRemove(slot())
            "edge_add" -> WireDeltaOp.EdgeAdd(op.get("dependent").asLong, op.get("dependency").asLong)
            "edge_remove" -> WireDeltaOp.EdgeRemove(op.get("dependent").asLong, op.get("dependency").asLong)
            else -> null
        }
    }

    companion object {
        /** Decode a `base64(serde_json(struct))` payload to a Gson JSON object, or null on failure. */
        fun decodePayload(payload: String?): JsonObject? {
            if (payload.isNullOrEmpty()) return null
            return try {
                val decoded = String(Base64.getDecoder().decode(payload), Charsets.UTF_8)
                JsonParser.parseString(decoded).takeIf { it.isJsonObject }?.asJsonObject
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
