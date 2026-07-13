package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReliableSyncLivenessGraphTest {
    @Test
    fun `startup enumeration and file-open callback report one open fact`() {
        val graph = ReliableSyncLivenessGraph(42)

        assertNotNull(graph.open("doc"))
        assertNull(graph.open("doc"))
        assertTrue(graph.isOpen("doc"))

        assertNotNull(graph.close("doc"))
        assertFalse(graph.isOpen("doc"))
    }
}
