package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
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
                "--debounce",
                "0",
                "--wait-for-ready",
                "120",
                "tasks/root.md",
            ),
            TerminalUtil.buildRunRouteCommand("/usr/local/bin/agent-doc", "tasks/root.md"),
        )
    }

    @Test
    fun `run route records durable attempt stages`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()

        assertTrue(source.contains("attempt?.recordIfCurrent(\"route_prepare\")"))
        assertTrue(source.contains("attempt?.recordIfCurrent(\"route_command_built\", command = cmd)"))
        assertTrue(source.contains("attempt?.recordIfCurrent(\"route_start\", command = cmd)"))
        assertTrue(source.contains("AGENT_DOC_EDITOR_ROUTE_ATTEMPT_ID"))
        assertTrue(source.contains("AGENT_DOC_EDITOR_ROUTE_KEY"))
        assertTrue(source.contains("\"route_retryable_starting\""))
        assertTrue(source.contains("attempt?.finishIfCurrent(stage, command = cmd, error = finalError)"))
    }

    @Test
    fun `restart agent action records ops log menu invocation marker before restart`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/RestartAgentAction.kt"
        ).toFile().readText()

        assertTrue(source.contains("TerminalUtil.recordRestartAgentMenuInvoked(project, file)"))
        assertTrue(source.indexOf("recordRestartAgentMenuInvoked") < source.indexOf("restartSession(project, file)"))
    }

    @Test
    fun `restart agent menu invocation marker is optverify scannable`() {
        val line = TerminalUtil.buildRestartAgentMenuInvokedOpsLogLine(
            timestamp = "2026-06-24T12:34:56Z",
            relativePath = "tasks/agent-doc/agent-doc-bugs2.md",
        )

        assertEquals(
            "[2026-06-24T12:34:56Z] restart_agent_menu_invoked file=tasks/agent-doc/agent-doc-bugs2.md source=jetbrains action=restart_agent doc=agent-doc-bugs2",
            line,
        )
        assertTrue(line.contains("restart_agent_menu_invoked"))
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
    fun `starting a route while one is alive keeps the first submitter`() {
        val registry = TerminalUtil.InFlightRouteRegistry()
        val active = FakeRouteHandle(alive = true)
        val duplicate = FakeRouteHandle(alive = true)

        assertTrue(registry.startIfIdle("doc", active))
        assertFalse(registry.startIfIdle("doc", duplicate))

        assertEquals(0, active.cancelCount)
        assertEquals(0, duplicate.cancelCount)
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
    fun `canceling active route removes and cancels current run`() {
        val registry = TerminalUtil.InFlightRouteRegistry()
        val active = FakeRouteHandle(alive = true)
        val next = FakeRouteHandle(alive = true)

        assertTrue(registry.startIfIdle("doc", active))
        assertTrue(registry.cancel("doc"))
        assertEquals(1, active.cancelCount)
        assertTrue(registry.startIfIdle("doc", next))
        assertFalse(next.wasCanceled())
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
        assertTrue(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
        assertFalse(TerminalUtil.isStartingActorRouteFailure("[agent-doc] proof-timeout: accepted but unproven"))
    }

    @Test
    fun `latest-run boot timeout is reported immediately while active-turn busy is not retried`() {
        val activeTurn = """
            Error: dispatch-only codex reopen refused to inject into pane %42 for tasks/professional/equityfundingsource.md because the latest run is still booting and never reached a dispatch-ready prompt (active codex turn); wait for the pane to become ready and reroute again
        """.trimIndent()
        val timedOut = """
            Error: dispatch-only codex reopen refused to inject into pane %42 for tasks/professional/equityfundingsource.md because the latest run is still booting and never reached a dispatch-ready prompt (timed_out); wait for the pane to become ready and reroute again
        """.trimIndent()
        val shellSearch = """
            Error: dispatch-only codex reopen refused to inject into pane %42 for tasks/professional/equityfundingsource.md because the latest run is still booting and never reached a dispatch-ready prompt (interactive shell reverse-i-search); wait for the pane to become ready and reroute again
        """.trimIndent()

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(activeTurn))
        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.PERSISTENT, TerminalUtil.classifyRunAgentDocRouteFailure(timedOut))
        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.PERSISTENT, TerminalUtil.classifyRunAgentDocRouteFailure(shellSearch))
        // These latest-run failures already spent the route ready wait. Active turns
        // get a still-running notice; prompt-less timed_out panes get the persisted
        // route diagnostic instead of another silent retry window.
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(activeTurn))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(timedOut))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(shellSearch))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure("[agent-doc] proof-timeout: accepted but unproven"))
    }

    @Test
    fun `active codex turn route refusal is reported as still running not persistent failure`() {
        val relativePath = "tasks/professional/equityfundingsource.md"
        val output = """
            Error: dispatch-only codex reopen refused to inject into pane %42 for $relativePath because the latest run is still booting and never reached a dispatch-ready prompt (active codex turn); wait for the pane to become ready and reroute again
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocStillRunningMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertTrue(message.contains("still running"))
        assertTrue(message.contains(relativePath))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `active-turn skip-wait route refusal is reported as still running immediately`() {
        val relativePath = "tasks/monsterrodholders.md"
        val output = """
            Error: authoritative actor generation 242 for $relativePath owns pane %30 but dispatch-only route will not inject a new trigger because the pane is busy on an active codex turn (Working (1m 34s - esc to interrupt)); restore an idle prompt and retry
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocStillRunningMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
        assertTrue(message.contains("still running"))
        assertTrue(message.contains(relativePath))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `opencode active turn direct blocker is reported as still running not persistent failure`() {
        val relativePath = "tasks/software/tsift.md"
        val output = """
            Error: dispatch-only opencode reopen refused to inject into pane %21 for $relativePath because the pane still shows opencode active turn; restore an idle prompt and retry
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocStillRunningMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertTrue(message.contains("still running"))
        assertTrue(message.contains(relativePath))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `dispatch-only busy actor wait timeout notifies still-running immediately, not retried`() {
        // The exact live-repro shape (#jb-run-agent-doc-command-route-miss): a plain
        // Run Agent Doc on a busy Codex pane. The route now fails closed with this
        // message instead of silently succeeding, and the plugin notifies the
        // operator immediately rather than silently re-waiting the ready timeout.
        val relativePath = "tasks/monsterrodholders.md"
        val output = """
            Error: authoritative actor generation 244 for $relativePath owns pane %40 but dispatch-only route will not inject a new trigger because the authoritative actor is busy did not return to a dispatch-ready prompt in the current generation after waiting 60s. Run `agent-doc session status $relativePath` and wait for an idle prompt.
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocStillRunningMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
        assertTrue(message.contains("still running"))
        assertTrue(message.contains(relativePath))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `queued route success is reported as queued not silent success`() {
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = """
            [route] authoritative actor generation 199 for $relativePath is busy on pane %6; queued pending dispatch "JB `Run Agent Doc` does not complete the agent-doc <file-name> run." in active agent:queue (appended=true, already_present=false) instead of injecting a duplicate trigger
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocQueuedMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.QUEUED_PENDING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
        assertTrue(message.contains("queued"))
        assertTrue(message.contains(relativePath))
        assertTrue(message.contains("when that turn drains"))
    }

    @Test
    fun `paused queue route is reported as actionable info not persistent route failure`() {
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = """
            Error: project controller command `dispatch` failed: dispatch blocked for $relativePath: failed_stage=queue_paused reason=operator paused this queue for manual review receipt_id=52 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control
        """.trimIndent()

        val paused = TerminalUtil.parseRunAgentDocQueuePaused(output)
        val message = TerminalUtil.buildRunAgentDocQueuePausedMessage(relativePath, paused!!)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.QUEUE_PAUSED, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
        assertFalse(paused.restartSupervisorRedirect)
        assertEquals("operator paused this queue for manual review", paused.reason)
        assertTrue(message.contains("queue is paused"))
        assertTrue(message.contains("Resume the queue"))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `stale-supervisor paused queue route offers restart supervisor then resume`() {
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = """
            Error: project controller command `dispatch` failed: dispatch blocked for $relativePath: failed_stage=queue_paused reason=churn-stop: stale supervisor pid1368698; needs operator recycle receipt_id=42 supervisor_restart_redirect stale_pid=1368698 ui_outcome_contract=ui-outcome-v1 ui_outcome=recovered_and_retried ui_outcome_class=ok next_action=restart_supervisor_once_and_retry
        """.trimIndent()

        val paused = TerminalUtil.parseRunAgentDocQueuePaused(output)
        val message = TerminalUtil.buildRunAgentDocQueuePausedMessage(relativePath, paused!!)
        val opsLine = TerminalUtil.buildQueuePausedRouteNotificationOpsLogLine(
            timestamp = "2026-06-24T12:34:56Z",
            relativePath = relativePath,
            paused = paused,
        )

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.QUEUE_PAUSED, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertTrue(paused.restartSupervisorRedirect)
        assertEquals("1368698", paused.stalePid)
        assertTrue(message.contains("Restart Supervisor and resume"))
        assertTrue(message.contains("Stale supervisor pid: 1368698"))
        assertTrue(opsLine.contains("jb_queue_paused_route_notification"))
        assertTrue(opsLine.contains("ui_outcome=recovered_and_retried"))
        assertTrue(opsLine.contains("action=restart_supervisor_and_resume"))
    }

    @Test
    fun `paused queue resume action command carries observed generation and json receipt`() {
        assertEquals(
            listOf(
                "agent-doc",
                "admin",
                "queue",
                "resume",
                "tasks/agent-doc/agent-doc-bugs2.md",
                "--project-root",
                "/repo",
                "--observed-generation",
                "792",
                "--reason",
                "#qpauseux: JetBrains Resume Queue action",
                "--json",
            ),
            TerminalUtil.buildAdminQueueResumeCommand(
                "agent-doc",
                "/repo",
                "tasks/agent-doc/agent-doc-bugs2.md",
                792,
                "#qpauseux: JetBrains Resume Queue action",
            ),
        )
    }

    @Test
    fun `paused queue resume action parses generation and receipt status`() {
        val status = """
            document: /repo/tasks/root.md
            actor: generation=792 pane=%1 window=@1 state=ready
            live_pane: state=alive-idle pane=%1 source=authoritative_actor current_command=agent-doc prompt_ready=true tail=>
        """.trimIndent()

        assertEquals(792L, TerminalUtil.sessionStatusActorGeneration(status))
        assertNull(TerminalUtil.sessionStatusActorGeneration("document: /repo/tasks/root.md"))
        assertTrue(TerminalUtil.adminQueueControlAccepted("""{"operation_kind":"queue_resumed","status":"accepted"}"""))
        assertFalse(TerminalUtil.adminQueueControlAccepted("""{"operation_kind":"queue_resumed","status":"rejected"}"""))
    }

    @Test
    fun `paused queue notification source exposes action labels and ops proof markers`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()

        assertTrue(source.contains("NotificationType.INFORMATION"))
        assertTrue(source.contains("Restart Supervisor and resume"))
        assertTrue(source.contains("Resume queue"))
        assertTrue(source.contains("recordQueuePausedRouteNotificationShown(project, file, paused)"))
        assertTrue(source.contains("jb_queue_paused_route_action"))
    }

    @Test
    fun `accepted-only dispatch proof gap reports typed blocked outcome with attempt and snapshot`() {
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = """
            [route] preserved dispatch-start proof snapshot for $relativePath pane %12 harness=codex phase=direct_pane_dispatch_start_unproven capture_len=1234 capture_hash=e5b665883cc3 snapshot_path=/repo/.agent-doc/logs/route-submit/1781670099279-direct_pane_dispatch_start_unproven-codex-_12-e5b665883cc3.txt editor_attempt_id=1781670087547-4
            Error: dispatch-only codex reopen for $relativePath was accepted in pane %12 via direct_pane_submit (tmux_text_enter), but only pane-input acceptance proof was available after waiting 10s; treating this as not dispatched because no dispatch-start proof was recorded. Restore an idle codex prompt or restart the session and reroute again
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocDispatchUnprovenMessage(relativePath, output)

        assertEquals(
            TerminalUtil.RunAgentDocRouteFailureKind.DISPATCH_START_UNPROVEN,
            TerminalUtil.classifyRunAgentDocRouteFailure(output),
        )
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
        assertTrue(message.contains("did not start"))
        assertTrue(message.contains("dispatch-start proof"))
        assertTrue(message.contains("Attempt: 1781670087547-4"))
        assertTrue(message.contains("Route snapshot: /repo/.agent-doc/logs/route-submit/1781670099279-direct_pane_dispatch_start_unproven-codex-_12-e5b665883cc3.txt"))
        assertFalse(message.contains("route failed"))
    }

    @Test
    fun `typed queued route outcome is reported as queued without prose matching`() {
        val output = """
            [route] dispatch deferred ui_outcome_contract=ui-outcome-v1 ui_outcome=queued_behind_owner ui_outcome_class=ok next_action=wait_for_owner_turn_to_drain
        """.trimIndent()

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.QUEUED_PENDING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
    }

    @Test
    fun `queued dispatch mixed with benign cross-project resync info is still queued not a failure`() {
        // #jb-busy-queue-ux-feedback: a queued-behind-busy route can co-emit
        // benign cross-project resync info. The benign "registered in its own
        // project root — skipping kill" preservation line (now routed to ops_log
        // by #cross-project-stash-pane-condition, but defended here against any
        // future stderr leak) must NOT tip classification to a PERSISTENT error:
        // the queued-pending-dispatch line keeps it QUEUED_PENDING (informational,
        // distinct from real route failures).
        val relativePath = "tasks/software/tsift.md"
        val output = """
            resync: stash pane %32 is registered in its own project root — skipping kill
            [route] authoritative actor generation 233 for $relativePath is busy on pane %19; queued pending dispatch "What are #next-steps?" in active agent:queue (appended=true, already_present=false) instead of injecting a duplicate trigger
        """.trimIndent()

        assertEquals(
            TerminalUtil.RunAgentDocRouteFailureKind.QUEUED_PENDING,
            TerminalUtil.classifyRunAgentDocRouteFailure(output),
        )
        assertFalse(TerminalUtil.isRetryableRunAgentDocRouteFailure(output))
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
    fun `starting restart refusal parses force guidance details`() {
        val output = """
            agent-doc command failed (exit 1): Error: session_restart refused for /repo/tasks/root.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /repo/tasks/root.md` to inspect the pane. Pass `--force` to interrupt the running turn and restart anyway.
        """.trimIndent()

        val refusal = TerminalUtil.parseStartingSessionRestartRefusal(output)

        assertNotNull(refusal)
        assertEquals("/repo/tasks/root.md", refusal!!.file)
        assertEquals("the document changed after the last committed cycle", refusal.reason)
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
    fun `starting restart refusal message exposes interrupt restart action`() {
        val message = TerminalUtil.buildStartingSessionRestartBlockedMessage(
            relativePath = "tasks/root.md",
            refusal = TerminalUtil.StartingSessionRestartRefusal(
                file = "/repo/tasks/root.md",
                reason = "the document changed after the last committed cycle",
            ),
        )

        assertTrue(message.contains("Restart Supervisor is blocked"))
        assertTrue(message.contains("authoritative actor is still starting"))
        assertTrue(message.contains("document changed after the last committed cycle"))
        assertTrue(message.contains("Interrupt and restart"))
        assertFalse(message.contains("agent-doc command failed"))
    }

    @Test
    fun `editor-holds-pane restart refusal parses editor and fields`() {
        val output = """
            agent-doc command failed (exit 1): Error: session_restart refused for /repo/tasks/root.md because pane %2 is held by editor nvim (source=authoritative_actor, current_command=nvim, tail="-- INSERT --"). Run `agent-doc session status /repo/tasks/root.md` to inspect the pane. A terminal editor (for example Claude Code `ctrl+g` edit-in-nvim) owns the pane, so the restart interrupt is swallowed and the restart command would type into the editor buffer. Close the editor (for example `:wq` in nvim) and retry.
        """.trimIndent()

        val refusal = TerminalUtil.parseEditorHoldsPaneRestartRefusal(output)

        assertNotNull(refusal)
        assertEquals("/repo/tasks/root.md", refusal!!.file)
        assertEquals("%2", refusal.pane)
        assertEquals("nvim", refusal.editor)
        assertEquals("authoritative_actor", refusal.source)
        assertEquals("nvim", refusal.currentCommand)
        assertEquals("-- INSERT --", refusal.tail)
    }

    @Test
    fun `editor-holds-pane restart message tells operator to close the editor and omits interrupt action`() {
        val message = TerminalUtil.buildEditorHoldsPaneRestartBlockedMessage(
            relativePath = "tasks/root.md",
            refusal = TerminalUtil.EditorHoldsPaneRestartRefusal(
                file = "/repo/tasks/root.md",
                pane = "%2",
                editor = "nvim",
                source = "authoritative_actor",
                currentCommand = "nvim",
                tail = "-- INSERT --",
            ),
        )

        assertTrue(message.contains("Restart Supervisor is blocked"))
        assertTrue(message.contains("terminal editor (nvim) holds pane %2"))
        assertTrue(message.contains("Close the editor"))
        assertTrue(message.contains(":wq"))
        // The message tells the operator --force won't help (the notify action list
        // also omits an "Interrupt and restart" button).
        assertTrue(message.contains("will not bypass it"))
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
