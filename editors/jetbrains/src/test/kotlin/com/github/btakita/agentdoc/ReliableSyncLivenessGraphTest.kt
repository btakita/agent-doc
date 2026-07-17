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
            "0.2.273",
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
}
