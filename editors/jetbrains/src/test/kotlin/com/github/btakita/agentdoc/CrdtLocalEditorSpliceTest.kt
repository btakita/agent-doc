package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class CrdtLocalEditorSpliceTest {
    @Test
    fun `first backlog deletion reconstructs its causal base before attach`() {
        val removed = "- [ ] stale backlog item\n"
        val before = "# contract\n\n<!-- agent:pending -->\n$removed<!-- /agent:pending -->\n"
        val offset = before.indexOf(removed)
        val after = before.removeRange(offset, offset + removed.length)
        val edit = CapturedLocalEditorEdit(offset, removed, "", 1)

        val reconstructed = reconstructLocalEditorBaseTextUtil(after, edit)

        assertEquals(before, reconstructed)
        assertEquals(after, prepareLocalEditorEditsUtil(reconstructed!!, listOf(edit))!!.resultingText)
    }

    @Test
    fun `first edit base reconstruction rejects a mismatched post-edit buffer`() {
        assertNull(
            reconstructLocalEditorBaseTextUtil(
                after = "different",
                edit = CapturedLocalEditorEdit(0, "old", "new", 1),
            ),
        )
    }

    @Test
    fun `partial typing snapshots remain one causal splice stream`() {
        val before = "Queue: "
        val edits =
            listOf(
                CapturedLocalEditorEdit(7, "", "Temp", 4),
                CapturedLocalEditorEdit(11, "", "or", 4),
                CapturedLocalEditorEdit(13, "", "al", 4),
            )

        val prepared = prepareLocalEditorEditsUtil(before, edits)

        assertNotNull(prepared)
        assertEquals(3, prepared!!.edits.size)
        assertEquals("Queue: Temporal", prepared.resultingText)
        assertEquals(listOf("Temp", "or", "al"), prepared.edits.map { it.insert })
    }

    @Test
    fun `edits in separate cells stay separate`() {
        val before = "first\nsecond"
        val edits =
            listOf(
                CapturedLocalEditorEdit(0, "first", "one", 7),
                CapturedLocalEditorEdit(4, "second", "two", 7),
            )

        val prepared = prepareLocalEditorEditsUtil(before, edits)

        assertNotNull(prepared)
        assertEquals(2, prepared!!.edits.size)
        assertEquals("one\ntwo", prepared.resultingText)
    }

    @Test
    fun `stale splice never widens into whole-buffer authority`() {
        assertNull(
            prepareLocalEditorEditsUtil(
                "canonical",
                listOf(CapturedLocalEditorEdit(0, "stale", "typed", 1)),
            ),
        )
    }

    @Test
    fun `splice boundaries convert surrogate pairs to code points`() {
        val prepared =
            prepareLocalEditorEditsUtil(
                "a😀z",
                listOf(CapturedLocalEditorEdit(1, "😀", "😁", 1)),
            )

        assertNotNull(prepared)
        assertEquals(1, prepared!!.edits.single().offsetCodePoints)
        assertEquals(1, prepared.edits.single().deleteCodePoints)
        assertEquals("a😁z", prepared.resultingText)
    }

    @Test
    fun `held key burst retains one final projection`() {
        val before = "x".repeat(84_000)
        val insertionOffset = 81_000
        val edits =
            (0 until 3_000).map { index ->
                CapturedLocalEditorEdit(insertionOffset + index, "", "?", 9)
            }

        val prepared = prepareLocalEditorEditsUtil(before, edits)

        assertNotNull(prepared)
        assertEquals(3_000, prepared!!.edits.size)
        assertEquals(before.substring(0, insertionOffset) + "?".repeat(3_000) + before.substring(insertionOffset), prepared.resultingText)
        assertEquals(insertionOffset + 2_999, prepared.edits.last().offsetCodePoints)
    }

    @Test
    fun `splices subsumed by a published buffer are not replayed`() {
        // #subsumedsplicereplay live shape (infra.md): the 18 typed characters were
        // published inside the whole buffer, and their queued splices replayed.
        val shadow = "- Create a PR to fix the SBX + STG drift.\n"
        val typed = " if there is drift"
        val offset = shadow.indexOf(".\n")
        val edits = typed.mapIndexed { index, ch -> CapturedLocalEditorEdit(offset + index, "", ch.toString(), 4) }
        val published = prepareLocalEditorEditsUtil(shadow, edits)!!.resultingText

        // The hazard: pure inserts validate against the published buffer too.
        val replayed = prepareLocalEditorEditsUtil(published, edits)
        assertEquals(2, replayed!!.resultingText.split(typed).size - 1)

        // The fence: the publication advanced the epoch, so nothing is owed.
        assertEquals(emptyList<CapturedLocalEditorEdit>(), currentEpochCapturedEditsUtil(edits, 5))
        // Splices typed after the publication still forward.
        val later = CapturedLocalEditorEdit(0, "", "x", 5)
        assertEquals(listOf(later), currentEpochCapturedEditsUtil(edits + later, 5))
    }

    // `retainedtargetdropsedit`: the live shape. A re-register published the buffer
    // (shadow + one typed newline) while the operator pasted a queue line; the
    // paste's trailing newline was then deleted. The paste was retired by the
    // publication fence, and the delete validated against a blank line further
    // down the shadow, so the replica lost the paste AND a blank line.
    private val queueShadow =
        "# Queue\n\n<!-- agent:queue priority -->\n<!-- /agent:queue -->\n\n# Backlog\n\n" +
            "<!-- agent:backlog priority queue -->\n<!-- /agent:backlog -->\n\n## Review\n"
    // Sized so the paste's trailing newline sits, in the published buffer, on the
    // blank line before `## Review` — the coincidence that let the stale delete
    // validate live.
    private val pastedLine =
        "- \uD83D\uDEA7 Make the rest of the sample-app/pull/22 stack ready to review and rebase it onto main first."

    private data class LiveRace(
        val published: String,
        val visibleAtFence: String,
        val finalVisible: String,
        val capturedAtFence: List<CapturedLocalEditorEdit>,
        val deleteAfterFence: CapturedLocalEditorEdit,
    )

    private fun liveRace(): LiveRace {
        val markerEnd = queueShadow.indexOf("<!-- agent:queue priority -->") + "<!-- agent:queue priority -->".length
        val typedNewline = CapturedLocalEditorEdit(markerEnd, "", "\n", 0)
        val published = prepareLocalEditorEditsUtil(queueShadow, listOf(typedNewline))!!.resultingText
        val paste = CapturedLocalEditorEdit(markerEnd + 1, "", "$pastedLine\n", 0)
        val visibleAtFence = prepareLocalEditorEditsUtil(published, listOf(paste))!!.resultingText
        val trailingNewline = markerEnd + 1 + pastedLine.length
        val deleteTrailingNewline = CapturedLocalEditorEdit(trailingNewline, "\n", "", 0)
        val finalVisible = prepareLocalEditorEditsUtil(visibleAtFence, listOf(deleteTrailingNewline))!!.resultingText
        return LiveRace(published, visibleAtFence, finalVisible, listOf(typedNewline, paste), deleteTrailingNewline)
    }

    @Test
    fun `publication fence keeps the splice typed while the buffer was in flight`() {
        val race = liveRace()
        assertEquals(
            queueShadow.replace("priority -->\n", "priority -->\n$pastedLine\n"),
            race.finalVisible,
        )

        val owed =
            capturedEditsOwedAfterPublishedCutUtil(
                published = race.published,
                visible = race.visibleAtFence,
                captured = race.capturedAtFence,
                epoch = 1,
            )

        // Only the paste is owed; the typed newline is inside the published buffer.
        assertEquals(listOf(race.capturedAtFence[1].copy(projectionEpoch = 1)), owed)
        // The delete typed after the fence joins the same epoch and the batch,
        // forwarded from the published buffer, lands exactly on the editor text.
        val forwarded =
            prepareLocalEditorEditsUtil(
                race.published,
                currentEpochCapturedEditsUtil(owed + race.deleteAfterFence.copy(projectionEpoch = 1), 1),
            )
        assertEquals(race.finalVisible, forwarded!!.resultingText)
    }

    @Test
    fun `retiring every captured splice reproduces the live replica loss`() {
        // The pre-fix fence advanced the epoch, retiring the paste with the
        // subsumed newline. This pins WHY the owed suffix matters: the lone
        // delete still validates (the shadow has a blank line at that offset)
        // and deletes the wrong line.
        val race = liveRace()
        val survivors =
            currentEpochCapturedEditsUtil(
                race.capturedAtFence + race.deleteAfterFence.copy(projectionEpoch = 1),
                1,
            )
        val replica = prepareLocalEditorEditsUtil(race.published, survivors)!!.resultingText

        assertEquals(race.published.replace("-->\n\n## Review", "-->\n## Review"), replica)
        assertFalse(replica.contains(pastedLine))
    }

    @Test
    fun `publication fence owes nothing when the buffer did not move`() {
        // `#subsumedsplicereplay` stays fixed: every captured splice is subsumed.
        val race = liveRace()
        assertEquals(
            emptyList<CapturedLocalEditorEdit>(),
            capturedEditsOwedAfterPublishedCutUtil(
                race.published,
                race.published,
                race.capturedAtFence.take(1),
                3,
            ),
        )
    }

    @Test
    fun `publication fence falls back to one exact splice when the cut is unprovable`() {
        val race = liveRace()
        // No captured history explains the visible text (e.g. the list was lost).
        val owed = capturedEditsOwedAfterPublishedCutUtil(race.published, race.finalVisible, emptyList(), 2)

        assertEquals(1, owed.size)
        assertEquals(2L, owed.single().projectionEpoch)
        assertEquals(race.finalVisible, prepareLocalEditorEditsUtil(race.published, owed)!!.resultingText)
    }

    @Test
    fun `stuck replica rolls the visible operator text forward`() {
        // The wedged state: shadow == replica == the lossy text, nothing captured,
        // the editor shows the operator's paste. Recovery owes exactly the editor text.
        val race = liveRace()
        val lossy = race.published.replace("-->\n\n## Review", "-->\n## Review")

        val splice = unforwardedOperatorTextSpliceUtil(lossy, lossy, race.finalVisible, emptyList(), 4)

        assertNotNull(splice)
        assertEquals(race.finalVisible, prepareLocalEditorEditsUtil(lossy, listOf(splice!!))!!.resultingText)
    }

    @Test
    fun `stuck replica recovery waits for captured splices and remote projections`() {
        val race = liveRace()
        val lossy = race.published
        // Typing in flight: the ordinary splice flush owns it.
        assertNull(
            unforwardedOperatorTextSpliceUtil(lossy, lossy, race.finalVisible, race.capturedAtFence, 0),
        )
        // Replica ahead of the shadow: a remote delivery is still projecting.
        assertNull(unforwardedOperatorTextSpliceUtil(lossy, race.finalVisible, lossy, emptyList(), 0))
        // Converged.
        assertNull(unforwardedOperatorTextSpliceUtil(lossy, lossy, lossy, emptyList(), 0))
    }

    @Test
    fun `single splice never splits a surrogate pair`() {
        val splice = singleSpliceCapturedEditUtil("a\uD83D\uDE00z", "a\uD83D\uDE01z", 0)!!

        assertEquals(1, splice.offsetUtf16)
        assertEquals("\uD83D\uDE00", splice.oldFragment)
        assertEquals("\uD83D\uDE01", splice.newFragment)
        assertNull(singleSpliceCapturedEditUtil("same", "same", 0))
    }
}
