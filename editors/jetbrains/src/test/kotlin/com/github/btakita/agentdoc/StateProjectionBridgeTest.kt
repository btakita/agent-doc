package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import io.github.lazily.GraphView
import io.github.lazily.IpcMessage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
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
    fun `cold projection summary renders route transport and proof slices`() {
        // Cold FFI-projection pull (the `agent_doc_state_projection` JSON), kept as
        // the cold-start fallback in reactiveSummaryForFile.
        val projection = """
            {
              "document_hash":"doc-a",
              "route":{"generation":3,"pane_id":"%2","readiness":"dispatch_proven","dispatch_proofs":["p1"]},
              "transport":{"patches":{"patch-1":{"phase":"queued"},"patch-2":{"phase":"applied"}}},
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
        assertEquals("applied", summary.latestTransportPhase)
        assertEquals(1, summary.proofMarkers)
    }

    // --- Native GraphView fold + AgentDocProjection tests (`#lzsync` 3B) --------
    // The reactive read path folds the canonical lazily wire (native `IpcMessage`
    // Snapshot/Delta, `NodeId`/`IpcValue`) through a generic GraphView and layers
    // AgentDocProjection on top. The fold must converge identically to a snapshot
    // after applying the corresponding delta stream, and a no-op delta must be safe.

    /** Component JSON bytes → native `Payload`/`Inline` byte-array literal. */
    private fun bytesOf(json: String): String =
        json.toByteArray(Charsets.UTF_8).joinToString(",", "[", "]")

    private fun snapshotMessage(
        epoch: Long,
        routePayload: String? = null,
        closeoutPayload: String? = null,
        transportPatches: List<Pair<Long, String>> = emptyList(),
        proofMarkers: List<Long> = emptyList(),
    ): String {
        val nodes = mutableListOf<String>()
        fun addNode(id: Long, typeTag: String, payload: String?) {
            val state = if (payload != null) """{"Payload":${bytesOf(payload)}}""" else "\"Opaque\""
            nodes.add("""{"node":$id,"type_tag":"$typeTag","state":$state}""")
        }
        if (routePayload != null) addNode(11, AgentDocNodeType.ROUTE, routePayload)
        if (closeoutPayload != null) addNode(21, AgentDocNodeType.CLOSEOUT_CYCLE, closeoutPayload)
        transportPatches.forEach { (id, payload) -> addNode(id, AgentDocNodeType.TRANSPORT_PATCH, payload) }
        proofMarkers.forEach { id -> addNode(id, AgentDocNodeType.PROOF_MARKER, "{}") }
        return """{"Snapshot":{"epoch":$epoch,"nodes":[${nodes.joinToString(",")}],"edges":[],"roots":[]}}"""
    }

    private fun deltaMessage(baseEpoch: Long, epoch: Long, opsJson: String): String =
        """{"Delta":{"base_epoch":$baseEpoch,"epoch":$epoch,"ops":[$opsJson]}}"""

    private fun cellSetOp(node: Long, payload: String): String =
        """{"CellSet":{"node":$node,"payload":{"Inline":${bytesOf(payload)}}}}"""

    private fun nodeAddOp(node: Long, typeTag: String, payload: String): String =
        """{"NodeAdd":{"node":$node,"type_tag":"$typeTag","state":{"Payload":${bytesOf(payload)}}}}"""

    private fun nodeRemoveOp(node: Long): String = """{"NodeRemove":{"node":$node}}"""

    /** Fold a native message into a fresh GraphView (mirrors the bridge's private apply). */
    private fun folded(vararg messages: String): GraphView {
        val view = GraphView()
        for (raw in messages) {
            when (val message = IpcMessage.decodeJson(raw)) {
                is IpcMessage.SnapshotMessage -> view.applySnapshot(message.snapshot)
                is IpcMessage.DeltaMessage -> view.applyDelta(message.delta)
                else -> error("unexpected message: $message")
            }
        }
        return view
    }

    @Test
    fun `cold snapshot then delta converges identically to direct snapshot`() {
        val routeFinal = """{"readiness":"dispatch_proven","pane_id":"%2"}"""

        val direct = folded(
            snapshotMessage(epoch = 3, routePayload = routeFinal, proofMarkers = listOf(50)),
        )
        val incremental = folded(
            snapshotMessage(epoch = 1, routePayload = """{"readiness":"idle","pane_id":"%2"}"""),
            deltaMessage(
                baseEpoch = 1,
                epoch = 3,
                opsJson = cellSetOp(11, routeFinal) + "," +
                    nodeAddOp(50, AgentDocNodeType.PROOF_MARKER, "{}"),
            ),
        )

        assertEquals(direct.epoch, incremental.epoch)
        assertEquals(
            AgentDocProjection.fromView(direct).compact(),
            AgentDocProjection.fromView(incremental).compact(),
        )
        assertEquals("dispatch_proven", AgentDocProjection.fromView(incremental).routeReadiness)
    }

    @Test
    fun `derive projection reads route phase and proof count from folded nodes`() {
        val view = folded(
            snapshotMessage(
                epoch = 4,
                routePayload = """{"readiness":"dispatch_proven","pane_id":"%2"}""",
                transportPatches = listOf(
                    40L to """{"phase":"applied","actor_generation":1}""",
                    41L to """{"phase":"queued","actor_generation":1}""",
                ),
                proofMarkers = listOf(50L),
            ),
        )

        val projection = AgentDocProjection.fromView(view)
        assertEquals("dispatch_proven", projection.routeReadiness)
        assertEquals("%2", projection.routePaneId)
        // Latest transport patch by node id wins.
        assertEquals("queued", projection.latestTransportPhase)
        assertEquals(1, projection.proofMarkers)
    }

    @Test
    fun `no-op delta is safe and node_remove is honored`() {
        val view = folded(snapshotMessage(epoch = 2, routePayload = """{"readiness":"idle","pane_id":"%2"}"""))
        val beforeEpoch = view.epoch

        // No-op delta: caller is current.
        view.applyDelta((IpcMessage.decodeJson(deltaMessage(2, 2, "")) as IpcMessage.DeltaMessage).delta)
        assertEquals(beforeEpoch, view.epoch)

        // Node remove.
        view.applyDelta(
            (IpcMessage.decodeJson(deltaMessage(2, 3, nodeRemoveOp(11))) as IpcMessage.DeltaMessage).delta,
        )
        assertEquals(3, view.epoch)
        assertNull(view.singletonNode(AgentDocNodeType.ROUTE))
    }

    @Test
    fun `empty view yields nullish projection`() {
        val projection = AgentDocProjection.fromView(GraphView())
        assertNull(projection.routeReadiness)
        assertNull(projection.latestTransportPhase)
        assertEquals(0, projection.proofMarkers)
    }

    @Test
    fun `turn projection derives from closeout cycle phase`() {
        val awaiting = folded(
            snapshotMessage(
                epoch = 1,
                closeoutPayload = """{"phase":"preflight_started","realtime_steering":{"state":"prompt_target","count":2,"preview":"First edit","verbatim":"First edit\n\nSecond edit"}}""",
            ),
        )
        val awaitingProjection = AgentDocTurnProjection.fromView(awaiting)
        assertEquals("awaiting_response", awaitingProjection.state)
        assertTrue(awaitingProjection.turnInFlight)
        val awaitingJson = JsonParser.parseString(awaitingProjection.toJsonString()).asJsonObject
        assertEquals(2, awaitingJson.getAsJsonObject("realtime_steering").get("count").asInt)
        assertTrue(
            awaitingJson.getAsJsonObject("realtime_steering")
                .get("verbatim")
                .asString
                .contains("Second edit"),
        )

        val persisting = folded(snapshotMessage(epoch = 2, closeoutPayload = """{"phase":"response_captured"}"""))
        assertEquals("persisting", AgentDocTurnProjection.fromView(persisting).state)
        assertTrue(AgentDocTurnProjection.fromView(persisting).turnInFlight)

        val idle = folded(snapshotMessage(epoch = 3, closeoutPayload = """{"phase":"committed"}"""))
        assertEquals("idle", AgentDocTurnProjection.fromView(idle).state)
        assertEquals(false, AgentDocTurnProjection.fromView(idle).turnInFlight)
    }

    @Test
    fun `epoch is null before initialization via StateProjectionBridge`() {
        // The bridge-level accessor returns null for a never-subscribed document.
        assertNull(StateProjectionBridge.mirrorEpochForFile("/tmp/agent-doc-lzsync-uninitialized-view.md"))
        assertNull(StateProjectionBridge.mirrorSummaryForFile("/tmp/agent-doc-lzsync-uninitialized-view.md"))
    }

    @Test
    fun `seedMirrorMessageForTest rejects malformed native messages`() {
        val path = "/tmp/agent-doc-lzsync-malformed-${System.nanoTime()}.md"
        try {
            assertFalse(StateProjectionBridge.seedMirrorMessageForTest(path, "{not json"))
            assertFalse(StateProjectionBridge.seedMirrorMessageForTest(path, """{"Bogus":{}}"""))
            assertNull(StateProjectionBridge.mirrorEpochForFile(path))
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    // --- Consumer reactive-read tests (`#lzsync` 3B) ---------------------------
    // The route summary consumer (TerminalUtil finally block) reads the reactive
    // view via reactiveSummaryForFile. With FFI unavailable in unit tests,
    // subscribeMirrorForFile is a no-op, so a view seeded directly stands in for
    // "FFI deltas already applied".

    @Test
    fun `reactiveSummaryForFile derives from the seeded view not the cold pull`() {
        val path = "/tmp/agent-doc-lzsync-reactive-read-${System.nanoTime()}.md"
        try {
            val applied = StateProjectionBridge.seedMirrorMessageForTest(
                path,
                snapshotMessage(
                    epoch = 5,
                    routePayload = """{"readiness":"dispatch_proven","pane_id":"%3"}""",
                    transportPatches = listOf(60L to """{"phase":"applied","actor_generation":2}"""),
                    proofMarkers = listOf(70L),
                ),
            )
            assertTrue(applied)

            val summary = StateProjectionBridge.reactiveSummaryForFile(path)
            assertNotNull(summary)
            assertEquals("dispatch_proven", summary!!.routeReadiness)
            assertEquals("%3", summary.routePaneId)
            assertEquals("applied", summary.latestTransportPhase)
            assertEquals(1, summary.proofMarkers)
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    @Test
    fun `reactiveSummaryForFile reflects a subsequently applied delta`() {
        val path = "/tmp/agent-doc-lzsync-reactive-delta-${System.nanoTime()}.md"
        try {
            StateProjectionBridge.seedMirrorMessageForTest(
                path,
                snapshotMessage(epoch = 1, routePayload = """{"readiness":"idle","pane_id":"%3"}"""),
            )
            assertEquals("idle", StateProjectionBridge.reactiveSummaryForFile(path)?.routeReadiness)

            StateProjectionBridge.seedMirrorMessageForTest(
                path,
                deltaMessage(1, 2, cellSetOp(11, """{"readiness":"dispatch_proven","pane_id":"%3"}""")),
            )
            assertEquals("dispatch_proven", StateProjectionBridge.reactiveSummaryForFile(path)?.routeReadiness)
            assertEquals(2L, StateProjectionBridge.mirrorEpochForFile(path))
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    @Test
    fun `evictForFile clears owner-generation counters and restarts fresh`() {
        // #jbmirrorevict / #nsq2: the mirrors + generations maps must not leak
        // across document close/reopen.
        val path = "/tmp/agent-doc-nsq2-evict-${System.nanoTime()}.md"

        val firstGen = StateProjectionBridge.recordEditorPatchQueued(path, "patch-evict-seed")
        assertNotNull(firstGen)
        assertEquals(1L, firstGen)
        val secondGen = StateProjectionBridge.recordEditorPatchQueued(path, "patch-evict-seed-2")
        assertEquals(2L, secondGen)

        StateProjectionBridge.evictForFile(path)

        val reGen = StateProjectionBridge.recordEditorPatchQueued(path, "patch-evict-reseed")
        assertEquals(1L, reGen)
        assertNull(StateProjectionBridge.mirrorEpochForFile(path))
    }
}
