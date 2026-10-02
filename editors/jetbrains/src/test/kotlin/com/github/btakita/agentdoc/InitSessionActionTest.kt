package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

class InitSessionActionTest {
    @Test
    fun `action delegates session branching to binary`() {
        assertEquals(
            listOf("agent-doc", "init-session", "tasks/new-session.md"),
            InitSessionAction.buildCommand("agent-doc", "tasks/new-session.md"),
        )
    }
}
