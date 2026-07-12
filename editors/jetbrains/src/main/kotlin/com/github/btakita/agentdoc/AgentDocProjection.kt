package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import io.github.lazily.GraphView

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
        const val ROUTE = "agent_doc.route"
        const val TRANSPORT_PATCH = "agent_doc.transport.patch"
        const val PROOF_MARKER = "agent_doc.proof.marker"

        /** Derive the agent-doc inspector projection from a folded [GraphView]. */
        fun fromView(view: GraphView): AgentDocProjection {
            val route = payloadJson(view.singletonNode(ROUTE)?.payload)
            val latestPatch = view.nodesOfType(TRANSPORT_PATCH).maxByOrNull { it.id }
            return AgentDocProjection(
                routeReadiness = route?.stringField("readiness"),
                routePaneId = route?.stringField("pane_id"),
                latestTransportPhase = payloadJson(latestPatch?.payload)?.stringField("phase"),
                proofMarkers = view.nodesOfType(PROOF_MARKER).size,
            )
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

        private fun JsonObject.stringField(key: String): String? =
            this.get(key)?.takeIf { it.isJsonPrimitive }?.asString
    }
}
