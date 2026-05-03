package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class EditorTabSyncListenerTest {

    @Test
    fun `selection change keeps split layouts on sync when visible markdown set is unchanged`() {
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(
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
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = listOf("/repo/src/boost-client/tasks/monsterrodholders.md"),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(
                listOf("/repo/src/boost-client/tasks/monsterrodholders.md")
            ),
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Focus, plan?.kind)
    }

    @Test
    fun `visible markdown changes trigger non destructive sync`() {
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(
                listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")
            ),
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
                focusedFile = focusedFile,
                previousVisibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(visibleMdFiles),
                previousFocusedFile = focusedFile,
            )
        )
    }

    @Test
    fun `bounce back suppressed when focus returns to previous file within settle window`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val visibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(visibleMdFiles)

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature,
            previousFocusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            lastCommandCompletedAt = System.currentTimeMillis() - 500,
            focusedFileBeforeLastCommand = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.BounceBack, plan?.kind)
    }

    @Test
    fun `bounce back not suppressed after settle window expires`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val visibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(visibleMdFiles)

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = visibleSignature,
            previousFocusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            lastCommandCompletedAt = System.currentTimeMillis() - 2000,
            focusedFileBeforeLastCommand = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `bounce back not suppressed when visible set changes`() {
        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            focusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            previousVisibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(
                listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")
            ),
            previousFocusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            lastCommandCompletedAt = System.currentTimeMillis() - 500,
            focusedFileBeforeLastCommand = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }

    @Test
    fun `bounce back not suppressed when focus goes to a different file than previous`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/src/boost-client/tasks/monsterrodholders.md",
        )
        val visibleSignature = EditorTabSyncListener.AutomaticCommandPlanner.visibleSignature(visibleMdFiles)

        val plan = EditorTabSyncListener.AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
            previousVisibleSignature = visibleSignature,
            previousFocusedFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            lastCommandCompletedAt = System.currentTimeMillis() - 500,
            focusedFileBeforeLastCommand = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(EditorTabSyncListener.AutomaticCommandKind.Sync, plan?.kind)
    }
}
