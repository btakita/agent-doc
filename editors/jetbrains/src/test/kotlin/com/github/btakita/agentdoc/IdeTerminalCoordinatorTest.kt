package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

class IdeTerminalCoordinatorTest {
    @Test
    fun `parses tmux ensure receipt`() {
        val outcome = parseIdeTerminalEnsureOutcome(
            """{"session_name":"dev","pane_id":"%3","attach_command":"tmux attach-session -t dev","created":true,"attached":false}""",
        )

        assertEquals("dev", outcome.sessionName)
        assertEquals("%3", outcome.paneId)
        assertEquals(true, outcome.created)
        assertEquals(false, outcome.attached)
    }

    @Test
    fun `external attached client remains authoritative`() {
        assertEquals(
            IdeTerminalAttachDecision.NOOP_EXTERNAL_ATTACHED,
            decideIdeTerminalAttach(sessionAttached = true, existingTabAlive = false),
        )
    }

    @Test
    fun `detached session opens or reuses IDE tab`() {
        assertEquals(
            IdeTerminalAttachDecision.CREATE_AND_ATTACH,
            decideIdeTerminalAttach(sessionAttached = false, existingTabAlive = false),
        )
        assertEquals(
            IdeTerminalAttachDecision.ATTACH_EXISTING,
            decideIdeTerminalAttach(sessionAttached = false, existingTabAlive = true),
        )
    }
}
