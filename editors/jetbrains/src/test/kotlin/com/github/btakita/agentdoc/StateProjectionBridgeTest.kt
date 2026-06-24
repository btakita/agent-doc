package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import java.io.File
import java.security.MessageDigest

class StateProjectionBridgeTest {
    @Test
    fun `document hash uses canonical path sha256`() {
        val file = File.createTempFile("agent-doc-state", ".md")
        try {
            val expected = MessageDigest.getInstance("SHA-256")
                .digest(file.canonicalPath.toByteArray(Charsets.UTF_8))
                .joinToString("") { "%02x".format(it) }
            assertEquals(expected, StateProjectionBridge.documentHash(file.path))
        } finally {
            file.delete()
        }
    }

    @Test
    fun `state event json matches Rust state backbone serde shape`() {
        val json = StateProjectionBridge.stateEventJson(
            documentHash = "doc-a",
            type = "editor_patch_queued",
            fields = mapOf("patch_id" to "patch-1", "actor_generation" to 7),
            eventSuffix = "editor-patch-queued-patch-1-7",
        )

        val root = JsonParser.parseString(json).asJsonObject
        assertEquals("doc-a:editor-patch-queued-patch-1-7", root.get("event_id").asString)
        val fact = root.getAsJsonObject("fact")
        assertEquals("editor_patch_queued", fact.get("type").asString)
        assertEquals("doc-a", fact.get("document_hash").asString)
        assertEquals("patch-1", fact.get("patch_id").asString)
        assertEquals(7, fact.get("actor_generation").asLong)
    }

    @Test
    fun `projection summary renders route transport and proof slices`() {
        val projection = """
            {
              "document_hash":"doc-a",
              "route":{"generation":3,"pane_id":"%2","readiness":"dispatch_proven","dispatch_proofs":["p1"]},
              "transport":{"patches":{"patch-1":{"phase":"queued"},"patch-2":{"phase":"acked"}}},
              "proof":{"markers":{"dispatch_start":{"phase":"observed","sources":["route"]}}},
              "document":{},
              "queue":{},
              "closeout":{},
              "supervisor":{}
            }
        """.trimIndent()

        val summary = StateProjectionBridge.projectionSummary(projection)
        assertNotNull(summary)
        assertEquals("dispatch_proven", summary!!.routeReadiness)
        assertEquals("%2", summary.routePaneId)
        assertEquals("patch-2", summary.latestTransportPatchId)
        assertEquals("acked", summary.latestTransportPhase)
        assertEquals(1, summary.proofMarkers)
        assertEquals(
            "route=dispatch_proven pane=%2 transport=patch-2:acked proof_markers=1",
            summary.compact(),
        )
    }
}
