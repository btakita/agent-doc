package com.github.btakita.agentdoc

import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TerminalUtilTest {

    @Test
    fun `run route command requests plain trigger for editor dispatch`() {
        assertEquals(
            listOf(
                "/usr/local/bin/agent-doc",
                "route",
                "--dispatch-only",
                "--plain-trigger",
                "--wait-for-ready",
                "60",
                "tasks/root.md",
            ),
            TerminalUtil.buildRunRouteCommand("/usr/local/bin/agent-doc", "tasks/root.md"),
        )
    }

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
    fun `interrupt clear uses explicit session operator command`() {
        assertEquals(
            listOf(
                "agent-doc",
                "session",
                "interrupt-clear",
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
            TerminalUtil.buildSessionCommand(
                "agent-doc",
                listOf("interrupt-clear"),
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
    fun `busy clear refusal parses generic command wrapper output`() {
        val output = """
            agent-doc command failed (exit 1): Error: session_clear refused for /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md because pane %13 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="● Tip Override global tool settings per agent configuration"). Run `agent-doc session status /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionClearRefusal(output)

        assertNotNull(refusal)
        assertEquals("/home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md", refusal!!.file)
        assertEquals("%13", refusal.pane)
        assertEquals("authoritative_actor", refusal.source)
        assertEquals("agent-doc", refusal.currentCommand)
        assertEquals("● Tip Override global tool settings per agent configuration", refusal.tail)
    }

    @Test
    fun `busy clear refusal parses active agent doc wrapper output`() {
        val output = """
            agent-doc command failed (exit 1): Error: session_clear refused for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because pane %23 is active_agent_doc (source=authoritative_actor, current_command=agent-doc, tail="gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 41% used"). Wait for an idle prompt, or run `agent-doc session interrupt-clear /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` to intentionally interrupt the pane and clear context.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionClearRefusal(output)

        assertNotNull(refusal)
        assertEquals("/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md", refusal!!.file)
        assertEquals("%23", refusal.pane)
        assertEquals("authoritative_actor", refusal.source)
        assertEquals("agent-doc", refusal.currentCommand)
        assertEquals("gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 41% used", refusal.tail)
    }

    @Test
    fun `busy clear refusal keeps quoted or parenthesized pane tail text`() {
        val output = """
            Error: session_clear refused for /repo/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="Tip says \"Override\" settings (per agent)"). Run `agent-doc session status /repo/tasks/root.md` and wait for an idle prompt.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionClearRefusal(output)

        assertNotNull(refusal)
        assertEquals("Tip says \"Override\" settings (per agent)", refusal!!.tail)
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
        assertTrue(message.contains("Refresh and retry"))
        assertTrue(message.contains("Interrupt and clear"))
        assertFalse(message.contains("agent-doc command failed"))
    }

    @Test
    fun `protected prompt input clear refusal parses protected reason`() {
        val output = """
            Error: session_clear refused for /repo/tasks/root.md because pane %2 contains protected prompt input (reason=drafted prompt input, source=authoritative_actor, current_command=agent-doc, tail="› unfinished prompt"). Clear the prompt input manually, or run `agent-doc session interrupt-clear /repo/tasks/root.md` to intentionally interrupt the pane and clear context.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionClearRefusal(output)

        assertNotNull(refusal)
        assertEquals("/repo/tasks/root.md", refusal!!.file)
        assertEquals("%2", refusal.pane)
        assertEquals("drafted prompt input", refusal.protectedReason)
        assertEquals("› unfinished prompt", refusal.tail)
    }

    @Test
    fun `protected prompt input clear warning omits refresh retry guidance`() {
        val message = TerminalUtil.buildBusySessionClearBlockedMessage(
            relativePath = "tasks/root.md",
            refusal = TerminalUtil.BusySessionClearRefusal(
                file = "/repo/tasks/root.md",
                pane = "%2",
                source = "authoritative_actor",
                currentCommand = "agent-doc",
                tail = "› unfinished prompt",
                protectedReason = "drafted prompt input",
            ),
        )

        assertTrue(message.contains("protected prompt input"))
        assertTrue(message.contains("Interrupt and clear"))
        assertFalse(message.contains("Refresh and retry"))
    }

    @Test
    fun `session status idle direct pane enables refresh retry clear`() {
        val output = """
            document: /repo/tasks/root.md
            actor: generation=41 pane=%2 window=@1 state=busy
            live_pane: state=alive-idle pane=%2 source=authoritative_actor current_command=agent-doc prompt_ready=true tail=>
            supervisor: health=healthy state=healthy actor_state=busy restart_count=0 socket=/tmp/sup.sock
            controller_lease: generation=41 pid=100 runtime_state=busy heartbeat=2026-05-12T00:00:00Z socket=/tmp/sup.sock
        """.trimIndent()

        assertTrue(TerminalUtil.sessionStatusShowsIdleDirectPane(output))
        assertFalse(TerminalUtil.sessionStatusShowsIdleDirectPane(output.replace("alive-idle", "alive-busy")))
    }

    @Test
    fun `session status ready actor does not enable refresh retry while live pane is busy`() {
        val output = """
            document: /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md
            session_id: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
            harness: codex
            actor: generation=183 pane=%23 window=@15 state=ready
            actor_last_transition: caller=route reason=dispatch_bind prior_generation=183 new_generation=183 at=2026-05-19T07:49:04Z
            registry: pane=%23 window=@15 pid=2142801 cwd=/home/brian/work/btakita/agent-loop supervisor_instance_id=none
            live_pane: state=alive-busy pane=%23 source=authoritative_actor current_command=agent-doc prompt_ready=false tail=gpt-5.5 high · ~/work/btakita/agent-loop · Context 17% used
            supervisor: health=healthy state=healthy actor_state=ready restart_count=0 socket=/home/brian/work/btakita/agent-loop/.agent-doc/supervisor/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.sock
            supervisor_process: pid=2192763 child_pid=2192893 cwd_source=project_root instance_id=f4ea4a3c-3fd0-42ae-8870-3ae4558e115e
            startup_miss: none
            session_log: latest_start_pane=%23 latest_open=true committed_after_latest_run=true last_event=document_cycle phase=preflight_started cycle=cycle-1779176949689 event=preflight_started
            codex_capability_proof: proven
            controller_lease: generation=183 pid=2192763 runtime_state=ready heartbeat=2026-05-19T07:49:44Z socket=/home/brian/work/btakita/agent-loop/.agent-doc/supervisor/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.sock
            controller_last_command: kind=dispatch_only_reopen accepted_stage=ready failed_stage=none at=2026-05-19T07:49:04Z
        """.trimIndent()

        assertFalse(TerminalUtil.sessionStatusShowsIdleDirectPane(output))
    }

    @Test
    fun `session status exposes projection and live pane evidence for stale busy diagnosis`() {
        val output = """
            document: /repo/tasks/root.md
            actor: generation=41 pane=%2 window=@1 state=busy
            live_pane: state=alive-idle pane=%2 source=authoritative_actor current_command=agent-doc prompt_ready=true tail=>
            supervisor: health=healthy state=healthy actor_state=busy restart_count=0 socket=/tmp/sup.sock
            controller_lease: generation=41 pid=100 runtime_state=busy heartbeat=2026-05-12T00:00:00Z socket=/tmp/sup.sock
        """.trimIndent()

        val message = TerminalUtil.sessionStatusSuccessMessage("tasks/root.md", output)

        assertEquals(output, message)
        assertTrue(message.contains("actor:"))
        assertTrue(message.contains("live_pane: state=alive-idle"))
        assertTrue(message.contains("supervisor:"))
        assertTrue(message.contains("controller_lease:"))
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
    fun `forced restart supervisor keeps force flag inside session command`() {
        assertEquals(
            listOf(
                "agent-doc",
                "session",
                "restart-supervisor",
                "--force",
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
            TerminalUtil.buildSessionCommand(
                "agent-doc",
                listOf("restart-supervisor", "--force"),
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
        )
    }

    @Test
    fun `busy restart refusal parses force guidance details`() {
        val output = """
            agent-doc command failed (exit 1): Error: session_restart refused for /repo/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="gpt-5 high - ~/repo - Context 20% used"). Run `agent-doc session status /repo/tasks/root.md` and wait for an idle prompt, or pass `--force` to interrupt the running turn and restart anyway.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionRestartRefusal(output)

        assertNotNull(refusal)
        assertEquals("/repo/tasks/root.md", refusal!!.file)
        assertEquals("%2", refusal.pane)
        assertEquals("authoritative_actor", refusal.source)
        assertEquals("agent-doc", refusal.currentCommand)
        assertEquals("gpt-5 high - ~/repo - Context 20% used", refusal.tail)
    }

    @Test
    fun `busy restart refusal message exposes interrupt restart action`() {
        val message = TerminalUtil.buildBusySessionRestartBlockedMessage(
            relativePath = "tasks/root.md",
            refusal = TerminalUtil.BusySessionRestartRefusal(
                file = "/repo/tasks/root.md",
                pane = "%2",
                source = "authoritative_actor",
                currentCommand = "agent-doc",
                tail = "gpt-5 high - ~/repo - Context 20% used",
            ),
        )

        assertTrue(message.contains("Restart Supervisor is blocked"))
        assertTrue(message.contains("Pane %2 is busy (agent-doc)"))
        assertTrue(message.contains("Interrupt and restart"))
        assertFalse(message.contains("agent-doc command failed"))
    }

    @Test
    fun `restart telemetry parses force recovery ops log events for document`() {
        val lines = listOf(
            "[1] session_restart_force_used file=/repo/tasks/other.md pane=%9 source=authoritative_actor state=alive-busy current_command=agent-doc prompt_ready=false tail=\"other\"",
            "[2] session_restart_force_used file=/repo/tasks/root.md pane=%2 source=authoritative_actor state=alive-busy current_command=agent-doc prompt_ready=false tail=\"running\"",
            "[3] session_restart_busy_force_killed file=/repo/tasks/root.md pane=%2 source=authoritative_actor state=closed current_command=agent-doc prompt_ready=false tail=\"exited\"",
        )

        val telemetry = TerminalUtil.parseRestartSupervisorTelemetry(lines, "/repo", "tasks/root.md")

        assertNotNull(telemetry)
        assertTrue(telemetry!!.forceUsed)
        assertTrue(telemetry.busyForceKilled)
        assertFalse(telemetry.busyPreInterruptIdle)
        assertEquals("%2", telemetry.pane)
        assertEquals("closed", telemetry.state)
        assertEquals("agent-doc", telemetry.currentCommand)
        assertEquals(
            listOf("session_restart_force_used", "session_restart_busy_force_killed"),
            telemetry.eventNames,
        )
    }

    @Test
    fun `restart telemetry ignores unrelated documents`() {
        val lines = listOf(
            "[1] session_restart_force_used file=/repo/tasks/other.md pane=%9 source=authoritative_actor state=alive-busy current_command=agent-doc prompt_ready=false tail=\"other\"",
        )

        assertNull(TerminalUtil.parseRestartSupervisorTelemetry(lines, "/repo", "tasks/root.md"))
    }

    @Test
    fun `restart success message surfaces force telemetry events`() {
        val message = TerminalUtil.restartSessionSuccessMessage(
            relativePath = "tasks/root.md",
            output = "Requested continue restart for /repo/tasks/root.md (controller stage ready).",
            telemetry = TerminalUtil.RestartSupervisorTelemetry(
                forceUsed = true,
                busyPreInterruptIdle = false,
                busyForceKilled = true,
                pane = "%2",
                state = "closed",
                currentCommand = "agent-doc",
                eventNames = listOf("session_restart_force_used", "session_restart_busy_force_killed"),
            ),
        )

        assertTrue(message.contains("Requested continue restart"))
        assertTrue(message.contains("forced restart interrupted a busy pane"))
        assertTrue(message.contains("pane %2"))
        assertTrue(message.contains("Events: session_restart_force_used, session_restart_busy_force_killed"))
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
