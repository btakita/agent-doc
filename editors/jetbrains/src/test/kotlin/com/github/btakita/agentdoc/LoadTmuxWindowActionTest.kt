package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LoadTmuxWindowActionTest {
    @Test
    fun `enabled only for markdown documents`() {
        assertTrue(LoadTmuxWindowAction.shouldEnable("md"))
        assertTrue(LoadTmuxWindowAction.shouldEnable("MD"))
        assertFalse(LoadTmuxWindowAction.shouldEnable("txt"))
        assertFalse(LoadTmuxWindowAction.shouldEnable(null))
    }

    @Test
    fun `load tmux window action is offered in the popup`() {
        assertTrue(AgentDocPopupAction.PRIMARY_ACTION_IDS.contains("AgentDoc.LoadTmuxWindow"))
    }
}
