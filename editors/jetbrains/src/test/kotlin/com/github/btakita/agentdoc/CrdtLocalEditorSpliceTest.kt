package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
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

    // ---- `#splicebaselength` / `#agentpatchlineage` (live 2026-10-09, contracts.md) ----

    private val lineageShadow =
        "<!-- agent:exchange -->\n> cancel the rebase\n<!-- /agent:exchange -->\n" +
            "<!-- agent:queue -->\n- #a\n<!-- /agent:queue -->\n" +
            "<!-- agent:backlog -->\n- [/] [#open] #129 at 3800641 then stacked\n<!-- /agent:backlog -->\n"
    private val response = "### Re: cancel\n\nCancelled. Nothing was pushed.\n"

    private fun agentPatched(): String {
        val at = lineageShadow.indexOf("<!-- /agent:exchange -->")
        return lineageShadow.substring(0, at) + response + lineageShadow.substring(at)
    }

    private fun lineageOf(text: String, patchId: String = "65bde203") =
        AgentMutationLineage(patchId, agentLineageTextHashUtil(text), text.length)

    private fun queueInsert(base: String, patchId: String? = "65bde203"): CapturedLocalEditorEdit {
        val at = base.indexOf("<!-- /agent:queue -->")
        return CapturedLocalEditorEdit(
            offsetUtf16 = at,
            oldFragment = "",
            newFragment = "- #rebase-conflicts pull/123\n",
            projectionEpoch = 9,
            beforeLengthUtf16 = base.length,
            agentLineage = patchId,
        )
    }

    @Test
    fun `pure insert typed into a longer document never replays at shifted offsets`() {
        val visibleBase = agentPatched()
        val edit = queueInsert(visibleBase)
        // Without the length the replay passes and lands inside the backlog:
        val legacy = edit.copy(beforeLengthUtf16 = -1)
        val shifted = prepareLocalEditorEditsUtil(lineageShadow, listOf(legacy))
        assertNotNull("legacy splices without a base length still replay by offset", shifted)
        assertFalse(
            "the live corruption: the queue line lands outside agent:queue",
            shifted!!.resultingText.contains("- #a\n- #rebase-conflicts"),
        )
        // With the captured pre-edit length the replay is refused.
        assertTrue(localEditorSpliceBaseLengthMismatchUtil(lineageShadow, listOf(edit)))
        assertNull(prepareLocalEditorEditsUtil(lineageShadow, listOf(edit)))
    }

    @Test
    fun `operator splice is rebased past the unreplicated agent patch it was typed after`() {
        val patched = agentPatched()
        val edit = queueInsert(patched)
        val visible = prepareLocalEditorEditsUtil(patched, listOf(edit))!!.resultingText

        val batch =
            rebaseOperatorEditsPastAgentPatchUtil(lineageShadow, visible, listOf(edit), lineageOf(patched))

        assertNotNull(batch)
        // Only the operator's splice reaches the replica, in the queue, and the
        // agent response is NOT inserted (the controller folds it), so nothing
        // can be duplicated.
        assertEquals(
            lineageShadow.replace("- #a\n", "- #a\n- #rebase-conflicts pull/123\n"),
            batch!!.resultingText,
        )
        assertFalse(batch.resultingText.contains("Cancelled."))
        assertEquals(1, batch.edits.size)
    }

    @Test
    fun `a second burst rebases against the advanced lineage cut`() {
        val patched = agentPatched()
        val first = queueInsert(patched)
        val visible1 = prepareLocalEditorEditsUtil(patched, listOf(first))!!.resultingText
        val shadow1 =
            rebaseOperatorEditsPastAgentPatchUtil(lineageShadow, visible1, listOf(first), lineageOf(patched))!!
                .resultingText
        val colonAt = visible1.indexOf("- #rebase-conflicts") + "- #rebase-conflicts".length
        val second = CapturedLocalEditorEdit(colonAt, "", ":", 9, visible1.length, "65bde203")
        val visible2 = prepareLocalEditorEditsUtil(visible1, listOf(second))!!.resultingText

        val batch =
            rebaseOperatorEditsPastAgentPatchUtil(shadow1, visible2, listOf(second), lineageOf(visible1))

        assertEquals(
            lineageShadow.replace("- #a\n", "- #a\n- #rebase-conflicts: pull/123\n"),
            batch!!.resultingText,
        )
    }

    @Test
    fun `splices without a proven agent lineage are held, not rebased`() {
        val patched = agentPatched()
        val edit = queueInsert(patched)
        val visible = prepareLocalEditorEditsUtil(patched, listOf(edit))!!.resultingText
        // No lineage recorded (an unidentified agent mutation cleared it).
        assertNull(rebaseOperatorEditsPastAgentPatchUtil(lineageShadow, visible, listOf(edit), null))
        // A splice typed under a different patch lineage.
        assertNull(
            rebaseOperatorEditsPastAgentPatchUtil(
                lineageShadow,
                visible,
                listOf(edit.copy(agentLineage = "other-patch")),
                lineageOf(patched),
            ),
        )
        // The recorded post-patch text does not match the reconstructed base.
        assertNull(
            rebaseOperatorEditsPastAgentPatchUtil(lineageShadow, visible, listOf(edit), lineageOf(patched + "x")),
        )
    }

    @Test
    fun `an operator edit inside the agent patch cannot be rebased`() {
        val patched = agentPatched()
        val at = patched.indexOf("Nothing")
        val edit = CapturedLocalEditorEdit(at, "Nothing", "Something", 9, patched.length, "65bde203")
        val visible = prepareLocalEditorEditsUtil(patched, listOf(edit))!!.resultingText
        assertNull(rebaseOperatorEditsPastAgentPatchUtil(lineageShadow, visible, listOf(edit), lineageOf(patched)))
    }

    @Test
    fun `an edit before the agent patch keeps its offset`() {
        val patched = agentPatched()
        val at = patched.indexOf("cancel the rebase")
        val edit = CapturedLocalEditorEdit(at, "", "please ", 9, patched.length, "65bde203")
        val visible = prepareLocalEditorEditsUtil(patched, listOf(edit))!!.resultingText
        val batch = rebaseOperatorEditsPastAgentPatchUtil(lineageShadow, visible, listOf(edit), lineageOf(patched))
        assertEquals(lineageShadow.replace("> cancel", "> please cancel"), batch!!.resultingText)
    }
}
