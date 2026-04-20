package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test

class FileRenameListenerTest {

    @Test
    fun `shouldHandleFile returns true for md files`() {
        assertTrue(FileRenameListener.shouldHandleFile("plan.md"))
        assertTrue(FileRenameListener.shouldHandleFile("tasks/agent-doc-bugs.md"))
        assertTrue(FileRenameListener.shouldHandleFile("CHANGELOG.md"))
    }

    @Test
    fun `shouldHandleFile returns false for non-md files`() {
        assertFalse(FileRenameListener.shouldHandleFile("main.rs"))
        assertFalse(FileRenameListener.shouldHandleFile("package.json"))
        assertFalse(FileRenameListener.shouldHandleFile("style.css"))
        assertFalse(FileRenameListener.shouldHandleFile("readme.markdown"))
    }

    @Test
    fun `shouldHandleFile returns false for null`() {
        assertFalse(FileRenameListener.shouldHandleFile(null))
    }

    @Test
    fun `buildSyncCommand constructs correct args`() {
        val cmd = FileRenameListener.buildSyncCommand(
            "agent-doc",
            listOf("plan.md", "tasks/bugs.md"),
            "tasks/software/renamed.md"
        )
        assertEquals(
            listOf("agent-doc", "sync", "--col", "plan.md,tasks/bugs.md", "--focus", "tasks/software/renamed.md"),
            cmd
        )
    }

    @Test
    fun `buildSyncCommand handles single visible file`() {
        val cmd = FileRenameListener.buildSyncCommand(
            "/usr/local/bin/agent-doc",
            listOf("only-file.md"),
            "only-file.md"
        )
        assertEquals(
            listOf("/usr/local/bin/agent-doc", "sync", "--col", "only-file.md", "--focus", "only-file.md"),
            cmd
        )
    }

    @Test
    fun `buildSyncCommand handles empty visible files`() {
        val cmd = FileRenameListener.buildSyncCommand(
            "agent-doc",
            emptyList(),
            "renamed.md"
        )
        assertEquals(
            listOf("agent-doc", "sync", "--col", "", "--focus", "renamed.md"),
            cmd
        )
    }
}
