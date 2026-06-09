package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class InvocationCoalescerTest {
    @Before
    fun reset() {
        InvocationCoalescer.resetForTest()
    }

    @Test
    fun `first invocation proceeds`() {
        assertTrue(InvocationCoalescer.shouldProceed("run:doc", 1_000L))
    }

    @Test
    fun `rapid re-fire within the window is coalesced`() {
        assertTrue(InvocationCoalescer.shouldProceed("run:doc", 1_000L))
        assertFalse("a second fire 100ms later must coalesce", InvocationCoalescer.shouldProceed("run:doc", 1_100L))
    }

    @Test
    fun `a re-fire after the window proceeds`() {
        assertTrue(InvocationCoalescer.shouldProceed("run:doc", 1_000L))
        assertTrue(
            "past the 750ms window the next deliberate fire proceeds",
            InvocationCoalescer.shouldProceed("run:doc", 1_000L + InvocationCoalescer.DEFAULT_WINDOW_MILLIS),
        )
    }

    @Test
    fun `different action kinds do not coalesce each other`() {
        val routeKey = "cwd::plan.md"
        assertTrue(InvocationCoalescer.shouldProceed(InvocationCoalescer.key("run", routeKey), 1_000L))
        // A Clear immediately after a Run for the same doc must still proceed —
        // only same-kind rapid re-fires collapse.
        assertTrue(InvocationCoalescer.shouldProceed(InvocationCoalescer.key("clear", routeKey), 1_010L))
    }

    @Test
    fun `different documents do not coalesce each other`() {
        assertTrue(InvocationCoalescer.shouldProceed("run:a.md", 1_000L))
        assertTrue(InvocationCoalescer.shouldProceed("run:b.md", 1_010L))
    }
}
