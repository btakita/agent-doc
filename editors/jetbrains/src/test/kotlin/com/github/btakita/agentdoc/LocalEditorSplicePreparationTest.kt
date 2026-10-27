package com.github.btakita.agentdoc

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.random.Random

/**
 * `#kotlinflushprep`: [prepareLocalEditorEditsUtil] applies a burst to an edit window over the
 * shadow instead of a whole-document `StringBuilder`, and converts offsets with a cached
 * surrogate-pair index instead of `Character.codePointCount` over the document. Every case here
 * runs the shipped function beside [legacyPrepareLocalEditorEditsUtil], the pre-window
 * implementation kept verbatim, and requires the same answer: the same native edits (offsets,
 * delete counts, inserts), the same resulting text, and the same refusals.
 */
class LocalEditorSplicePreparationTest {
    @Test
    fun `randomized bursts prepare exactly what the whole-document walk prepared`() {
        val random = Random(0x6b6f746c)
        var compared = 0
        var refused = 0
        repeat(4_000) { round ->
            var shadow = randomText(random, random.nextInt(0, 160))
            // Chain several flushes the way the replica worker does: each one types into the
            // text the previous one produced, so the pair index cached for that text is reused.
            repeat(random.nextInt(1, 6)) {
                val burst = randomBurst(random, shadow, clustered = round % 3 != 0)
                val expected = legacyPrepareLocalEditorEditsUtil(shadow, burst)
                val actual = prepareLocalEditorEditsUtil(shadow, burst)
                assertEquals("round $round shadow=${shadow.escaped()} burst=$burst", expected, actual)
                compared++
                if (expected == null) {
                    refused++
                } else {
                    assertArrayEquals(
                        "pair index carried to the next shadow",
                        scanSurrogatePairStartsUtil(expected.resultingText),
                        surrogatePairStartsUtil(actual!!.resultingText),
                    )
                    shadow =
                        if (random.nextInt(4) == 0) {
                            // An equal text that is not the instance the flush produced: a
                            // cache miss, so the index is rebuilt from the text.
                            String(actual.resultingText.toCharArray())
                        } else {
                            actual.resultingText
                        }
                }
            }
        }
        assertTrue("refused=$refused of $compared: both outcomes must be exercised", refused in 1 until compared)
    }

    @Test
    fun `the edit window counts code points like Character codePointCount over any range`() {
        val random = Random(0x77696e64)
        repeat(2_000) { round ->
            val base = randomText(random, random.nextInt(0, 80))
            val window = EditWindowText(base, scanSurrogatePairStartsUtil(base))
            val text = StringBuilder(base)
            repeat(random.nextInt(1, 8)) {
                val start = random.nextInt(text.length + 1)
                val end = start + random.nextInt(0, minOf(4, text.length - start) + 1)
                val insert = randomText(random, random.nextInt(0, 3))
                window.cover(start, end)
                window.replaceCovered(start, end, insert)
                text.replace(start, end, insert)
                assertEquals(text.length, window.length)
                repeat(12) {
                    val from = random.nextInt(text.length + 1)
                    val to = random.nextInt(from, text.length + 1)
                    assertEquals(
                        "round $round [$from, $to) of ${text.toString().escaped()}",
                        Character.codePointCount(text, from, to),
                        window.codePointCount(from, to),
                    )
                }
            }
            val materialized = window.materialize()
            assertEquals(text.toString(), materialized)
            assertArrayEquals(scanSurrogatePairStartsUtil(materialized), window.resultingPairStarts(materialized))
        }
    }

    @Test
    fun `typing into its own result reuses the cached pair index`() {
        var shadow = "a😀b".repeat(2_000) + "\n❯ prompt\n"
        surrogatePairStartsUtil(shadow)
        val scansBefore = surrogatePairFullScans.get()
        var anchor = shadow.length - 1
        repeat(64) { typed ->
            val edit = CapturedLocalEditorEdit(anchor, "", "😁", 0L, shadow.length)
            val batch = prepareLocalEditorEditsUtil(shadow, listOf(edit))
            assertEquals(legacyPrepareLocalEditorEditsUtil(shadow, listOf(edit)), batch)
            shadow = batch!!.resultingText
            anchor += 2
            assertEquals(2_000 + typed + 1, shadow.length - shadow.codePointCount(0, shadow.length))
        }
        assertEquals("no whole-text scan while typing into produced shadows", scansBefore, surrogatePairFullScans.get())
    }

    @Test
    fun `an empty burst answers the shadow itself`() {
        val shadow = "Queue: 😀\n"
        val batch = prepareLocalEditorEditsUtil(shadow, emptyList())
        assertNotNull(batch)
        assertSame(shadow, batch!!.resultingText)
        assertEquals(emptyList<PreparedLocalEditorEdit>(), batch.edits)
    }

    @Test
    fun `splices that split or join surrogate pairs keep the walk's anchor arithmetic`() {
        val high = "\uD83D"
        val low = "\uDE00"
        val cases =
            listOf(
                // Insert a low half after a lone high half, joining a pair, then type after it.
                "x${high}y" to listOf(edit(2, "", low), edit(3, "", "z"), edit(0, "", "q")),
                // Delete the high half of a pair, then the orphan, then type before both.
                "a😀b😀c" to listOf(edit(1, high, ""), edit(1, low, ""), edit(0, "", "😃")),
                // Replace across a pair boundary from the far end back to the start.
                "😀😀😀" to listOf(edit(5, low, "$low$high"), edit(3, "$low$high", ""), edit(0, high, "é")),
                // A pair whose halves arrive in two splices, typed out of order.
                "ab" to listOf(edit(1, "", low), edit(1, "", high), edit(0, "", "—")),
            )
        for ((shadow, burst) in cases) {
            val expected = legacyPrepareLocalEditorEditsUtil(shadow, burst)
            assertNotNull("case ${shadow.escaped()} must prepare", expected)
            assertEquals("case ${shadow.escaped()}", expected, prepareLocalEditorEditsUtil(shadow, burst))
        }
    }

    private fun edit(offset: Int, old: String, new: String) = CapturedLocalEditorEdit(offset, old, new, 0L)

    private fun String.escaped(): String =
        buildString {
            for (c in this@escaped) if (c.code in 0x20..0x7e) append(c) else append("\\u%04x".format(c.code))
        }

    companion object {
        private val UNITS =
            listOf("a", "b", " ", "\n", "#", "é", "—", "❯", "✓", "😀", "😁", "𝄞", "\uD83D", "\uDE00")

        internal fun randomText(random: Random, units: Int): String =
            buildString { repeat(units) { append(UNITS[random.nextInt(UNITS.size)]) } }

        /**
         * A burst against [shadow]: valid splices against the evolving text (clustered around one
         * caret like typing, or scattered), with an occasional stale range, wrong pre-edit length
         * or out-of-range offset so refusals are compared too.
         */
        internal fun randomBurst(random: Random, shadow: String, clustered: Boolean): List<CapturedLocalEditorEdit> {
            val text = StringBuilder(shadow)
            var caret = if (text.isEmpty()) 0 else random.nextInt(text.length + 1)
            return List(random.nextInt(1, 20)) {
                val start =
                    if (clustered) {
                        (caret + random.nextInt(-3, 4)).coerceIn(0, text.length)
                    } else {
                        random.nextInt(text.length + 1)
                    }
                val deleteLength = if (random.nextInt(3) == 0) random.nextInt(0, minOf(6, text.length - start) + 1) else 0
                val old = text.substring(start, start + deleteLength)
                val new = if (random.nextInt(4) == 0) "" else randomText(random, random.nextInt(1, 4))
                val beforeLength = if (random.nextBoolean()) text.length else -1
                when (random.nextInt(60)) {
                    0 -> CapturedLocalEditorEdit(start, old + "x", new, 0L)
                    1 -> CapturedLocalEditorEdit(start, old, new, 0L, text.length + 1)
                    2 -> CapturedLocalEditorEdit(text.length + 1, "", new, 0L)
                    3 -> CapturedLocalEditorEdit(-1, "", new, 0L)
                    else -> {
                        text.replace(start, start + deleteLength, new)
                        caret = start + new.length
                        CapturedLocalEditorEdit(start, old, new, 0L, beforeLength)
                    }
                }
            }
        }
    }
}

/**
 * The implementation [prepareLocalEditorEditsUtil] replaced, verbatim: a whole-document
 * `StringBuilder` and `Character.codePointCount` walks from the end-of-document anchor.
 */
internal fun legacyPrepareLocalEditorEditsUtil(
    before: String,
    edits: List<CapturedLocalEditorEdit>,
): PreparedLocalEditorBatch? {
    val current = StringBuilder(before)
    var anchorUtf16 = before.length
    var anchorCodePoints = before.codePointCount(0, before.length)
    val prepared = ArrayList<PreparedLocalEditorEdit>(edits.size)
    for (edit in edits) {
        val start = edit.offsetUtf16
        val end = start + edit.oldFragment.length
        if (edit.beforeLengthUtf16 >= 0 && edit.beforeLengthUtf16 != current.length) return null
        if (start < 0 || start > current.length || end > current.length) return null
        if (current.substring(start, end) != edit.oldFragment) return null
        val deleteCodePoints = edit.oldFragment.codePointCount(0, edit.oldFragment.length)
        val insertCodePoints = edit.newFragment.codePointCount(0, edit.newFragment.length)
        val offsetCodePoints =
            if (start >= anchorUtf16) {
                anchorCodePoints + Character.codePointCount(current, anchorUtf16, start)
            } else {
                anchorCodePoints - Character.codePointCount(current, start, anchorUtf16)
            }
        prepared.add(
            PreparedLocalEditorEdit(
                offsetCodePoints = offsetCodePoints,
                deleteCodePoints = deleteCodePoints,
                insert = edit.newFragment,
            ),
        )
        current.replace(start, end, edit.newFragment)
        anchorUtf16 = start + edit.newFragment.length
        anchorCodePoints = offsetCodePoints + insertCodePoints
    }
    return PreparedLocalEditorBatch(prepared, current.toString())
}
