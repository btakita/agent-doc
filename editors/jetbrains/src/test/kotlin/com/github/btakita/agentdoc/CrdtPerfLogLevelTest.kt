package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GH #65: routine off-EDT CRDT timing must not reach `idea.log` at WARN. The healthy session
 * in the issue measured p50=133ms, p99=343ms, max=395ms off the EDT; every one of those lines
 * was a WARN because the per-call thresholds were 50/100 ms.
 */
class CrdtPerfLogLevelTest {
    @Test
    fun `routine off-EDT latency is debug, a genuine stall still warns`() {
        for (elapsed in listOf(118L, 133L, 202L, 343L, 395L)) {
            assertFalse("off-EDT ${elapsed}ms must not warn", crdtPerfWarns(elapsed, 50L, onEdt = false))
            assertFalse("off-EDT ${elapsed}ms must not warn", crdtPerfWarns(elapsed, 100L, onEdt = false))
        }
        assertTrue(crdtPerfWarns(CRDT_OFF_EDT_WARN_FLOOR_MS, 100L, onEdt = false))
        assertTrue(crdtPerfWarns(2_500L, 100L, onEdt = false))
        // A caller threshold above the floor is kept.
        assertFalse(crdtPerfWarns(1_500L, 2_000L, onEdt = false))
    }

    @Test
    fun `the EDT keeps the caller's tight threshold`() {
        assertTrue(crdtPerfWarns(12L, 10L, onEdt = true))
        assertFalse(crdtPerfWarns(8L, 10L, onEdt = true))
        assertTrue(crdtPerfWarns(120L, 100L, onEdt = true))
    }
}
