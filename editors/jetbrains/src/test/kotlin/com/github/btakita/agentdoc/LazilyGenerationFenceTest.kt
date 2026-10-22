package com.github.btakita.agentdoc

import io.github.lazily.ThreadSafeContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

/** Consumer proof for the generation-fenced handles provided by lazily-kt 0.46.0. */
class LazilyGenerationFenceTest {
    @Test
    fun `stale disposal cannot retire a replacement in a recycled arena slot`() {
        val context = ThreadSafeContext()
        val stale = context.source("first")
        context.disposeCell(stale)

        val replacement = context.source("replacement")
        // The recycled public arena id makes the stale-handle collision
        // non-vacuous without depending on unpublished generation internals.
        assertEquals(stale.nodeId, replacement.nodeId)

        context.disposeCell(stale)

        assertFalse(context.isDisposed(replacement))
        assertEquals("replacement", context.get(replacement))
    }
}
