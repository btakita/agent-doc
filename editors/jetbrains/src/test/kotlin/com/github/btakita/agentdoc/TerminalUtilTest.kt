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
    fun `successful route or status clears persisted route failure for document`() {
        val cwd = Files.createTempDirectory("agent-doc-jb-route-error-clear").toFile()
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = "[agent-doc] proof-timeout: accepted but unproven\n"
        val saved = TerminalUtil.persistRouteFailureOutput(
            cwd = cwd.path,
            relativePath = relativePath,
            routeOutput = output,
        )

        assertNotNull(saved)
        assertTrue(saved!!.isFile)
        assertTrue(TerminalUtil.clearPersistedRouteFailureOutput(cwd.path, relativePath))
        assertFalse(saved.exists())
        assertFalse(TerminalUtil.clearPersistedRouteFailureOutput(cwd.path, relativePath))
    }

    @Test
    fun `starting actor route failures are retryable`() {
        val output = """
            [route] target tmux session: 1
            Error: authoritative actor generation 22 for tasks/root.md owns pane %12 but route will not inject a new trigger because the authoritative actor is still starting.
        """.trimIndent()

        assertTrue(TerminalUtil.isStartingActorRouteFailure(output))
        assertFalse(TerminalUtil.isStartingActorRouteFailure("[agent-doc] proof-timeout: accepted but unproven"))
    }

    @Test
    fun `starting actor retry backoff uses bounded attempt delays`() {
        assertEquals(4, TerminalUtil.STARTING_ACTOR_ROUTE_MAX_ATTEMPTS)
        assertEquals(2_000L, TerminalUtil.startingActorRouteRetryDelayMillis(1))
        assertEquals(4_000L, TerminalUtil.startingActorRouteRetryDelayMillis(2))
        assertEquals(8_000L, TerminalUtil.startingActorRouteRetryDelayMillis(3))
        assertEquals(8_000L, TerminalUtil.startingActorRouteRetryDelayMillis(4))
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

    @Test
    fun `busy clear refusal parses protected pane details`() {
        val output = """
            Error: session_clear refused for /repo/tasks/agent-doc/agent-doc-bugs2.md because pane %1 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="gpt-5.5 high - ~/work/btakita/agent-loop - Context 15% used"). Run `agent-doc session status /repo/tasks/agent-doc/agent-doc-bugs2.md` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionClearRefusal(output)

        assertNotNull(refusal)
        assertEquals("/repo/tasks/agent-doc/agent-doc-bugs2.md", refusal!!.file)
        assertEquals("%1", refusal.pane)
        assertEquals("authoritative_actor", refusal.source)
        assertEquals("agent-doc", refusal.currentCommand)
        assertEquals("gpt-5.5 high - ~/work/btakita/agent-loop - Context 15% used", refusal.tail)
    }

    @Test
    fun `busy clear refusal message avoids generic command failure text`() {
        val message = TerminalUtil.buildBusySessionClearBlockedMessage(
            relativePath = "tasks/agent-doc/agent-doc-bugs2.md",
            refusal = TerminalUtil.BusySessionClearRefusal(
                file = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                pane = "%1",
                source = "authoritative_actor",
                currentCommand = "agent-doc",
                tail = "gpt-5.5 high - ~/work/btakita/agent-loop - Context 15% used",
            ),
        )

        assertTrue(message.contains("Session is still running"))
        assertTrue(message.contains("Pane %1 is busy (agent-doc)"))
        assertTrue(message.contains("retry Clear Session Context"))
        assertFalse(message.contains("agent-doc command failed"))
    }

    @Test
    fun `restart supervisor uses explicit supervisor session command`() {
        assertEquals(
            listOf(
                "agent-doc",
                "session",
                "restart-supervisor",
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
            TerminalUtil.buildSessionCommand(
                "agent-doc",
                listOf("restart-supervisor"),
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
        )
    }

    @Test
    fun `compact exchange uses committed exchange compact command`() {
        assertEquals(
            listOf(
                "agent-doc",
                "compact",
                "tasks/agent-doc/agent-doc-bugs2.md",
                "--component",
                "exchange",
                "--commit",
            ),
            TerminalUtil.buildCompactExchangeCommand(
                "agent-doc",
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
