package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.github.lazily.GraphView

/**
 * agent-doc state node `type_tag`s for the reactive projection (`#lzsync` 3B).
 *
 * These mirror `agent-doc-orchestration/src/state_wire.rs` 1:1 and are the single
 * source of the agent-doc node vocabulary the domain projections fold from a
 * generic lazily [GraphView].
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
 * agent-doc's document-lifecycle inspector projection, derived from a generic lazily
 * [GraphView] (`#lzsync` 3B clean split).
 *
 * This is the agent-doc-owned half that used to be welded into lazily's
 * `StateGraphMirror` (`MirrorProjectionSummary`). The clean split puts the generic
 * materialized view (`GraphView`) in lazily and this **domain** projection here:
 * agent-doc is a *peer product surface* over lazily's view, sibling to signal-space's
 * patchboard surface — it reads its own `agent_doc.*` node vocabulary, nothing generic.
 *
 * Node payloads on the native wire are the component JSON bytes
 * (`IpcValue::Inline` / `NodeState::Payload`), stored verbatim by the view — no base64.
 */
data class AgentDocProjection(
    val routeReadiness: String?,
    val routePaneId: String?,
    val latestTransportPhase: String?,
    val proofMarkers: Int,
) {
    fun compact(): String =
        "route=${routeReadiness ?: "unknown"} pane=${routePaneId ?: "-"} " +
            "transport=${latestTransportPhase ?: "-"} proof_markers=$proofMarkers"

    companion object {
        /** Derive the agent-doc inspector projection from a folded [GraphView]. */
        fun fromView(view: GraphView): AgentDocProjection {
            val route = payloadJson(view.singletonNode(AgentDocNodeType.ROUTE)?.payload)
            val latestPatch = view.nodesOfType(AgentDocNodeType.TRANSPORT_PATCH).maxByOrNull { it.id }
            return AgentDocProjection(
                routeReadiness = route.stringField("readiness"),
                routePaneId = route.stringField("pane_id"),
                latestTransportPhase = payloadJson(latestPatch?.payload).stringField("phase"),
                proofMarkers = view.nodesOfType(AgentDocNodeType.PROOF_MARKER).size,
            )
        }
    }
}

/**
 * agent-doc's turn-state projection (idle / awaiting_response / persisting),
 * derived from the `agent_doc.closeout.cycle` node's `phase` on a folded
 * [GraphView] (`#lzsync` 3B clean split). The Project Controller owns the phase;
 * this is the plugin's read-only view of it, symmetric with the VS Code
 * `agentDocTurnProjectionFromView`.
 */
data class AgentDocTurnProjection(
    val state: String,
    val turnInFlight: Boolean,
    val realtimeSteering: JsonObject? = null,
) {
    fun toJsonString(): String {
        val root = JsonObject()
        root.addProperty("state", state)
        root.addProperty("turn_in_flight", turnInFlight)
        root.addProperty("transition_authority", "project_controller")
        if (turnInFlight && realtimeSteering != null) {
            root.add("realtime_steering", realtimeSteering)
        }
        return root.toString()
    }

    companion object {
        /** Derive the turn projection from a folded [GraphView]. */
        fun fromView(view: GraphView): AgentDocTurnProjection {
            val closeout = payloadJson(view.singletonNode(AgentDocNodeType.CLOSEOUT_CYCLE)?.payload)
            return fromPhase(
                closeout.stringField("phase"),
                closeout?.getAsJsonObject("realtime_steering"),
            )
        }

        fun fromPhase(phase: String?, realtimeSteering: JsonObject? = null): AgentDocTurnProjection = when (phase) {
            "preflight_started" -> AgentDocTurnProjection("awaiting_response", true, realtimeSteering)
            "response_captured", "write_applied" -> AgentDocTurnProjection("persisting", true, realtimeSteering)
            else -> AgentDocTurnProjection("idle", false)
        }
    }
}

/** Raw component payload bytes (the native `Inline` JSON) → a JSON object, or null. */
private fun payloadJson(bytes: ByteArray?): JsonObject? {
    if (bytes == null || bytes.isEmpty()) return null
    return try {
        JsonParser.parseString(String(bytes, Charsets.UTF_8)).asJsonObject
    } catch (_: Exception) {
        null
    }
}

private fun JsonObject?.stringField(key: String): String? =
    this?.get(key)?.takeIf { it.isJsonPrimitive }?.asString
