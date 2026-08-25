package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Test

class IdeTerminalCoordinatorTest {
    @Test
    fun `parses tmux ensure receipt`() {
        val outcome = parseIdeTerminalEnsureOutcome(
            """{"session_name":"dev","pane_id":"%3","attach_command":"tmux attach-session -t dev","created":true,"attached":false,"terminal_host":"ide","terminal_host_reason":"configured IDE host","auto_start_tmux":true}""",
        )

        assertEquals("dev", outcome.sessionName)
        assertEquals("%3", outcome.paneId)
        assertEquals(true, outcome.created)
        assertEquals(false, outcome.attached)
        assertEquals("ide", outcome.terminalHost)
        assertEquals(true, outcome.autoStartTmux)
    }

    @Test
    fun `external attached client remains authoritative`() {
        assertEquals(
            IdeTerminalAttachDecision.NOOP_EXTERNAL_ATTACHED,
            decideIdeTerminalAttach(
                terminalHost = "none",
                sessionAttached = true,
                existingTabAlive = false,
            ),
        )
    }

    @Test
    fun `detached session opens or reuses IDE tab`() {
        assertEquals(
            IdeTerminalAttachDecision.CREATE_AND_ATTACH,
            decideIdeTerminalAttach(
                terminalHost = "ide",
                sessionAttached = false,
                existingTabAlive = false,
            ),
        )
        assertEquals(
            IdeTerminalAttachDecision.ATTACH_EXISTING,
            decideIdeTerminalAttach(
                terminalHost = "ide",
                sessionAttached = false,
                existingTabAlive = true,
            ),
        )
    }

    @Test
    fun `configured external host does not open an IDE tab`() {
        assertEquals(
            IdeTerminalAttachDecision.NOOP_CONFIGURED_HOST,
            decideIdeTerminalAttach(
                terminalHost = "external",
                sessionAttached = false,
                existingTabAlive = false,
            ),
        )
    }
}
