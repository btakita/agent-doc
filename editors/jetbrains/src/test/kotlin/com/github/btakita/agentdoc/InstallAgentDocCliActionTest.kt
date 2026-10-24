package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InstallAgentDocCliActionTest {
    @Test
    fun `installer action uses the official checksummed shell installer`() {
        assertEquals(
            listOf("sh", "-c", InstallAgentDocCliAction.INSTALL_COMMAND),
            InstallAgentDocCliAction.INSTALL_COMMAND_ARGS,
        )
        assertTrue(InstallAgentDocCliAction.INSTALL_COMMAND.startsWith("curl -fsSL https://"))
        assertTrue(InstallAgentDocCliAction.INSTALL_COMMAND.endsWith(" | sh"))
        assertTrue(InstallAgentDocCliAction.confirmationMessage().contains(InstallAgentDocCliAction.INSTALL_COMMAND))
    }

    @Test
    fun `installer action is project scoped`() {
        assertTrue(InstallAgentDocCliAction.shouldEnable(hasProject = true))
        assertFalse(InstallAgentDocCliAction.shouldEnable(hasProject = false))
    }

    @Test
    fun `long installer failures retain the useful tail`() {
        val output = "prefix" + "x".repeat(2_100) + " final error"
        val concise = InstallAgentDocCliAction.conciseFailure(output)
        assertTrue(concise.startsWith("…"))
        assertTrue(concise.endsWith("final error"))
        assertTrue(concise.length <= 2_001)
    }
}
