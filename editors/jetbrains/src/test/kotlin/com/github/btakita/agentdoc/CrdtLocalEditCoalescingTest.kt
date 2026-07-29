package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class CrdtLocalEditCoalescingTest {
    @Test
    fun `typing burst collapses to one exact splice`() {
        val before = "alpha beta omega"
        val after = "alpha quick brown omega"

        val edit = coalescedLocalEditUtil(before, after)
        assertNotNull(edit)
        edit!!

        assertEquals(6, edit.offsetCodePoints)
        assertEquals(4, edit.deleteCodePoints)
        assertEquals("quick brown", edit.insert)
        assertEquals(after, applyEdit(before, edit))
    }

    @Test
    fun `splice boundaries never split a surrogate pair`() {
        val before = "a😀z"
        val after = "a😁z"

        val edit = coalescedLocalEditUtil(before, after)
        assertNotNull(edit)
        edit!!

        assertEquals(1, edit.offsetCodePoints)
        assertEquals(1, edit.deleteCodePoints)
        assertEquals("😁", edit.insert)
        assertEquals(after, applyEdit(before, edit))
    }

    @Test
    fun `unchanged editor cut needs no delta`() {
        assertNull(coalescedLocalEditUtil("same", "same"))
    }

    private fun applyEdit(before: String, edit: CoalescedLocalEdit): String {
        val start = before.offsetByCodePoints(0, edit.offsetCodePoints)
        val end = before.offsetByCodePoints(start, edit.deleteCodePoints)
        return before.substring(0, start) + edit.insert + before.substring(end)
    }
}
