package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.TimeUnit

class NativeReloadCoordinatorTest {
    @Test
    fun `reload gate coalesces handoffs and releases waiting actions`() {
        val gate = NativeReloadGate()
        val handoff = gate.begin()

        assertTrue(handoff != null)
        assertNull(gate.begin())
        assertFalse(gate.awaitReady(1))

        gate.complete(requireNotNull(handoff))

        assertTrue(gate.awaitReady(1))
        assertTrue(gate.begin() != null)
    }

    @Test
    fun `manager waits share one reload deadline`() {
        val deadline = TimeUnit.MILLISECONDS.toNanos(5_000L)

        assertEquals(5_000L, nativeReloadRemainingWaitMillis(deadline, 0L))
        assertEquals(1L, nativeReloadRemainingWaitMillis(deadline, deadline - 1L))
        assertNull(nativeReloadRemainingWaitMillis(deadline, deadline))
        assertNull(nativeReloadRemainingWaitMillis(deadline, deadline + 1L))
    }
}
