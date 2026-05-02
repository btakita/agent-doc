package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class EditorTabSyncListenerTest {

    @Test
    fun `selection change prefers focus when visible markdown set is unchanged`() {
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
}
