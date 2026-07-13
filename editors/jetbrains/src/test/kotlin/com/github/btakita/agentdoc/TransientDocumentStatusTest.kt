package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TransientDocumentStatusTest {
    private val idle = TurnStateBridge.TurnStatePresentation("", false)

    @Test
    fun `transient compact status overrides controller projection until completion`() {
        val statuses = TransientDocumentStatus()
        val token = statuses.begin(
            "/repo/task.md",
            "⟳ agent-doc: Compacting Exchange",
            "Compacting and committing",
        )

        val active = statuses.presentationFor("/repo/task.md", idle)
        assertEquals("⟳ agent-doc: Compacting Exchange", active.label)
        assertEquals("Compacting and committing", active.tooltip)
        assertTrue(active.showBanner)
        assertTrue(statuses.finish("/repo/task.md", token))
        assertEquals(idle, statuses.presentationFor("/repo/task.md", idle))
    }

    @Test
    fun `stale completion cannot clear a newer operation`() {
        val statuses = TransientDocumentStatus()
        val first = statuses.begin("/repo/task.md", "first")
        val second = statuses.begin("/repo/task.md", "second")

        assertFalse(statuses.finish("/repo/task.md", first))
        assertEquals("second", statuses.presentationFor("/repo/task.md", idle).label)
        assertTrue(statuses.finish("/repo/task.md", second))
    }
}
