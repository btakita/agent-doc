package com.github.btakita.agentdoc

import io.github.lazily.IngressOutcome
import io.github.lazily.keepLatest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RelayBackpressureTest {
    @Test
    fun `slow keyed consumer observes one coalesced newest value`() {
        val relay = KeyedCoalescingRelay<String, String>(keepLatest())

        assertEquals(IngressOutcome.Accepted, relay.ingress("a.md", "one"))
        assertEquals(IngressOutcome.Conflated, relay.ingress("a.md", "two"))
        assertEquals(IngressOutcome.Conflated, relay.ingress("a.md", "three"))
        assertEquals(IngressOutcome.Accepted, relay.ingress("b.md", "other"))
        assertEquals(2, relay.pendingKeyCount())

        assertEquals("a.md" to "three", relay.drainOne())
        assertTrue(relay.hasPending())
        assertEquals("b.md" to "other", relay.drainOne())
        assertFalse(relay.hasPending())
        assertNull(relay.drainOne())
    }
}
