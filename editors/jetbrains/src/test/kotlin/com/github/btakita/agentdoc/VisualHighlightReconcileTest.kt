package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#keepidesnappy`: the visual refresh runs on the EDT after every typing pause.
 * It used to remove and re-add every agent-doc highlighter; now it touches only
 * the highlighters whose token actually changed.
 */
class VisualHighlightReconcileTest {
    private fun key(start: Int, end: Int, kind: String = "bold", attrs: Any? = "plain") =
        VisualHighlightKey(start = start, end = end, layer = 1, kind = kind, attributes = attrs)

    private fun manyTokens(count: Int): List<VisualHighlightKey> =
        (0 until count).map { key(it * 10, it * 10 + 4, if (it % 2 == 0) "bold" else "tracked_id") }

    @Test
    fun `unchanged tokens cost no markup mutation`() {
        val tokens = manyTokens(400)
        val existing = tokens.mapIndexed { index, key -> "h$index" to key }

        val plan = planVisualHighlightReconcile(existing, tokens)

        assertTrue("nothing to remove: ${plan.remove.size}", plan.remove.isEmpty())
        assertTrue("nothing to add: ${plan.add.size}", plan.add.isEmpty())
    }

    @Test
    fun `a keystroke that shifts ranges via range markers still matches`() {
        // The range markers moved every highlighter after offset 100 by one
        // character; the fresh token parse reports the same shifted offsets.
        val shifted = manyTokens(50).map { if (it.start >= 100) it.copy(start = it.start + 1, end = it.end + 1) else it }
        val existing = shifted.mapIndexed { index, key -> "h$index" to key }

        val plan = planVisualHighlightReconcile(existing, shifted)

        assertEquals(emptyList<String>(), plan.remove)
        assertEquals(emptyList<VisualHighlightKey>(), plan.add)
    }

    @Test
    fun `only the changed token is replaced`() {
        val before = listOf(key(0, 4), key(10, 14), key(20, 24))
        val after = listOf(key(0, 4), key(10, 16), key(20, 24))
        val existing = listOf("a" to before[0], "b" to before[1], "c" to before[2])

        val plan = planVisualHighlightReconcile(existing, after)

        assertEquals(listOf("b"), plan.remove)
        assertEquals(listOf(key(10, 16)), plan.add)
    }

    @Test
    fun `invalid or foreign highlighters are always removed`() {
        val existing = listOf("dead" to null, "live" to key(0, 4))

        val plan = planVisualHighlightReconcile(existing, listOf(key(0, 4)))

        assertEquals(listOf("dead"), plan.remove)
        assertTrue(plan.add.isEmpty())
    }

    @Test
    fun `a style change re-applies the highlighter`() {
        val existing = listOf("a" to key(0, 4, attrs = "plain"))

        val plan = planVisualHighlightReconcile(existing, listOf(key(0, 4, attrs = "bold-red")))

        assertEquals(listOf("a"), plan.remove)
        assertEquals(listOf(key(0, 4, attrs = "bold-red")), plan.add)
    }

    @Test
    fun `kind and layer are part of the identity`() {
        val existing = listOf("a" to key(0, 4, kind = "bold"))

        val plan = planVisualHighlightReconcile(existing, listOf(key(0, 4, kind = "italic")))

        assertEquals(listOf("a"), plan.remove)
        assertEquals(1, plan.add.size)
    }

    @Test
    fun `duplicate tokens are matched as a multiset`() {
        val dup = key(0, 4)
        val existing = listOf("a" to dup, "b" to dup, "c" to dup)

        val shrink = planVisualHighlightReconcile(existing, listOf(dup, dup))
        assertEquals(1, shrink.remove.size)
        assertTrue(shrink.add.isEmpty())

        val grow = planVisualHighlightReconcile(existing.take(1), listOf(dup, dup))
        assertTrue(grow.remove.isEmpty())
        assertEquals(listOf(dup), grow.add)
    }

    @Test
    fun `removing every token clears every highlighter`() {
        val tokens = manyTokens(5)
        val existing = tokens.mapIndexed { index, key -> "h$index" to key }

        val plan = planVisualHighlightReconcile(existing, emptyList())

        assertEquals(existing.map { it.first }.toSet(), plan.remove.toSet())
        assertTrue(plan.add.isEmpty())
    }
}
