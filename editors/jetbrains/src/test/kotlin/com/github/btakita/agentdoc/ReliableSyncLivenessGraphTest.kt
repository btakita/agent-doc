package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Test
import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.google.gson.JsonParser
import java.io.File

class ReliableSyncLivenessGraphTest {
    @Test
    fun `startup enumeration and file-open callback report one open fact`() {
        val graph = ReliableSyncLivenessGraph(42)

        val opened = graph.open(
            "doc",
            "/tmp/doc.md",
            "jetbrains-42-test",
            "jetbrains",
            "0.2.275",
            "operator_text_authority_v1,lazily_transport_receipts_v1",
        )
        assertNotNull(opened)
        assertTrue(opened!!.contains("\"Open\""))
        assertTrue(opened.contains("\"Register\""))
        assertTrue(opened.contains("\"editor_id\":\"jetbrains-42-test\""))
        assertEquals(1, "\"Register\"".toRegex().findAll(opened).count())
        assertNull(graph.open("doc", "/tmp/doc.md", "ignored", "jetbrains", "0", ""))
        assertTrue(graph.isOpen("doc"))

        assertNotNull(graph.close("doc"))
        assertFalse(graph.isOpen("doc"))
    }

    @Test
    fun `native reload republishes current endpoint without duplicating presence`() {
        val graph = ReliableSyncLivenessGraph(42)
        graph.open("doc", "/tmp/doc.md", "old-editor", "jetbrains", "0.2.418", "")

        val republished =
            graph.republishOpen(
                "doc",
                "/tmp/doc.md",
                "current-editor",
                "jetbrains",
                "0.2.419",
                "native_hot_reload_generation_v1",
            )

        assertFalse(republished.contains("\"Open\""))
        assertTrue(republished.contains("\"Register\""))
        assertTrue(republished.contains("\"editor_id\":\"current-editor\""))
        assertTrue(graph.isOpen("doc"))
    }

    @Test
    fun `native reload republish restores presence when the local graph was rebuilt`() {
        val graph = ReliableSyncLivenessGraph(42)

        val republished =
            graph.republishOpen(
                "doc",
                "/tmp/doc.md",
                "current-editor",
                "jetbrains",
                "0.2.419",
                "native_hot_reload_generation_v1",
            )

        assertTrue(republished.indexOf("\"Open\"") < republished.indexOf("\"Register\""))
        assertTrue(graph.isOpen("doc"))
    }

    @Test
    fun `path move opens and registers new identity before closing old identity`() {
        val graph = ReliableSyncLivenessGraph(42)
        graph.open("old-hash", "/tmp/old.md", "editor", "jetbrains", "1", "")

        val moved =
            graph.move(
                "old-hash",
                "new-hash",
                "/tmp/new.md",
                "editor",
                "jetbrains",
                "1",
                "",
            )!!

        assertTrue(graph.isOpen("new-hash"))
        assertFalse(graph.isOpen("old-hash"))
        assertTrue(moved.indexOf("\"Open\"") < moved.indexOf("\"Register\""))
        assertTrue(moved.indexOf("\"Register\"") < moved.indexOf("\"Close\""))
        assertTrue(moved.contains("\"path\":\"/tmp/new.md\""))
    }

    @Test
    fun `path transition retries retain the exact original liveness frame`() {
        val frames = PathTransitionFrameLedger()
        var produced = 0

        val first =
            frames.retain("old-hash\u0000new-hash") {
                produced += 1
                """[{"Open":{"tag":"original"}},{"Close":{"observed_tags":["old"]}}]"""
            }
        val retry =
            frames.retain("old-hash\u0000new-hash") {
                produced += 1
                """[{"Open":{"tag":"wrong-replacement"}}]"""
            }

        assertEquals(first, retry)
        assertEquals(1, produced)
        frames.acknowledge("old-hash\u0000new-hash", first)
        val later =
            frames.retain("old-hash\u0000new-hash") {
                produced += 1
                """[{"Open":{"tag":"next-transition"}}]"""
            }
        assertTrue(later.contains("next-transition"))
        assertEquals(2, produced)
    }

    /** The `Register` op of [batch], with its wall-clock timestamp pinned for comparison. */
    private fun registerOp(batch: String): String {
        val op = JsonParser.parseString(batch).asJsonArray
            .map { it.asJsonObject }
            .single { it.has("Register") }
        op.getAsJsonObject("Register").addProperty("timestamp_ms", 1)
        return op.toString()
    }

    @Test
    fun `registration carries the backend activation receipt the controller reads`() {
        // GH #339 R1 controller identity. The same bytes are the Rust fixture
        // (agent-doc-reliable-sync-io/tests/jetbrains_registration_receipt.rs), which decodes them
        // as an EditorRegistration whose receipt parses as role=backend.
        val receipt = ActivationReceipt(
            role = ActivationReceipt.ROLE_BACKEND,
            jvmInstanceId = "11111111-2222-3333-4444-555555555555",
            shellDigest = "abc123",
            spiVersion = 1,
            implVersion = "0.2.851",
            implSha256 = null,
            generation = 1,
            epoch = 0,
        ).encode()
        var current: String? = receipt
        val graph = ReliableSyncLivenessGraph(42) { current }
        val opened = graph.open(
            "docA",
            "/tmp/doc.md",
            "jetbrains-42-test",
            "jetbrains",
            "0.2.851",
            "operator_text_authority_v1,lazily_transport_receipts_v1",
        )!!
        val fixture = File("src/test/resources/reliable-sync/jetbrains-register-with-receipt.json").readText().trim()
        assertEquals(fixture, registerOp(opened))

        // A republish reads the receipt again, so it reports the generation active then.
        current = receipt.replace("generation=1", "generation=2")
        val republished = graph.republishOpen("docA", "/tmp/doc.md", "jetbrains-42-test", "jetbrains", "0.2.851", "")
        assertTrue(registerOp(republished).contains("generation=2"))
    }

    @Test
    fun `registration without a receipt keeps the wire shape an older controller reads`() {
        val graph = ReliableSyncLivenessGraph(42)
        val opened = graph.open("docA", "/tmp/doc.md", "jetbrains-42-test", "jetbrains", "0.2.851", "")!!
        assertFalse(registerOp(opened).contains("activation_receipt"))
    }

    @Test
    fun `the liveness listener registers with this JVM's backend receipt`() {
        val listener = File("src/main/kotlin/com/github/btakita/agentdoc/ReliableSyncLivenessListener.kt").readText()
        assertTrue(listener.contains("ReliableSyncLivenessGraph(pid) {"))
        assertTrue(listener.contains("ShellGenerationHost.getInstanceIfCreated()?.receipt()?.encode()"))
    }
}
