package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AgentDocPopupActionTest {
    @Test
    fun `primary popup keeps clear session context numbered ninth`() {
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
                "AgentDoc.ClearSessionContext",
                "AgentDoc.CancelTurn",
                "AgentDoc.CopySessionDiagnostics",
                "AgentDoc.SyncLayout",
                "AgentDoc.LoadTmuxWindow",
                "AgentDoc.RefreshEnvironment",
            ),
            AgentDocPopupAction.PRIMARY_ACTION_IDS,
        )
        assertEquals("AgentDoc.ClearSessionContext", AgentDocPopupAction.PRIMARY_ACTION_IDS[8])
        assertEquals(1, AgentDocPopupAction.PRIMARY_ACTION_IDS.count { it == "AgentDoc.ClearSessionContext" })
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
