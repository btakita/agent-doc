package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test

/**
 * Coverage for the queue-convergence field of the IPC patch protocol
 * (#adoc-queue-ipc-buffer-divergence). A queue convergence message carries
 * `queue_auto` (desired opening-tag `auto` state), canonical `queue:`
 * frontmatter, and the corrected queue component body.
 */
class ParsePatchJsonQueueAutoTest {

    @Test
    fun `parses queue_auto false convergence message`() {
        val json = """
            {"type":"patch","file":"/tmp/plan.md",
             "patches":[{"component":"queue","content":"- next\n"}],"unmatched":"",
             "frontmatter":"queue: stop","queue_auto":false}
        """.trimIndent()
        val patch = parsePatchJson(json)
        assertNotNull(patch)
        assertEquals(false, patch!!.queueAuto)
        assertEquals("queue: stop", patch.frontmatter)
        assertEquals(1, patch.patches.size)
        assertEquals("queue", patch.patches.single().component)
        assertEquals("- next\n", patch.patches.single().content)
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
