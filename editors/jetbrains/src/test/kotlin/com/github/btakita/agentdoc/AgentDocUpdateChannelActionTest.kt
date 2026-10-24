package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AgentDocUpdateChannelActionTest {
    @Test
    fun `repository selection follows the three disjoint IDE ranges`() {
        assertEquals(AgentDocUpdateChannelAction.CLASSIC_REPOSITORY_URL, AgentDocUpdateChannelAction.repositoryUrl(242))
        assertEquals(AgentDocUpdateChannelAction.CLASSIC_REPOSITORY_URL, AgentDocUpdateChannelAction.repositoryUrl(261))
        assertEquals(AgentDocUpdateChannelAction.EXACT_262_REPOSITORY_URL, AgentDocUpdateChannelAction.repositoryUrl(262))
        assertEquals(AgentDocUpdateChannelAction.EXACT_263_REPOSITORY_URL, AgentDocUpdateChannelAction.repositoryUrl(263))
        assertNull(AgentDocUpdateChannelAction.repositoryUrl(241))
        assertNull(AgentDocUpdateChannelAction.repositoryUrl(264))
    }

    @Test
    fun `toggle preserves foreign hosts and replaces stale agent doc ranges`() {
        val foreign = "https://plugins.jetbrains.com/plugins/eap/ideavim"
        val (enabledHosts, enabled) =
            AgentDocUpdateChannelAction.toggledHosts(
                listOf(foreign, AgentDocUpdateChannelAction.EXACT_262_REPOSITORY_URL),
                AgentDocUpdateChannelAction.EXACT_263_REPOSITORY_URL,
            )
        assertTrue(enabled)
        assertEquals(listOf(foreign, AgentDocUpdateChannelAction.EXACT_263_REPOSITORY_URL), enabledHosts)

        val (disabledHosts, disabled) =
            AgentDocUpdateChannelAction.toggledHosts(
                enabledHosts,
                AgentDocUpdateChannelAction.EXACT_263_REPOSITORY_URL,
            )
        assertFalse(disabled)
        assertEquals(listOf(foreign), disabledHosts)
    }
}
