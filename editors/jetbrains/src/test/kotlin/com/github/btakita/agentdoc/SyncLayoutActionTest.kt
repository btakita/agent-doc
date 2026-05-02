package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SyncLayoutActionTest {

    @Test
    fun `automatic sync commands opt out of autostart`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("tasks/one.md", "tasks/two.md"),
            editorLayout = null,
            focusedFile = "tasks/one.md",
            windowId = "@7",
            noAutostart = true,
        )

        assertTrue(cmd.contains("--no-autostart"))
    }

    @Test
    fun `manual sync commands keep autostart available`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("tasks/one.md"),
            editorLayout = null,
            focusedFile = "tasks/one.md",
            windowId = "@7",
            noAutostart = false,
        )

        assertFalse(cmd.contains("--no-autostart"))
    }
}
