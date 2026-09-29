package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * `#ambiguousholdforever2`: registration's containment proof runs over operator text only.
 * The fpe.md replica was refused for good because a reloaded disk projection moved the
 * boundary and added ` (HEAD)`, which canonical never shared (formal/tla/RetainedProjectionHold.tla;
 * agent-doc-merge `marker_only_difference_blocks_containment_until_markers_are_normalized`).
 */
class BinaryOwnedMarkerNormalizationTest {
    private val diskProjection =
        "<!-- agent:exchange -->\n" +
            "<!-- agent:boundary:60c81193 -->\n" +
            "### Re: FPE capacity recommendation (HEAD)\n\n" +
            "Increase CPU first.\n" +
            "<!-- /agent:exchange -->\n" +
            "- PR #194 is merged. Continue.\n"

    private val canonical =
        "<!-- agent:exchange -->\n" +
            "### Re: FPE capacity recommendation\n\n" +
            "Increase CPU first.\n" +
            "  <!-- agent:boundary:60c81193 -->\n" +
            "<!-- /agent:exchange -->\n" +
            "- PR #194 is merged. Continue.\n"

    @Test
    fun `boundary lines and HEAD suffixes are not operator text`() {
        assertEquals(
            withoutBinaryOwnedMarkersUtil(canonical),
            withoutBinaryOwnedMarkersUtil(diskProjection),
        )
    }

    @Test
    fun `operator text that merely mentions the markers is kept`() {
        val operator =
            "Why does the heading say (HEAD)?\n" +
                "`<!-- agent:boundary:60c81193 -->` appears inline here.\n" +
                "### Re: topic (HEAD) was renamed\n"
        assertEquals(operator, withoutBinaryOwnedMarkersUtil(operator))
    }

    @Test
    fun `a boundary on the last line without a newline is removed`() {
        assertEquals("text\n", withoutBinaryOwnedMarkersUtil("text\n<!-- agent:boundary:ab12 -->"))
    }
}
