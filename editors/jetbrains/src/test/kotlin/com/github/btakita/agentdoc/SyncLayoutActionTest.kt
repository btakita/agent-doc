package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SyncLayoutActionTest {

    @Test
    fun `automatic sync commands opt out of autostart`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/tasks/one.md", "/repo/tasks/two.md"),
            editorLayout = null,
            focusedFile = "/repo/tasks/one.md",
            noAutostart = true,
        )

        assertTrue(cmd.contains("--no-autostart"))
    }

    @Test
    fun `manual sync commands keep autostart available`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/tasks/one.md"),
            editorLayout = null,
            focusedFile = "/repo/tasks/one.md",
            noAutostart = false,
        )

        assertFalse(cmd.contains("--no-autostart"))
    }

    @Test
    fun `sync command never injects a window flag`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/tasks/one.md"),
            editorLayout = null,
            focusedFile = "/repo/tasks/one.md",
            noAutostart = false,
        )

        assertFalse(cmd.contains("--window"))
    }

    @Test
    fun `sync command preserves empty columns so sync can restore remembered panes`() {
        val cmd = SyncLayoutAction.buildSyncCommand(
            agentDoc = "agent-doc",
            visibleMdFiles = listOf("/repo/src/boost-client/tasks/monsterrodholders.md"),
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
                )
            ),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            noAutostart = false,
        )

        assertEquals(
            listOf(
                "agent-doc",
                "sync",
                "--col",
                "",
                "--col",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
                "--focus",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            cmd,
        )
    }

    @Test
    fun `normalize editor layout rewrites workspace relative files into submodule relative files`() {
        val normalized = SyncLayoutAction.normalizeEditorLayout(
            basePath = "/repo",
            projectRoot = "/repo/src/boost-client",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("src/boost-client/tasks/one.md")),
                    LayoutColumn(listOf("src/boost-client/tasks/two.md", "tasks/ignored.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("tasks/one.md")),
                    LayoutColumn(listOf("tasks/two.md", "/repo/tasks/ignored.md")),
                )
            ),
            normalized,
        )
    }

    @Test
    fun `normalize editor layout preserves empty columns for non markdown siblings`() {
        val normalized = SyncLayoutAction.normalizeEditorLayout(
            basePath = "/repo",
            projectRoot = "/repo/src/boost-client",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("src/boost-client/tasks/monsterrodholders.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("tasks/monsterrodholders.md")),
                )
            ),
            normalized,
        )
    }

    @Test
    fun `normalize editor layout preserves cross root markdown files as absolute paths`() {
        val normalized = SyncLayoutAction.normalizeEditorLayout(
            basePath = "/repo",
            projectRoot = "/repo/src/boost-client",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("tasks/agent-doc/agent-doc-bugs2.md")),
                    LayoutColumn(listOf("src/boost-client/tasks/monsterrodholders.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
                    LayoutColumn(listOf("tasks/monsterrodholders.md")),
                )
            ),
            normalized,
        )
    }

    @Test
    fun `absolutize editor layout rewrites project relative files into absolute paths`() {
        val absolute = SyncLayoutAction.absolutizeEditorLayout(
            projectRoot = "/repo/src/boost-client",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("tasks/one.md")),
                    LayoutColumn(listOf("/already/absolute.md", "tasks/two.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/src/boost-client/tasks/one.md")),
                    LayoutColumn(listOf("/already/absolute.md", "/repo/src/boost-client/tasks/two.md")),
                )
            ),
            absolute,
        )
    }

    @Test
    fun `absolutize editor layout preserves empty columns`() {
        val absolute = SyncLayoutAction.absolutizeEditorLayout(
            projectRoot = "/repo/src/boost-client",
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("tasks/monsterrodholders.md")),
                )
            ),
        )

        assertEquals(
            EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
                )
            ),
            absolute,
        )
    }

    @Test
    fun `collect visible markdown files keeps cross root paths`() {
        val visible = arrayOf(
            FakeVirtualFile("/repo/tasks/agent-doc/agent-doc-bugs2.md"),
            FakeVirtualFile("/repo/src/boost-client/tasks/monsterrodholders.md"),
            FakeVirtualFile("/repo/notes/todo.txt"),
        )

        assertEquals(
            listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            SyncLayoutAction.collectVisibleMarkdownFiles(visible),
        )
    }

    private class FakeVirtualFile(private val rawPath: String) :
        com.intellij.testFramework.LightVirtualFile(
            java.io.File(rawPath).name,
            ""
        ) {
        override fun getPath(): String = rawPath
    }
}
