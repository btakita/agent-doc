package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AgentDocPopupActionTest {
    @Test
    fun `primary popup actions keep compact exchange and supervisor restart numbered`() {
        assertTrue(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.CompactExchange"))
        assertTrue(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.RestartSupervisorProcess"))
        val supervisorIndex = AgentDocPopupAction.PRIMARY_ACTION_IDS.indexOf("AgentDoc.RestartSupervisorProcess")
        assertEquals("AgentDoc.RestartAgent", AgentDocPopupAction.PRIMARY_ACTION_IDS[supervisorIndex + 1])
        assertTrue(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.CancelTurn"))
        assertTrue(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.InterruptClearSessionContext"))
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.RunWithJunie"))
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.ForceClaim"))
    }

    @Test
    fun `overflow popup actions keep junie and force claim available`() {
        assertEquals(
            listOf(
                "AgentDoc.RunWithJunie",
                "AgentDoc.ForceClaim",
                // #plugin-cleanup-menu-command: operator session-hygiene commands
                // live in the overflow group (occasional, project-scoped cleanup).
                "AgentDoc.ResyncFixSessions",
                "AgentDoc.GcStaleSessions",
            ),
            AgentDocPopupAction.OVERFLOW_ACTION_IDS,
        )
    }
}
