package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TerminalUtilTest {

    @Test
    fun `replacing an alive route cancels the stale run`() {
        val registry = TerminalUtil.InFlightRouteRegistry()
        val stale = FakeRouteHandle(alive = true)
        val next = FakeRouteHandle(alive = true)

        assertFalse(registry.replace("doc", stale))
        assertTrue(registry.replace("doc", next))
        assertEquals(1, stale.cancelCount)
        assertFalse(next.wasCanceled())
    }

    @Test
    fun `clearing a stale handle does not remove the newer run`() {
        val registry = TerminalUtil.InFlightRouteRegistry()
        val stale = FakeRouteHandle(alive = true)
        val next = FakeRouteHandle(alive = true)

        registry.replace("doc", stale)
        registry.replace("doc", next)
        registry.clearIfCurrent("doc", stale)

        assertTrue(registry.replace("doc", FakeRouteHandle(alive = true)))
        assertEquals(1, next.cancelCount)
    }

    @Test
    fun `route layout args preserve cross root split as absolute paths`() {
        val args = TerminalUtil.buildRouteLayoutArgs(
            visibleMdFiles = listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
                    LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
                )
            ),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
        )

        assertEquals(
            listOf(
                "--col",
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "--col",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
                "--focus",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            args,
        )
    }

    @Test
    fun `route layout args preserve empty split columns for mixed layouts`() {
        val args = TerminalUtil.buildRouteLayoutArgs(
            visibleMdFiles = listOf("/repo/src/boost-client/tasks/monsterrodholders.md"),
            editorLayout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("/repo/src/boost-client/tasks/monsterrodholders.md")),
                )
            ),
            focusedFile = "/repo/src/boost-client/tasks/monsterrodholders.md",
        )

        assertEquals(
            listOf(
                "--col",
                "",
                "--col",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
                "--focus",
                "/repo/src/boost-client/tasks/monsterrodholders.md",
            ),
            args,
        )
    }

    private class FakeRouteHandle(private var alive: Boolean) : TerminalUtil.InFlightRouteHandle {
        var cancelCount: Int = 0
            private set

        override fun isAlive(): Boolean = alive

        override fun cancelForReplacement() {
            cancelCount += 1
            alive = false
        }

        override fun wasCanceled(): Boolean = cancelCount > 0
    }
}
