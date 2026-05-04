package com.github.btakita.agentdoc

import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
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

    @Test
    fun `route failure diagnostics path stays under project state directory`() {
        val file = TerminalUtil.routeFailureDiagnosticsFile(
            cwd = "/repo",
            relativePath = "tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(
            "/repo/.agent-doc/state/editor-route-errors/tasks__agent-doc__agent-doc-bugs2.md.txt",
            file.path,
        )
    }

    @Test
    fun `persisted route failure keeps exact binary output`() {
        val cwd = Files.createTempDirectory("agent-doc-jb-route-error").toFile()
        val output = "[agent-doc] startup-miss: routed trigger accepted but no document cycle started\n"

        val saved = TerminalUtil.persistRouteFailureOutput(
            cwd = cwd.path,
            relativePath = "tasks/agent-doc/agent-doc-bugs2.md",
            routeOutput = output,
        )

        assertNotNull(saved)
        assertTrue(saved!!.isFile)
        assertEquals(output, saved.readText())
    }

    @Test
    fun `session status success keeps exact cli output`() {
        val output = "generation=4\nstate=waiting_input\npane=%12"

        assertEquals(
            output,
            TerminalUtil.sessionStatusSuccessMessage("tasks/agent-doc/agent-doc-bugs2.md", output),
        )
    }

    @Test
    fun `clear session context uses actor-backed session command`() {
        assertEquals(
            listOf(
                "agent-doc",
                "session",
                "clear",
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
            TerminalUtil.buildSessionCommand(
                "agent-doc",
                listOf("clear"),
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
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
