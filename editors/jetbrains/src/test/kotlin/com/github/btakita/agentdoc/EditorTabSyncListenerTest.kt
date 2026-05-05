package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class EditorTabSyncListenerTest {

    private fun visibleSignature(
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout? = null,
    ): String = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(
        visibleMdFiles = visibleMdFiles,
        editorLayout = editorLayout,
    )

    @Test
    fun `selection change keeps split layouts on sync when visible markdown set is unchanged`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature(
                listOf(
                    "/repo/src/boost-client/tasks/monsterrodholders.md",
                    "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                )
            ),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `single visible markdown file still uses focus when selection changes`() {
        val visibleMdFiles = listOf("/repo/src/boost-client/tasks/monsterrodholders.md")
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature(visibleMdFiles),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Focus, plan?.kind)
    }

    @Test
    fun `visible markdown changes trigger non destructive sync`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `unchanged selection state does not rerun commands`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md"

        assertNull(
            EditorTabSyncListener.AutomaticCommandPlanner.plan(
                visibleMdFiles = visibleMdFiles,
                visibleSignature = visibleSignature(visibleMdFiles),
                focusedFile = focusedFile,
                previousVisibleSignature = visibleSignature(visibleMdFiles),
                previousFocusedFile = focusedFile,
            )
        )
    }

    @Test
    fun `opposite pane selection still dispatches sync when visible split is unchanged`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val visibleSignature = visibleSignature(visibleMdFiles)

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature,
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature,
            previousFocusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `visible set changes still dispatch sync`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles),
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
            previousFocusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `selection change to a different file still dispatches sync`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val visibleSignature = visibleSignature(visibleMdFiles)

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature,
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature,
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `newer automatic sync generations replay after the running command finishes`() {
        assertEquals(true, EditorTabSyncListener.AutomaticCommandPlanner.shouldReplayAfterRun(3, 4))
        assertEquals(false, EditorTabSyncListener.AutomaticCommandPlanner.shouldReplayAfterRun(4, 4))
    }

    @Test
    fun `column aware signatures preserve splitter identity for replayed requests`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val previousLayout = EditorLayout(
            columns = listOf(
                LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
                LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
            )
        )
        val latestLayout = EditorLayout(
            columns = listOf(
                LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
                LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
            )
        )

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            visibleSignature = visibleSignature(visibleMdFiles, latestLayout),
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature(visibleMdFiles, previousLayout),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }
}
