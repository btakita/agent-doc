package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test

/**
 * Coverage for the queue-convergence field of the IPC patch protocol
 * (#adoc-queue-ipc-buffer-divergence). A queue convergence message carries
 * `queue_auto` (desired opening-tag `auto` state) plus a `queue_active`
 * frontmatter merge and an empty `patches` array.
 */
class ParsePatchJsonQueueAutoTest {

    @Test
    fun `parses queue_auto false convergence message`() {
        val json = """
            {"type":"patch","file":"/tmp/plan.md","patches":[],"unmatched":"",
             "frontmatter":"queue_active: false","queue_auto":false}
        """.trimIndent()
        val patch = parsePatchJson(json)
        assertNotNull(patch)
        assertEquals(false, patch!!.queueAuto)
        assertEquals("queue_active: false", patch.frontmatter)
        assertTrue(patch.patches.isEmpty())
    }

    @Test
    fun `parses queue_auto true convergence message`() {
        val json =
            """{"type":"patch","file":"/tmp/plan.md","patches":[],"queue_auto":true}"""
        val patch = parsePatchJson(json)
        assertNotNull(patch)
        assertEquals(true, patch!!.queueAuto)
    }

    @Test
    fun `queueAuto is null when field absent`() {
        val json =
            """{"type":"patch","file":"/tmp/plan.md","patches":[],"frontmatter":"model: opus"}"""
        val patch = parsePatchJson(json)
        assertNotNull(patch)
        assertNull(patch!!.queueAuto)
    }

    @Test
    fun `queueAuto is null when field is json null`() {
        val json =
            """{"type":"patch","file":"/tmp/plan.md","patches":[],"queue_auto":null}"""
        val patch = parsePatchJson(json)
        assertNotNull(patch)
        assertNull(patch!!.queueAuto)
    }
}
