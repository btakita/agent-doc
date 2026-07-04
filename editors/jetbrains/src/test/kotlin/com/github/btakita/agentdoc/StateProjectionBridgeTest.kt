package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.security.MessageDigest
import java.util.Base64

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
        assertEquals(
            "route=dispatch_proven pane=%2 transport=patch-2:applied proof_markers=1",
            summary.compact(),
        )
    }

    // --- StateGraphMirror mirror-apply tests (`#lazilystatesync3`) ------------
    // Pin the plugin-local mirror behavior to the lazily-kt conformance tests
    // (`#lzpkgwire`). The mirror must converge identically to a snapshot after
    // applying the corresponding delta stream, and a no-op delta must be safe.

    private fun b64(json: String): String =
        Base64.getEncoder().encodeToString(json.toByteArray())

    private fun snapshotJson(
        epoch: Long,
        routePayload: String? = null,
        transportPatches: List<Pair<Long, String>> = emptyList(),
        proofMarkers: List<Long> = emptyList(),
    ): String {
        val nodes = StringBuilder()
        fun addNode(slot: Long, typeTag: String, payload: String?) {
            if (nodes.isNotEmpty()) nodes.append(',')
            nodes.append("""{"slot_id":$slot,"type_tag":"$typeTag","state":"resolved"""")
            if (payload != null) nodes.append(""","payload":"$payload"}""") else nodes.append("}")
        }
        if (routePayload != null) addNode(11, AgentDocNodeType.ROUTE, routePayload)
        transportPatches.forEach { (slot, payload) -> addNode(slot, AgentDocNodeType.TRANSPORT_PATCH, payload) }
        proofMarkers.forEach { slot -> addNode(slot, AgentDocNodeType.PROOF_MARKER, b64("{}")) }
        return """{"type":"snapshot","epoch":$epoch,"document_hash":"doc-a","nodes":[$nodes],"edges":[],"roots":[]}"""
    }

    private fun deltaJson(
        baseEpoch: Long,
        epoch: Long,
        opsJson: String,
    ): String =
        """{"type":"delta","base_epoch":$baseEpoch,"epoch":$epoch,"document_hash":"doc-a","ops":[$opsJson]}"""

    @Test
    fun `mirror cold snapshot then delta converges identically to direct snapshot`() {
        val routeFinal = b64("""{"readiness":"dispatch_proven","pane_id":"%2"}""")
        val proofPayload = b64("{}")

        // Path A: one cold snapshot of the full state.
        val direct = StateGraphMirror().apply {
            applyMessage(snapshotJson(epoch = 3, routePayload = routeFinal, proofMarkers = listOf(50)))
        }

        // Path B: cold snapshot (partial, epoch 1) then delta to full (epoch 3).
        val incremental = StateGraphMirror().apply {
            applyMessage(snapshotJson(epoch = 1, routePayload = b64("""{"readiness":"idle","pane_id":"%2"}""")))
            applyMessage(
                deltaJson(
                    baseEpoch = 1,
                    epoch = 3,
                    opsJson =
                        """{"op":"cell_set","slot_id":11,"payload":"$routeFinal"},""" +
                            """{"op":"node_add","slot_id":50,"type_tag":"${AgentDocNodeType.PROOF_MARKER}","payload":"$proofPayload"}""",
                )
            )
        }

        assertEquals(direct.epoch, incremental.epoch)
        assertEquals(
            MirrorProjectionSummary.fromMirror(direct).compact(),
            MirrorProjectionSummary.fromMirror(incremental).compact(),
        )
        assertEquals("dispatch_proven", MirrorProjectionSummary.fromMirror(incremental).routeReadiness)
    }

    @Test
    fun `mirror derive summary reads route phase and proof count from tracked cells`() {
        val mirror = StateGraphMirror()
        mirror.applyMessage(
            snapshotJson(
                epoch = 4,
                routePayload = b64("""{"readiness":"dispatch_proven","pane_id":"%2"}"""),
                transportPatches = listOf(
                    40L to b64("""{"phase":"applied","actor_generation":1}"""),
                    41L to b64("""{"phase":"queued","actor_generation":1}"""),
                ),
                proofMarkers = listOf(50L),
            )
        )

        val summary = MirrorProjectionSummary.fromMirror(mirror)
        assertEquals("dispatch_proven", summary.routeReadiness)
        assertEquals("%2", summary.routePaneId)
        // Latest transport patch by slot_id wins; phase readable, patch_id not on wire.
        assertEquals("queued", summary.latestTransportPhase)
        assertEquals(1, summary.proofMarkers)
    }

    @Test
    fun `mirror no-op delta is safe and node_remove is honored`() {
        val mirror = StateGraphMirror()
        mirror.applyMessage(snapshotJson(epoch = 2, routePayload = b64("""{"readiness":"idle","pane_id":"%2"}""")))
        val beforeEpoch = mirror.epoch

        // No-op delta: caller is current.
        mirror.applyMessage(deltaJson(baseEpoch = 2, epoch = 2, opsJson = ""))
        assertEquals(beforeEpoch, mirror.epoch)

        // Node remove.
        mirror.applyMessage(deltaJson(baseEpoch = 2, epoch = 3, opsJson = """{"op":"node_remove","slot_id":11}"""))
        assertEquals(3, mirror.epoch)
        assertNull(mirror.singletonNode(AgentDocNodeType.ROUTE))
    }

    @Test
    fun `mirror empty yields nullish summary`() {
        val mirror = StateGraphMirror()
        val summary = MirrorProjectionSummary.fromMirror(mirror)
        assertNull(summary.routeReadiness)
        assertNull(summary.latestTransportPhase)
        assertEquals(0, summary.proofMarkers)
    }

    @Test
    fun `mirror epoch is null before initialization via StateProjectionBridge`() {
        // The bridge-level accessor returns null for a never-subscribed document.
        // (Using a path unlikely to collide with real FFI state.)
        assertNull(StateProjectionBridge.mirrorEpochForFile("/tmp/agent-doc-n529-uninitialized-mirror.md"))
        assertNull(StateProjectionBridge.mirrorSummaryForFile("/tmp/agent-doc-n529-uninitialized-mirror.md"))
    }

    @Test
    fun `applyMessage rejects unknown type discriminator`() {
        val mirror = StateGraphMirror()
        org.junit.Assert.assertFalse(mirror.applyMessage("""{"type":"not_a_real_kind"}"""))
        org.junit.Assert.assertFalse(mirror.isInitialized)
        assertTrue(mirror.nodeCount == 0)
    }

    // --- Consumer reactive-read tests (`#n529b` / `#lazilystatesync3b`) --------
    // The route summary consumer (TerminalUtil finally block) now reads the
    // reactive mirror via reactiveSummaryForFile instead of the cold pull. With
    // FFI unavailable in unit tests, subscribeMirrorForFile is a no-op, so a
    // mirror seeded directly stands in for "FFI deltas already applied".

    @Test
    fun `reactiveSummaryForFile derives from the seeded mirror not the cold pull`() {
        val path = "/tmp/agent-doc-n529b-reactive-read-${System.nanoTime()}.md"
        try {
            // Seed the per-document mirror as if FFI deltas had advanced it.
            val applied = StateProjectionBridge.seedMirrorMessageForTest(
                path,
                snapshotJson(
                    epoch = 5,
                    routePayload = b64("""{"readiness":"dispatch_proven","pane_id":"%3"}"""),
                    transportPatches = listOf(60L to b64("""{"phase":"applied","actor_generation":2}""")),
                    proofMarkers = listOf(70L),
                ),
            )
            assertTrue(applied)

            // The consumer read derives every field from the reactive mirror.
            val summary = StateProjectionBridge.reactiveSummaryForFile(path)
            assertNotNull(summary)
            assertEquals("dispatch_proven", summary!!.routeReadiness)
            assertEquals("%3", summary.routePaneId)
            assertEquals("applied", summary.latestTransportPhase)
            assertEquals(1, summary.proofMarkers)
            // FFI unavailable in tests → no cold-pull transport-id backfill.
            assertNull(summary.latestTransportPatchId)
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    @Test
    fun `reactiveSummaryForFile reflects a subsequently applied delta`() {
        val path = "/tmp/agent-doc-n529b-reactive-delta-${System.nanoTime()}.md"
        try {
            StateProjectionBridge.seedMirrorMessageForTest(
                path,
                snapshotJson(epoch = 1, routePayload = b64("""{"readiness":"idle","pane_id":"%3"}""")),
            )
            assertEquals("idle", StateProjectionBridge.reactiveSummaryForFile(path)?.routeReadiness)

            // A later FFI delta flips the route readiness; the consumer read must
            // observe the new cell value reactively (no full re-render).
            StateProjectionBridge.seedMirrorMessageForTest(
                path,
                deltaJson(
                    baseEpoch = 1,
                    epoch = 2,
                    opsJson = """{"op":"cell_set","slot_id":11,"payload":"${b64("""{"readiness":"dispatch_proven","pane_id":"%3"}""")}"}""",
                ),
            )
            assertEquals("dispatch_proven", StateProjectionBridge.reactiveSummaryForFile(path)?.routeReadiness)
            assertEquals(2L, StateProjectionBridge.mirrorEpochForFile(path))
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    @Test
    fun `reactiveSummaryForFile prefers the reactive mirror over the cold pull`() {
        // Seed the mirror with a route readiness that no cold projection would
        // invent for a throwaway path. The consumer read must surface the mirror's
        // tracked-cell value, proving it derives from the reactive mirror (not the
        // cold projectionSummaryForFile pull) on the happy path. This holds whether
        // or not the FFI library happens to be loadable in the test JVM.
        val path = "/tmp/agent-doc-n529b-reactive-prefer-${System.nanoTime()}.md"
        try {
            StateProjectionBridge.seedMirrorMessageForTest(
                path,
                snapshotJson(epoch = 9, routePayload = b64("""{"readiness":"dispatch_proven","pane_id":"%9"}""")),
            )
            val summary = StateProjectionBridge.reactiveSummaryForFile(path)
            assertNotNull(summary)
            assertEquals("dispatch_proven", summary!!.routeReadiness)
            assertEquals("%9", summary.routePaneId)
        } finally {
            StateProjectionBridge.evictForFile(path)
        }
    }

    @Test
    fun `evictForFile clears owner-generation counters and restarts fresh`() {
        // #jbmirrorevict / #nsq2: the mirrors + generations maps must not leak
        // across document close/reopen. Use a unique path so the singleton bridge
        // state never collides with other tests.
        val path = "/tmp/agent-doc-nsq2-evict-${System.nanoTime()}.md"

        // Seed an owner-generation counter. nextGeneration mutates `generations`
        // unconditionally (FFI only gates recordFact, which is a no-op here).
        val firstGen = StateProjectionBridge.recordEditorPatchQueued(path, "patch-evict-seed")
        assertNotNull(firstGen)
        assertEquals(1L, firstGen)
        val secondGen = StateProjectionBridge.recordEditorPatchQueued(path, "patch-evict-seed-2")
        assertEquals(2L, secondGen)

        // Evict on close.
        StateProjectionBridge.evictForFile(path)

        // After eviction the docHash-scoped generation counter is gone, so a
        // reopened (or reused) path restarts at 1 — no stale actor_generation
        // bleed from the prior document.
        val reGen = StateProjectionBridge.recordEditorPatchQueued(path, "patch-evict-reseed")
        assertEquals(1L, reGen)
        // Mirror was never created (no FFI in unit tests); epoch stays null.
        assertNull(StateProjectionBridge.mirrorEpochForFile(path))
    }
}
