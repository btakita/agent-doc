package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#clearsupersedenotice`: a queued Clear Session Context dropped by the supervisor (a newer prompt
 * started before the turn ended) reaches the editor over the document-turn authority stream and is
 * announced once per dropped `clear_epoch`.
 */
class DeferredClearSupersededNoticesTest {
    private val doc = "/repo/tasks/root.md"

    private fun dropped(epoch: Long) = DeferredClearSuperseded(epoch, "%149", "newer_prompt")

    private fun readyFrame(turnExtra: String = ""): String =
        """{"document":"$doc","readiness":"ready","turn":{"state":"awaiting_response",""" +
            """"turn_in_flight":true,"transition_authority":"project_controller"$turnExtra}}"""

    @Test
    fun `parses the dropped clear from a ready authority frame`() {
        val observation = TurnStateBridge.deferredClearSupersededObservation(
            readyFrame(
                ""","deferred_clear_superseded":{"clear_epoch":7,"pane":"%149",""" +
                    """"reason":"newer_prompt","superseded_at":1040}""",
            ),
        )
        assertEquals(DeferredClearSupersededObservation(dropped(7)), observation)
        assertEquals(
            DeferredClearSupersededObservation(null),
            TurnStateBridge.deferredClearSupersededObservation(readyFrame()),
        )
    }

    @Test
    fun `frames without a controller projection are not observations`() {
        assertNull(TurnStateBridge.deferredClearSupersededObservation(null))
        assertNull(
            TurnStateBridge.deferredClearSupersededObservation(
                """{"document":"$doc","readiness":"unavailable","error":"socket gone"}""",
            ),
        )
        assertNull(TurnStateBridge.deferredClearSupersededObservation("not json"))
    }

    @Test
    fun `first frame seeds the baseline without announcing an old drop`() {
        val notices = DeferredClearSupersededNotices()
        assertFalse(notices.observe(doc, dropped(3)))
        assertFalse("same drop again (reconnect) is not re-announced", notices.observe(doc, dropped(3)))
        assertFalse(notices.observe(doc, null))
        assertTrue("a new drop is announced", notices.observe(doc, dropped(4)))
        assertFalse(notices.observe(doc, dropped(4)))
    }

    @Test
    fun `a drop after an empty baseline is announced exactly once`() {
        val notices = DeferredClearSupersededNotices()
        assertFalse(notices.observe(doc, null))
        assertTrue(notices.observe(doc, dropped(1)))
        assertFalse(notices.observe(doc, dropped(1)))
        // The next queued clear retires the field; that is not a drop.
        assertFalse(notices.observe(doc, null))
        assertTrue(notices.observe(doc, dropped(2)))
        // Other documents keep their own baseline.
        assertFalse(notices.observe("/repo/tasks/other.md", dropped(2)))
    }

    @Test
    fun `closing the document re-seeds on reopen`() {
        val notices = DeferredClearSupersededNotices()
        assertFalse(notices.observe(doc, null))
        notices.forget(doc)
        assertFalse(notices.observe(doc, dropped(5)))
    }

    @Test
    fun `message names the document and pane and how to clear again`() {
        val message = TerminalUtil.buildDeferredClearSupersededMessage("tasks/root.md", dropped(7))
        assertTrue(message.startsWith("Queued Clear Session Context was not sent for tasks/root.md.\n"))
        assertTrue(message.contains("A newer prompt started in pane %149"))
        assertTrue(message.contains("Press Clear Session Context again"))
        assertTrue(message.contains("Interrupt and clear"))
        val other = TerminalUtil.buildDeferredClearSupersededMessage(
            "tasks/root.md",
            DeferredClearSuperseded(8, "%2", "session_restarted"),
        )
        assertTrue(other.contains("reason=session_restarted"))
    }
}
