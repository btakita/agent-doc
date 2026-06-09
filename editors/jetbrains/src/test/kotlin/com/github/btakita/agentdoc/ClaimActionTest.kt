package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ClaimActionTest {
    @Test
    fun `parses cross-session reject marker from merged claim output`() {
        // Mirrors `agent-doc claim` stderr (claim.rs cross_session_reject_marker +
        // the human bail) merged via redirectErrorStream.
        val output = """
            [claim] cross-session-reject pane_id=%43 pane_session=5 configured=0
            Error: pane %43 is in tmux session '5' but project session is '0'; switch to the configured session or pass --force
        """.trimIndent()
        val reject = ClaimAction.parseCrossSessionReject(output)
        assertEquals("%43", reject?.paneId)
        assertEquals("5", reject?.paneSession)
        assertEquals("0", reject?.configured)
    }

    @Test
    fun `field order is not assumed`() {
        val output = "[claim] cross-session-reject configured=main pane_session=work pane_id=%7"
        val reject = ClaimAction.parseCrossSessionReject(output)
        assertEquals("%7", reject?.paneId)
        assertEquals("work", reject?.paneSession)
        assertEquals("main", reject?.configured)
    }

    @Test
    fun `returns null when no marker present`() {
        assertNull(ClaimAction.parseCrossSessionReject("Claimed plan.md"))
        assertNull(ClaimAction.parseCrossSessionReject("Error: some other failure"))
    }

    @Test
    fun `returns null when a required field is missing`() {
        // Missing `configured` — must not half-parse into a misleading dialog.
        assertNull(ClaimAction.parseCrossSessionReject("[claim] cross-session-reject pane_id=%1 pane_session=2"))
    }
}
