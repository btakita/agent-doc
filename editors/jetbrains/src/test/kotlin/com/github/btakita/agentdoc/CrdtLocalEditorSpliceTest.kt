package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class CrdtLocalEditorSpliceTest {
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
        assertEquals(3, prepared!!.size)
        assertEquals("Queue: Temporal", prepared.last().resultingText)
        assertEquals(listOf("Temp", "or", "al"), prepared.map { it.insert })
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
        assertEquals(2, prepared!!.size)
        assertEquals("one\ntwo", prepared.last().resultingText)
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
        assertEquals(1, prepared!!.single().offsetCodePoints)
        assertEquals(1, prepared.single().deleteCodePoints)
        assertEquals("a😁z", prepared.single().resultingText)
    }
}
