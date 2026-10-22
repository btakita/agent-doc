package com.github.btakita.agentdoc.split.backend

import io.github.lazily.ThreadSafeContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Test

/** Consumer proof for the generation-fenced handles provided by lazily-kt 0.46.0. */
class LazilyGenerationFenceTest {
    @Test
    fun `stale disposal cannot retire a replacement in a recycled arena slot`() {
        val context = ThreadSafeContext()
        val stale = context.source("first")
        context.disposeCell(stale)

        val replacement = context.source("replacement")
        assertEquals(stale.nodeId, replacement.nodeId)
        assertNotEquals(stale.nodeGeneration, replacement.nodeGeneration)

        context.disposeCell(stale)

        assertFalse(context.isDisposed(replacement))
        assertEquals("replacement", context.get(replacement))
    }
}
