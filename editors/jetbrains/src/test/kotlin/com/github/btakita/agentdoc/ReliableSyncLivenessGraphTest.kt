package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Test

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
}
