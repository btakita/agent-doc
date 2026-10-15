package com.github.btakita.agentdoc.split.backend

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class BackendGuestSelectionGateTest {
    @Test
    fun fallbackPublishesGuestSelectionOnlyWithoutFrontendAuthority() {
        val gate = BackendGuestSelectionGate<String>()

        assertEquals(null to "a.md", gate.observe("a.md", frontendAvailable = false))
        assertNull(gate.observe("a.md", frontendAvailable = false))
        assertNull(gate.observe("b.md", frontendAvailable = true))
        assertEquals(null to "b.md", gate.observe("b.md", frontendAvailable = false))
        assertEquals("b.md" to "c.md", gate.observe("c.md", frontendAvailable = false))
    }
}
