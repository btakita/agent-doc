package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.random.Random

/** Compare the persistent shadow to the original whole-document walk. */
class LocalEditorSplicePreparationTest {
    @Test
    fun `randomized bursts prepare exactly what the whole-document walk prepared`() {
        val random = Random(0x6b6f746c)
        var compared = 0
        var refused = 0
        repeat(4_000) { round ->
            var text = randomText(random, random.nextInt(0, if (round % 5 == 0) 420 else 160))
            var shadow = ReplicaShadow(text)
            repeat(random.nextInt(1, 6)) {
                val burst = randomBurst(random, text, clustered = round % 3 != 0)
                val expected = legacyPrepareLocalEditorEditsUtil(text, burst)
                val actual = prepareLocalEditorEditsUtil(shadow, burst)
                assertEquals("round $round shadow=${text.escaped()} burst=$burst", expected, actual?.toTextBatch())
                compared++
                if (expected == null) {
                    refused++
                } else {
                    text = expected.resultingText
                    shadow = actual!!.resultingShadow
                }
            }
        }
        assertTrue("refused=$refused of $compared: both outcomes must be exercised", refused in 1 until compared)
    }

    @Test
    fun `the rope counts code points like Character codePointCount over any range`() {
        val random = Random(0x77696e64)
        repeat(2_000) { round ->
            val base = randomText(random, random.nextInt(0, 350))
            var shadow = ReplicaShadow(base)
            val text = StringBuilder(base)
            repeat(random.nextInt(1, 8)) {
                val start = random.nextInt(text.length + 1)
                val end = start + random.nextInt(0, minOf(4, text.length - start) + 1)
                val insert = randomText(random, random.nextInt(0, 3))
                shadow = shadow.replace(start, end, insert)
                text.replace(start, end, insert)
                assertEquals(text.length, shadow.length)
                repeat(12) {
                    val from = random.nextInt(text.length + 1)
                    val to = random.nextInt(from, text.length + 1)
                    assertEquals(
                        "round $round [$from, $to) of ${text.toString().escaped()}",
                        Character.codePointCount(text, from, to),
                        shadow.codePointCount(from, to),
                    )
                }
            }
            assertEquals(text.toString(), shadow.toString())
        }
    }

    @Test
    fun `typing into its own result does not rescan the unchanged document`() {
        val before = "a😀b".repeat(20_000) + "\n❯ prompt\n"
        var shadow = ReplicaShadow(before)
        assertEquals(20_000, shadow.length - shadow.codePointCount(0, shadow.length))
        val scanned = surrogatePairScannedUtf16.get()
        var anchor = shadow.length - 1
        repeat(256) { typed ->
            val edit = CapturedLocalEditorEdit(anchor, "", "😁", 0L, shadow.length)
            val batch = prepareLocalEditorEditsUtil(shadow, listOf(edit))!!
            assertEquals(PreparedLocalEditorEdit(anchor - 20_000 - typed, 0, "😁"), batch.edits.single())
            shadow = batch.resultingShadow
            anchor += 2
            assertEquals(20_000 + typed + 1, shadow.length - shadow.codePointCount(0, shadow.length))
        }
        assertTrue("only small pieces are rescanned", surrogatePairScannedUtf16.get() - scanned < before.length)
        assertEquals(before.dropLast(1) + "😁".repeat(256) + "\n", shadow.toString())
    }

    @Test
    fun `an empty burst answers the shadow itself`() {
        val shadow = ReplicaShadow("Queue: 😀\n")
        val batch = prepareLocalEditorEditsUtil(shadow, emptyList())
        assertNotNull(batch)
        assertSame(shadow, batch!!.resultingShadow)
        assertEquals(emptyList<PreparedLocalEditorEdit>(), batch.edits)
    }

    @Test
    fun `UTF-8 hash preserves pairs across pieces and replaces malformed surrogates`() {
        val before = "x".repeat(4095) + "\uD83D" + "y".repeat(500) + "\uDE00"
        val shadow = ReplicaShadow(before).replace(4096, 4096, "\uDE00")
        val expected = before.take(4096) + "\uDE00" + before.drop(4096)
        assertEquals(expected, shadow.toString())
        assertEquals(expected.codePointCount(0, expected.length), shadow.codePointCount(0, shadow.length))
        assertEquals(contentHashUtil(expected), shadow.contentHash)
        assertEquals(contentHashUtil(before), ReplicaShadow(before).contentHash)
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
