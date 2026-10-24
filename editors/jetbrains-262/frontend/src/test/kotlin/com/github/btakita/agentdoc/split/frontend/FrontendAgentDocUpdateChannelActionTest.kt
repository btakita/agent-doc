package com.github.btakita.agentdoc.split.frontend

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class FrontendAgentDocUpdateChannelActionTest {
    @Test
    fun modularRepositorySelectionUsesTheClientBuildRange() {
        assertEquals(
            FrontendUpdateChannelPolicy.CLASSIC_REPOSITORY_URL,
            FrontendUpdateChannelPolicy.repositoryUrl(242),
        )
        assertEquals(
            FrontendUpdateChannelPolicy.CLASSIC_REPOSITORY_URL,
            FrontendUpdateChannelPolicy.repositoryUrl(261),
        )
        assertEquals(
            FrontendUpdateChannelPolicy.EXACT_262_REPOSITORY_URL,
            FrontendUpdateChannelPolicy.repositoryUrl(262),
        )
        assertEquals(
            FrontendUpdateChannelPolicy.EXACT_263_REPOSITORY_URL,
            FrontendUpdateChannelPolicy.repositoryUrl(263),
        )
        assertNull(FrontendUpdateChannelPolicy.repositoryUrl(241))
        assertNull(FrontendUpdateChannelPolicy.repositoryUrl(264))
    }

    @Test
    fun clientTogglePreservesForeignHostsAndReplacesAStaleAgentDocRange() {
        val foreign = "https://plugins.jetbrains.com/plugins/eap/ideavim"
        val (enabledHosts, enabled) =
            FrontendUpdateChannelPolicy.toggledHosts(
                listOf(foreign, FrontendUpdateChannelPolicy.EXACT_262_REPOSITORY_URL),
                FrontendUpdateChannelPolicy.EXACT_263_REPOSITORY_URL,
            )

        assertTrue(enabled)
        assertEquals(listOf(foreign, FrontendUpdateChannelPolicy.EXACT_263_REPOSITORY_URL), enabledHosts)

        val (disabledHosts, disabled) =
            FrontendUpdateChannelPolicy.toggledHosts(
                enabledHosts,
                FrontendUpdateChannelPolicy.EXACT_263_REPOSITORY_URL,
            )

        assertFalse(disabled)
        assertEquals(listOf(foreign), disabledHosts)
    }

    @Test
    fun modularDescriptorOwnsTheUpdateChannelActionOnTheFrontend() {
        val descriptor = requireNotNull(javaClass.getResource("/agent.doc.frontend.xml")).readText()

        assertTrue(descriptor.contains("id=\"AgentDoc.UpdateChannel\""))
        assertTrue(descriptor.contains("FrontendAgentDocUpdateChannelAction"))
    }
}
