package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GH #323: on a JetBrains 263 Remote Dev backend the turn banner stayed hidden for a document
 * whose controller reported `awaiting_response`. The client's open/selection edges never reach
 * the project `FileEditorManager` topic, so the document was never tracked and every controller
 * frame for it was discarded.
 */
class TurnStateDocumentTrackingTest {
    private class FakeSubscription(val path: String) {
        var closed = false
    }

    private val opened = mutableListOf<FakeSubscription>()

    private fun tracking() =
        TurnStateDocumentTracking<FakeSubscription, String>(
            subscribe = { path, _ -> FakeSubscription(path).also(opened::add) },
            close = { it.closed = true },
        )

    private val doc = "/repo/tasks/doc-a.md"

    @Test
    fun `a document admitted only by the banner collection edge renders its frames`() {
        val tracking = tracking()
        // No project FileEditorManager event: the provider's collection edge is the only admission.
        assertTrue(tracking.admit(doc).newlyAdmitted)
        assertTrue(tracking.ensureSubscription(doc, "editor-notification"))

        assertTrue(
            "a displayed document applies the awaiting_response frame",
            tracking.offer(doc, "awaiting_response"),
        )
    }

    @Test
    fun `a frame that arrived before the document was displayed is replayed on admission`() {
        val tracking = tracking()
        // `requestRefresh` (patch watcher, Run Agent Doc) subscribed before any editor showed it.
        assertTrue(tracking.ensureSubscription(doc, "socket-crdt-remote"))
        assertFalse(tracking.offer(doc, "idle"))
        assertFalse(tracking.offer(doc, "awaiting_response"))

        val admission = tracking.admit(doc)
        assertTrue(admission.newlyAdmitted)
        assertEquals("only the latest held frame replays", "awaiting_response", admission.replay)
        assertNull("a repeated admission replays nothing", tracking.admit(doc).replay)
        assertFalse(tracking.admit(doc).newlyAdmitted)
    }

    @Test
    fun `repeated collection edges keep one subscription`() {
        val tracking = tracking()
        tracking.admit(doc)
        assertTrue(tracking.ensureSubscription(doc, "editor-notification"))
        assertFalse(tracking.ensureSubscription(doc, "editor-notification"))
        assertFalse(tracking.ensureSubscription(doc, "selection"))
        assertEquals(1, opened.size)
    }

    @Test
    fun `release closes the subscription and a late frame cannot resurrect the phase`() {
        val tracking = tracking()
        tracking.admit(doc)
        tracking.ensureSubscription(doc, "file-opened")

        assertTrue(tracking.release(doc))
        assertTrue(opened.single().closed)
        assertFalse(tracking.isDisplayed(doc))
        assertFalse("a late frame from the closed stream is dropped", tracking.offer(doc, "awaiting_response"))

        val reopened = tracking.admit(doc)
        assertTrue(reopened.newlyAdmitted)
        assertNull("the dropped late frame is never replayed", reopened.replay)
        assertTrue(tracking.ensureSubscription(doc, "editor-notification"))
        assertEquals(2, opened.size)
    }

    @Test
    fun `clear closes every subscription`() {
        val tracking = tracking()
        tracking.admit(doc)
        tracking.ensureSubscription(doc, "file-opened")
        tracking.ensureSubscription("/repo/tasks/other.md", "run-agent-doc")

        tracking.clear()

        assertTrue(opened.all { it.closed })
        assertFalse(tracking.isDisplayed(doc))
        assertFalse(tracking.hasSubscription(doc))
    }
}
