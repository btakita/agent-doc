package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GH #436: the operator saw `agent-doc: input required` and the backend `idea.log` had no line
 * for it that day.
 *
 * The banner was logged only when the cached presentation CHANGED
 * (`[turn-state] projection changed`). Every later render reused the cached label with no log
 * line: a document still waiting for input when it is reopened, shown in a new editor or client,
 * or seen the next day draws `⚠ agent-doc: input required` from a change line that may be in an
 * earlier day's or a rotated log. A banner the operator sees must leave a line when it is drawn,
 * naming where it came from and how old its projection is.
 */
class TurnStateBannerDisplayLogTest {
    private val doc = "/repo/tasks/doc-a.md"
    private val inputRequired = "⚠ agent-doc: input required"

    private fun change(reason: String = "controller-authority", at: Long = 1_000L) =
        TurnStateBannerDisplayLog.Change(reason, at)

    @Test
    fun `first display of a banner logs one line naming source, reason, and age`() {
        val log = TurnStateBannerDisplayLog()

        val line = log.shown(doc, inputRequired, change(reason = "file-opened", at = 1_000L), "editor-notification", 61_000L)

        assertNotNull("a drawn banner must leave an idea.log line", line)
        line!!
        assertTrue("grep `input required` must find the banner: $line", line.contains("input required"))
        assertTrue(line, line.startsWith("[turn-state] banner shown for $doc: $inputRequired"))
        assertTrue(line, line.contains("source=editor-notification"))
        assertTrue(line, line.contains("via=file-opened"))
        assertTrue("the projection age explains a banner from an older frame: $line", line.contains("age_ms=60000"))
    }

    @Test
    fun `redrawing the same banner does not repeat the line`() {
        val log = TurnStateBannerDisplayLog()

        assertNotNull(log.shown(doc, inputRequired, change(), "editor-notification", 2_000L))
        assertNull(log.shown(doc, inputRequired, change(), "editor-notification", 3_000L))
    }

    @Test
    fun `a banner shown again after it was hidden logs again`() {
        val log = TurnStateBannerDisplayLog()

        log.shown(doc, inputRequired, change(), "editor-notification", 2_000L)
        log.hidden(doc)

        assertNotNull(log.shown(doc, inputRequired, change(), "editor-notification", 3_000L))
    }

    @Test
    fun `a label change logs the new label`() {
        val log = TurnStateBannerDisplayLog()

        log.shown(doc, "⟳ agent-doc: awaiting response", change(), "editor-notification", 2_000L)
        val line = log.shown(doc, inputRequired, change(reason = "controller-authority"), "editor-notification", 3_000L)

        assertNotNull(line)
        assertTrue(line!!, line.contains(inputRequired))
    }

    @Test
    fun `a forgotten document logs again when it is reopened`() {
        val log = TurnStateBannerDisplayLog()

        log.shown(doc, inputRequired, change(), "editor-notification", 2_000L)
        log.forget(doc)

        assertNotNull(log.shown(doc, inputRequired, change(), "editor-notification", 3_000L))
    }

    @Test
    fun `documents are independent`() {
        val log = TurnStateBannerDisplayLog()

        log.shown(doc, inputRequired, change(), "editor-notification", 2_000L)

        assertNotNull(log.shown("/repo/tasks/doc-b.md", inputRequired, change(), "editor-notification", 2_000L))
    }

    @Test
    fun `a banner with no recorded change still logs and says so`() {
        val log = TurnStateBannerDisplayLog()

        val line = log.shown(doc, inputRequired, null, "editor-notification", 2_000L)

        assertNotNull(line)
        assertTrue(line!!, line.contains("via=unknown"))
        assertTrue(line, line.contains("age_ms=unknown"))
    }

    @Test
    fun `the input-required toast names the frame that raised it`() {
        assertEquals(
            "TurnStateBannerRefresher.inputRequired[via=editor-notification-replay]",
            TurnStateBannerRefresher.inputRequiredNoticeSource("editor-notification-replay"),
        )
    }
}
