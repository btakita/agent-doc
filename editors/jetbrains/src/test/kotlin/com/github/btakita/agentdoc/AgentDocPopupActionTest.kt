package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AgentDocPopupActionTest {
    @Test
    fun `primary popup keeps claim numbered fourth and destructive clears out of line`() {
        assertEquals(
            listOf(
                "AgentDoc.Submit",
                "AgentDoc.InitSession",
                "AgentDoc.FixDocument",
                "AgentDoc.Claim",
                "AgentDoc.CompactExchange",
                "AgentDoc.ShowSessionStatus",
                "AgentDoc.RestartSupervisorProcess",
                "AgentDoc.RestartAgent",
                "AgentDoc.CancelTurn",
                "AgentDoc.CopySessionDiagnostics",
                "AgentDoc.SyncLayout",
                "AgentDoc.LoadTmuxWindow",
                "AgentDoc.RefreshEnvironment",
            ),
            AgentDocPopupAction.PRIMARY_ACTION_IDS,
        )
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.ClearSessionContext"))
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.InterruptClearSessionContext"))
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.RunWithJunie"))
        assertFalse(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.ForceClaim"))
    }

    @Test
    fun `overflow popup actions keep junie and force claim available`() {
        assertEquals(
            listOf(
                "AgentDoc.RunWithJunie",
                "AgentDoc.ForceClaim",
                "AgentDoc.ClearSessionContext",
                "AgentDoc.InterruptClearSessionContext",
                // #plugin-cleanup-menu-command: operator session-hygiene commands
                // live in the overflow group (occasional, project-scoped cleanup).
                "AgentDoc.ResyncFixSessions",
                "AgentDoc.GcStaleSessions",
            ),
            AgentDocPopupAction.OVERFLOW_ACTION_IDS,
        )
    }
}
