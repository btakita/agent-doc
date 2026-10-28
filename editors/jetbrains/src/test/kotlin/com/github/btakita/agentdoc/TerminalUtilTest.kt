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

    /** GH #326 phase 3b: Run Agent Doc's `--col` fallback for [root], answered by the frozen floor. */
    private fun routeSnapshot(
        root: String,
        visible: List<String>,
        layout: EditorLayout?,
        focused: String?,
    ): TerminalUtil.RouteLayoutRequest =
        TerminalUtil.RouteLayoutRequest.fromSyncLayout(
            SyncLayoutSnapshot.frozen(
                SyncLayoutSnapshot.evidence(root, root, focused, visible, layout, foreground = false, chosenRoot = root),
            ) { null },
            focused,
        )

    @Test
    fun `project path resolution is local and finds the nearest agent-doc root`() {
        val workspace = Files.createTempDirectory("agent-doc-jb-root")
        val nestedRoot = workspace.resolve("src/sample")
        Files.createDirectories(nestedRoot.resolve(".agent-doc"))
        val document = nestedRoot.resolve("tasks/session.md")
        Files.createDirectories(document.parent)
        Files.writeString(document, "# session\n")

        assertEquals(
            nestedRoot.toAbsolutePath().normalize().toString() to "tasks/session.md",
            TerminalUtil.resolveProjectPath(workspace.toString(), document.toString()),
        )

        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()
        val resolver = source.substringAfter("fun resolveProject(project:")
            .substringBefore("private fun rememberRunAfterClear")
        assertFalse(
            "path resolution on focus/action paths must not cross the native worker pool",
            resolver.contains("NativePatching.resolveProjectPath"),
        )
    }

    @Test
    fun `route success into an off-screen tmux pane surfaces the not-visible notice`() {
        val notice = "[route] pane-not-visible: tasks/a.md runs in pane %2 of tmux session '0', " +
            "which has no attached client (attached client(s) show session(s) '1'); its harness " +
            "output is not on any screen. Attach with: tmux attach-session -t 0"
        assertEquals(
            notice,
            TerminalUtil.routePaneNotVisibleNotice(
                "[route] dispatched via controller editor_route for tasks/a.md\n$notice",
            ),
        )
        val inactiveWindowNotice =
            "[route] pane-not-visible: tasks/a.md runs in pane %192 of inactive tmux window " +
                "'stash' (@1) in attached session '0'; its harness output is off-screen. " +
                "Return editor focus to this document or show it with: " +
                "tmux select-window -t @1 \\; select-pane -t '%192'"
        assertEquals(
            inactiveWindowNotice,
            TerminalUtil.routePaneNotVisibleNotice(
                "[route] delivered steering to the live owner\n$inactiveWindowNotice",
            ),
        )
        assertNull(
            TerminalUtil.routePaneNotVisibleNotice(
                "[route] dispatched via controller editor_route for tasks/a.md",
            ),
        )
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()
        val success = source.substringAfter("LOG.warn(\"[route] SUCCESS: \$output\")")
            .substringBefore("finalStage = \"route_success\"")
        assertTrue(
            "a successful route must warn when its pane is on no screen (GH #243)",
            // `#jbshrinkrestarts`: the notice is read by the native `route_outcome` derivation,
            // whose frozen floor is routePaneNotVisibleNotice.
            success.contains("outcome.paneNotVisibleNotice") && success.contains("notifyWarning("),
        )
    }

    @Test
    fun `native generation fails fast instead of waiting on the IDEA event thread`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"
        ).toFile().readText()
        val callOnWorker = source.substringAfter("private fun <T> callOnWorker")
            .substringBefore("companion object")

        assertTrue(callOnWorker.contains("SwingUtilities.isEventDispatchThread()"))
        assertTrue(callOnWorker.contains("native calls are forbidden on the IDEA event-dispatch thread"))
        assertTrue(
            "the EDT guard must run before submitting or waiting for native work",
            callOnWorker.indexOf("SwingUtilities.isEventDispatchThread()") <
                callOnWorker.indexOf("callOnNativeWorker("),
        )
    }

    @Test
    fun `run route request command uses CP editor route`() {
        assertEquals(
            listOf(
                "cp:editor_route",
                "--dispatch-only",
                "--plain-trigger",
                "--wait-for-ready",
                "120",
                "tasks/root.md",
            ),
            TerminalUtil.buildEditorRouteRequestCommand("tasks/root.md"),
        )
    }

    @Test
    fun `run route records durable attempt stages`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()
        val runRoute = source.substringAfter("internal fun sendToTerminal(")
            .substringBefore("internal fun buildEditorRouteRequestCommand(")

        assertTrue(source.contains("attempt?.recordIfCurrent(\"route_prepare\")"))
        assertTrue(source.contains("attempt?.recordIfCurrent(\"route_command_built\", command = cmd)"))
        assertTrue(source.contains("attempt?.recordIfCurrent(\"route_start\", command = cmd)"))
        assertTrue(source.contains("route_deduped_active_run"))
        assertFalse(source.contains("inFlightRouteRegistry.replace(routeKey, handle)"))
        assertTrue(source.contains("CpRouteClient.runEditorRoute("))
        assertTrue(source.contains("attemptId = attempt?.id"))
        assertTrue(source.contains("routeKey = attempt?.routeKey"))
        assertFalse(source.contains("ProcessBuilder(cmd)"))
        assertFalse(source.contains("\"route_retryable_starting\""))
        assertTrue(source.contains("attempt?.finishIfCurrent(stage, command = cmd, error = finalError)"))
        assertFalse(
            "Run Agent Doc must not create, attach, select, or reveal an IDE terminal",
            runRoute.contains("IdeTerminalCoordinator"),
        )
    }

    @Test
    fun `CP editor route request carries route metadata`() {
        val request = CpRouteClient.editorRouteRequest(
            filePath = "/repo/tasks/root.md",
            relativePath = "tasks/root.md",
            layoutArgs = listOf("--col", "/repo/tasks/root.md", "--focus", "/repo/tasks/root.md"),
            waitForReadySeconds = 15,
            attemptId = "attempt-1",
            routeKey = "route-key-1",
        )

        assertEquals("editor_route", request.get("command").asString)
        assertEquals("/repo/tasks/root.md", request.get("file").asString)
        val payload = com.google.gson.JsonParser.parseString(
            request.get("diagnostic_payload").asString,
        ).asJsonObject
        assertEquals("jetbrains_plugin", payload.get("source").asString)
        assertEquals("tasks/root.md", payload.get("relative_path").asString)
        assertEquals(true, payload.get("dispatch_only").asBoolean)
        assertEquals(true, payload.get("plain_trigger").asBoolean)
        assertEquals(15L, payload.get("wait_for_ready_secs").asLong)
        assertEquals("attempt-1", payload.get("attempt_id").asString)
        assertEquals("route-key-1", payload.get("route_key").asString)
        assertEquals("--col", payload.getAsJsonArray("layout_args")[0].asString)
        assertEquals("--focus", payload.getAsJsonArray("layout_args")[2].asString)
        assertFalse(payload.has("column_order"))
        assertEquals(
            "the route names this IDE's navigation lane (#ideriderindependentnav P6)",
            CpRouteClient.navigationClientId,
            payload.get("client_id").asString,
        )
    }

    @Test
    fun `CP editor route request omits the client id only when none is given`() {
        val request = CpRouteClient.editorRouteRequest(
            filePath = "/repo/tasks/root.md",
            relativePath = "tasks/root.md",
            layoutArgs = listOf("--col", "/repo/tasks/root.md"),
            waitForReadySeconds = 15,
            attemptId = null,
            routeKey = null,
            clientId = null,
        )
        val payload = com.google.gson.JsonParser.parseString(
            request.get("diagnostic_payload").asString,
        ).asJsonObject
        assertFalse(payload.has("client_id"))
    }

    @Test
    fun `CP editor route request names its column order source`() {
        val request = CpRouteClient.editorRouteRequest(
            filePath = "/repo/tasks/root.md",
            relativePath = "tasks/root.md",
            layoutArgs = listOf("--col", "/repo/tasks/root.md", "--focus", "/repo/tasks/root.md"),
            waitForReadySeconds = 15,
            attemptId = null,
            routeKey = null,
            columnOrder = "unknown",
        )
        val payload = com.google.gson.JsonParser.parseString(
            request.get("diagnostic_payload").asString,
        ).asJsonObject
        assertEquals("unknown", payload.get("column_order").asString)
    }

    @Test
    fun `CP editor route request carries the GH 111 layout mode only when given`() {
        val payloadOf = { layoutMode: String? ->
            val request = CpRouteClient.editorRouteRequest(
                filePath = "/repo/tasks/root.md",
                relativePath = "tasks/root.md",
                layoutArgs = listOf("--col", "/repo/tasks/root.md", "--focus", "/repo/tasks/root.md"),
                waitForReadySeconds = 15,
                attemptId = null,
                routeKey = null,
                layoutMode = layoutMode,
            )
            com.google.gson.JsonParser.parseString(
                request.get("diagnostic_payload").asString,
            ).asJsonObject
        }

        assertEquals("ensure", payloadOf("ensure").get("layout_mode").asString)
        assertEquals("exact", payloadOf("exact").get("layout_mode").asString)
        assertFalse(payloadOf(null).has("layout_mode"))
    }

    @Test
    fun `route layout mode is exact only for a detected multi column layout`() {
        assertEquals(TerminalUtil.ROUTE_LAYOUT_MODE_ENSURE, TerminalUtil.routeLayoutMode(null))
        assertEquals(
            TerminalUtil.ROUTE_LAYOUT_MODE_ENSURE,
            TerminalUtil.routeLayoutMode(EditorLayout(listOf(LayoutColumn(listOf("/repo/a.md"))))),
        )
        assertEquals(
            TerminalUtil.ROUTE_LAYOUT_MODE_EXACT,
            TerminalUtil.routeLayoutMode(
                EditorLayout(
                    listOf(
                        LayoutColumn(listOf("/repo/a.md")),
                        LayoutColumn(listOf("/repo/b.md")),
                    )
                )
            ),
        )
    }

    @Test
    fun `run agent doc passes the route layout mode to the controller`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()

        // GH #326 phase 3b: the mode is the `sync_layout` derivation's `layout_mode`.
        assertTrue(source.contains("layoutMode = snapshot.layoutMode"))
        assertTrue(source.contains("layoutMode = routeLayout.layoutMode"))
    }

    @Test
    fun `restart agent action records ops log menu invocation marker before restart`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/RestartAgentAction.kt"
        ).toFile().readText()

        assertTrue(source.contains("TerminalUtil.recordRestartAgentMenuInvoked(project, file)"))
        assertTrue(source.indexOf("recordRestartAgentMenuInvoked") < source.indexOf("restartAgentSession(project, file)"))
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
        val args = routeSnapshot(
            root = "/repo/src/sample-app",
            visible = listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/src/sample-app/tasks/sampleorders.md",
            ),
            layout = EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md")),
                    LayoutColumn(listOf("/repo/src/sample-app/tasks/sampleorders.md")),
                )
            ),
            focused = "/repo/src/sample-app/tasks/sampleorders.md",
        ).layoutArgs

        assertEquals(
            listOf(
                "--col",
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "--col",
                "/repo/src/sample-app/tasks/sampleorders.md",
                "--focus",
                "/repo/src/sample-app/tasks/sampleorders.md",
            ),
            args,
        )
    }

    @Test
    fun `route layout args preserve empty split columns for mixed layouts`() {
        val args = routeSnapshot(
            root = "/repo/src/sample-app",
            visible = listOf("/repo/src/sample-app/tasks/sampleorders.md"),
            layout = EditorLayout(
                listOf(
                    LayoutColumn(emptyList()),
                    LayoutColumn(listOf("/repo/src/sample-app/tasks/sampleorders.md")),
                )
            ),
            focused = "/repo/src/sample-app/tasks/sampleorders.md",
        ).layoutArgs

        assertEquals(
            listOf(
                "--col",
                "",
                "--col",
                "/repo/src/sample-app/tasks/sampleorders.md",
                "--focus",
                "/repo/src/sample-app/tasks/sampleorders.md",
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
    fun `persistent route failure leads with visible yaml tab cause`() {
        val cwd = Files.createTempDirectory("agent-doc-jb-route-error-message").toFile()
        val relativePath = "tasks/sdk/sdk-dev.md"
        val output = """
            Error: invalid YAML frontmatter in /repo/$relativePath: invalid YAML frontmatter in /repo/$relativePath: found character that cannot start any token at line 6 column 1, while scanning for the next token

            Frontmatter excerpt:
              5 | presets:
            > 6 | ${'\t'}'#pr-aggregate': '#repo: Aggregate PR'
                | ^
              7 | ${'\t'}'#repo': 'haiven-dev/haiven-sdk'
        """.trimIndent()
        val saved = TerminalUtil.routeFailureDiagnosticsFile(cwd.path, relativePath)

        val message = TerminalUtil.buildPersistentRouteFailureMessage(
            cwd = cwd.path,
            relativePath = relativePath,
            exitCode = 1,
            elapsed = "3.5s",
            routeOutput = output,
            savedFile = saved,
        )

        assertTrue(message.startsWith("Run Agent Doc failed: Invalid YAML frontmatter (line 6): tab indentation is not allowed; use spaces."))
        assertTrue(message.contains("$relativePath · 3.5s · exit 1"))
        assertTrue(message.contains("Full route output: .agent-doc/state/editor-route-errors/tasks__sdk__sdk-dev.md.txt"))
        assertFalse(message.contains("found character that cannot start any token"))
        assertTrue(message.length < 260)
    }

    @Test
    fun `persistent route failure bounds generic cause before route metadata`() {
        val detail = "controller rejected route because " + "x".repeat(300)
        val message = TerminalUtil.buildPersistentRouteFailureMessage(
            cwd = "/repo",
            relativePath = "tasks/a.md",
            exitCode = 1,
            elapsed = "2.0s",
            routeOutput = "[route] starting\nError: $detail\ntrailing diagnostics",
            savedFile = null,
        )
        val firstLine = message.lineSequence().first()

        assertTrue(firstLine.startsWith("Run Agent Doc failed: controller rejected route because "))
        assertTrue(firstLine.endsWith("…"))
        assertTrue(firstLine.length <= 202)
        assertTrue(message.endsWith("tasks/a.md · 2.0s · exit 1"))
        assertFalse(message.contains("trailing diagnostics"))
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
    fun `starting actor route failures are typed as startup not ready`() {
        val output = """
            [route] target tmux session: 1
            Error: authoritative actor generation 22 for tasks/root.md owns pane %12 but route will not inject a new trigger because the authoritative actor is still starting.
        """.trimIndent()

        assertTrue(TerminalUtil.isStartingActorRouteFailure(output))
        assertEquals(
            TerminalUtil.RunAgentDocRouteFailureKind.STARTUP_NOT_READY,
            TerminalUtil.classifyRunAgentDocRouteFailure(output),
        )
        assertFalse(TerminalUtil.isStartingActorRouteFailure("[agent-doc] proof-timeout: accepted but unproven"))
    }

    @Test
    fun `latest-run timeout is a typed startup blocker without client resubmission`() {
        val activeTurn = """
            Error: dispatch-only codex reopen refused to inject into pane %42 for tasks/professional/sampleportal.md because the latest run is still booting and never reached a dispatch-ready prompt (active codex turn); wait for the pane to become ready and reroute again
        """.trimIndent()
        val timedOut = """
            Error: dispatch-only codex reopen refused to inject into pane %42 for tasks/professional/sampleportal.md because the latest run is still booting and never reached a dispatch-ready prompt (timed_out); wait for the pane to become ready and reroute again
        """.trimIndent()
        val shellSearch = """
            Error: dispatch-only codex reopen refused to inject into pane %42 for tasks/professional/sampleportal.md because the latest run is still booting and never reached a dispatch-ready prompt (interactive shell reverse-i-search); wait for the pane to become ready and reroute again
        """.trimIndent()

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(activeTurn))
        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.STARTUP_NOT_READY, TerminalUtil.classifyRunAgentDocRouteFailure(timedOut))
        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.PERSISTENT, TerminalUtil.classifyRunAgentDocRouteFailure(shellSearch))
    }

    @Test
    fun `active codex turn route refusal is reported as still running not persistent failure`() {
        val relativePath = "tasks/professional/sampleportal.md"
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
        val relativePath = "tasks/sampleorders.md"
        val output = """
            Error: authoritative actor generation 242 for $relativePath owns pane %30 but dispatch-only route will not inject a new trigger because the pane is busy on an active codex turn (Working (1m 34s - esc to interrupt)); restore an idle prompt and retry
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocStillRunningMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
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
        val relativePath = "tasks/sampleorders.md"
        val output = """
            Error: authoritative actor generation 244 for $relativePath owns pane %40 but dispatch-only route will not inject a new trigger because the authoritative actor is busy did not return to a dispatch-ready prompt in the current generation after waiting 60s. Run `agent-doc session status $relativePath` and wait for an idle prompt.
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocStillRunningMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.BUSY_RUNNING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertTrue(message.contains("still running"))
        assertTrue(message.contains(relativePath))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `route into another lane's pane surfaces the owning lane notice`() {
        val output = """
            [route] Pane %7 is alive, sending command
            [agent-doc] owned_by_other_lane: plan.md runs in pane %7 of navigation lane IU-0a1b2c3d (tmux session proj-IU-0a1b2c3d); the route dispatched it there and left this IDE's tmux layout unchanged. Use Claim for Tmux Pane to move the pane into this IDE's lane.
        """.trimIndent()
        val notice = TerminalUtil.routeOwnedByOtherLaneNotice(output)
        assertTrue(notice!!.startsWith("plan.md runs in pane %7 of navigation lane IU-0a1b2c3d"))
        assertTrue(notice.contains("Claim for Tmux Pane"))
        assertNull(TerminalUtil.routeOwnedByOtherLaneNotice("[route] Pane %7 is alive, sending command"))
        assertNull(TerminalUtil.routeOwnedByOtherLaneNotice(TerminalUtil.ROUTE_OWNED_BY_OTHER_LANE_NOTICE_PREFIX))
    }

    @Test
    fun `queued route success is reported as queued not silent success`() {
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = """
            [route] authoritative actor generation 199 for $relativePath is busy on pane %6; queued pending dispatch "JB `Run Agent Doc` does not complete the agent-doc <file-name> run." in active agent:queue (appended=true, already_present=false) instead of injecting a duplicate trigger
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocQueuedMessage(relativePath)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.QUEUED_PENDING, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertTrue(message.contains("queued"))
        assertTrue(message.contains(relativePath))
        assertTrue(message.contains("when that turn drains"))
        assertFalse(TerminalUtil.routeStageBlocksStateProjection("route_queued_pending"))
        assertTrue(TerminalUtil.routeStageBlocksStateProjection("route_failed"))
    }

    @Test
    fun `protected prompt input route refusal is actionable not persistent failure`() {
        val relativePath = "tasks/professional/sampleportal.md"
        val snapshot = "/tmp/agent-doc-route-snapshot.txt"
        val output = """
            Error: route refusing to dispatch agent-doc /home/brian/work/btakita/agent-loop/$relativePath into pane %3 for /home/brian/work/btakita/agent-loop/$relativePath because the composer contains protected prompt input (drafted prompt input) draft_preview="› implement the feature"; clear or submit that draft, then rerun agent-doc route snapshot_path=$snapshot
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocProtectedPromptInputMessage(relativePath, output)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.PROTECTED_PROMPT_INPUT, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertTrue(message.contains("did not start"))
        assertTrue(message.contains("unsent prompt text"))
        assertTrue(message.contains("Draft preview: › implement the feature"))
        assertTrue(message.contains(snapshot))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
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
    fun `actor-switch defer on paused queue offers restart supervisor and resume`() {
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = """
            [route] target tmux session: 0
            Error: authoritative actor record for $relativePath is running harness claude-code, but frontmatter now resolves to codex; deferring to boundary agent restart instead of replacing live pane. queue is paused — the boundary restart will not fire until it is healthy and resumed. Run: agent-doc session restart-supervisor $relativePath
        """.trimIndent()

        val deferred = TerminalUtil.parseRunAgentDocAgentSwitchDeferred(output)
        val message = TerminalUtil.buildRunAgentDocAgentSwitchDeferredMessage(relativePath, deferred!!)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.AGENT_SWITCH_DEFERRED, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertEquals("claude-code", deferred.previousHarness)
        assertEquals("codex", deferred.targetHarness)
        assertTrue(deferred.queuePaused)
        assertFalse(deferred.forceRequired)
        assertTrue(message.contains("Restart Supervisor and resume"))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `actor-switch defer on not-ready pane offers interrupt restart`() {
        val relativePath = "tasks/professional/sampleportal.md"
        val output = """
            Error: authoritative actor record for $relativePath is running harness codex, but frontmatter now resolves to opencode; deferring to boundary agent restart instead of replacing live pane. pane is busy (not dispatch-ready) — run: agent-doc session restart-supervisor $relativePath --force
        """.trimIndent()

        val deferred = TerminalUtil.parseRunAgentDocAgentSwitchDeferred(output)
        val message = TerminalUtil.buildRunAgentDocAgentSwitchDeferredMessage(relativePath, deferred!!)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.AGENT_SWITCH_DEFERRED, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertEquals("codex", deferred.previousHarness)
        assertEquals("opencode", deferred.targetHarness)
        assertFalse(deferred.queuePaused)
        assertTrue(deferred.forceRequired)
        assertTrue(message.contains("Interrupt and restart"))
        assertFalse(message.contains("route failed"))
        assertFalse(message.contains("Saved exact route output"))
    }

    @Test
    fun `actor-switch defer during its own restart never tells operator to interrupt`() {
        // `#actorswitchdeferbusyself`: the live repro — operator ran JB `Restart Agent`
        // (codex -> claude), the supervisor spawned claude, and Run Agent Doc landed
        // inside that restart's busy window. The old message told them to interrupt
        // and restart, i.e. to abort the restart that was completing their switch.
        val relativePath = "tasks/agent-doc/agent-doc-bugs2.md"
        val output = """
            Error: authoritative actor record for $relativePath is running harness codex, but frontmatter now resolves to claude-code; deferring to boundary agent restart instead of replacing live pane. the harness restart is already in flight — the switch completes at that boundary, so wait for it and run Agent Doc again. No interrupt or forced restart is needed.
        """.trimIndent()

        val deferred = TerminalUtil.parseRunAgentDocAgentSwitchDeferred(output)
        val message = TerminalUtil.buildRunAgentDocAgentSwitchDeferredMessage(relativePath, deferred!!)

        assertEquals(TerminalUtil.RunAgentDocRouteFailureKind.AGENT_SWITCH_DEFERRED, TerminalUtil.classifyRunAgentDocRouteFailure(output))
        assertEquals("codex", deferred.previousHarness)
        assertEquals("claude-code", deferred.targetHarness)
        assertTrue(deferred.restartInFlight)
        assertFalse(deferred.forceRequired)
        assertFalse(deferred.queuePaused)
        assertTrue(message.contains("already in flight"))
        assertFalse(message.contains("Interrupt and restart"))
        assertFalse(message.contains("not at a dispatch-ready boundary"))
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
    }

    @Test
    fun `explicit steering delivery is informational and distinct from queued work`() {
        val relativePath = "tasks/agent-doc/agent-doc-bugs.md"
        val output = """
            Error: [route] Sent 1 update to the agent already running for /repo/$relativePath; no second turn was started. steering_delivered=1 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=steering_delivered_to_owner ui_outcome_class=ok next_action=owner_turn_handles_steering
        """.trimIndent()
        val message = TerminalUtil.buildRunAgentDocSteeringDeliveredMessage(relativePath)

        assertEquals(
            TerminalUtil.RunAgentDocRouteFailureKind.STEERING_DELIVERED,
            TerminalUtil.classifyRunAgentDocRouteFailure(output),
        )
        assertTrue(message.contains("Sent your update"))
        assertTrue(message.contains("no second turn was started"))
        assertFalse(message.contains("Error"))
        assertFalse(message.contains("session-check"))
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
    }

    @Test
    fun `run route submits once and lets the controller await reactive readiness`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()
        val route = source.substringAfter("val startedAt = System.currentTimeMillis()")
            .substringBefore("private fun routeAttemptError")

        assertEquals(1, Regex("CpRouteClient\\.runEditorRoute\\(").findAll(route).count())
        assertFalse(route.contains("while ("))
        assertFalse(route.contains("sleepBeforeRetry"))
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

    // #clearqueuedliveturn: the live-turn refusal (observed on agent-doc-bugs.md
    // pane %149) fell through to a generic "agent-doc command failed" error.
    @Test
    fun `live turn clear refusal parses into a busy refusal not a command failure`() {
        val output = """
            agent-doc command failed (exit 1): Error: session_clear refused for /repo/tasks/agent-doc/agent-doc-bugs.md because a turn is live in pane %149 (harness_turn_live=true evidence=dispatch_turn_running, source=authoritative_actor) and no supervisor owns the pane to deliver a queued clear when it ends; no `/clear` was sent or queued. ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=wait_for_turn_or_interrupt_then_retry unblocker=wait_for_turn_end_no_supervisor_to_queue_clear. Wait for the turn to finish and run Clear Session Context again.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionClearRefusal(output)

        assertNotNull(refusal)
        assertEquals("/repo/tasks/agent-doc/agent-doc-bugs.md", refusal!!.file)
        assertEquals("%149", refusal.pane)
        assertEquals("authoritative_actor", refusal.source)
        assertEquals("dispatch_turn_running", refusal.liveTurnEvidence)
        val message = TerminalUtil.buildBusySessionClearBlockedMessage("tasks/agent-doc/agent-doc-bugs.md", refusal)
        assertTrue(message.contains("A turn is running in pane %149 (dispatch_turn_running)"))
        assertTrue(message.contains("Nothing was sent"))
        assertFalse(message.contains("agent-doc command failed"))
    }

    @Test
    fun `live turn queued clear is a queued notice not a failure`() {
        val output = """
            session_clear queued for /repo/tasks/agent-doc/agent-doc-bugs.md — a turn is live in pane %149 (harness_turn_live=true evidence=turn_active_lease, source=authoritative_actor), so no `/clear` was typed into the running turn. One clear is queued and the supervisor delivers it automatically when the turn ends. ui_outcome=queued ui_outcome_class=queued next_action=none unblocker=none.
        """.trimIndent()

        val queued = TerminalUtil.parseLiveTurnQueuedClear(output)

        assertNotNull(queued)
        assertEquals("/repo/tasks/agent-doc/agent-doc-bugs.md", queued!!.file)
        assertEquals("%149", queued.pane)
        assertFalse(TerminalUtil.isDeferredQueuePreemptClear(output))
        assertNull(TerminalUtil.parseBusySessionClearRefusal(output))
        val message = TerminalUtil.buildLiveTurnQueuedClearMessage("tasks/agent-doc/agent-doc-bugs.md", queued)
        assertTrue(message.startsWith("Clear Session Context queued for tasks/agent-doc/agent-doc-bugs.md: it runs when the turn in pane %149 ends."))
        assertTrue(message.contains("live-turn evidence: turn_active_lease)"))
        assertNull(TerminalUtil.parseLiveTurnQueuedClear("Cleared session context for /repo/plan.md"))
    }

    // #clearnoticeverify: live-verified 2026-10-11 in IntelliJ — the notice
    // dropped the CLI's re-arm and evidence age, so a re-armed press read
    // exactly like the first one, and the 3s balloon collapsed before "when
    // that turn ends" was visible.
    @Test
    fun `live turn queued clear notice carries evidence age and re-arm`() {
        val first = """
            session_clear queued for /repo/tmp/doc.md — a turn is live in pane %185 (harness_turn_live=true evidence=turn_active_lease evidence_age_secs=42, source=authoritative_actor), so no `/clear` was typed into the running turn. One clear is queued and the supervisor delivers it automatically when the turn ends; it is dropped instead if a newer prompt starts first or the session restarts. ui_outcome=queued ui_outcome_class=queued next_action=none unblocker=none rearmed=false. Run `agent-doc session interrupt-clear /repo/tmp/doc.md` to interrupt the turn and clear now.
        """.trimIndent()
        val queued = TerminalUtil.parseLiveTurnQueuedClear(first)!!
        assertEquals("%185", queued.pane)
        assertEquals("turn_active_lease", queued.evidence)
        assertEquals(42L, queued.evidenceAgeSecs)
        assertFalse(queued.rearmed)
        val message = TerminalUtil.buildLiveTurnQueuedClearMessage("tmp/doc.md", queued)
        // The outcome and when it runs fit the collapsed balloon's first line.
        assertEquals(
            "Clear Session Context queued for tmp/doc.md: it runs when the turn in pane %185 ends.",
            message.lineSequence().first(),
        )
        assertTrue(message.contains("(live-turn evidence: turn_active_lease, 42s old)"))
        assertFalse(message.contains("re-armed"))

        val rearmedOutput = first
            .replace("evidence_age_secs=42", "evidence_age_secs=4000")
            .replace("rearmed=false", "rearmed=true")
            .replace("One clear is queued", "The clear that was already queued is re-armed for this turn (still one clear)")
        val rearmed = TerminalUtil.parseLiveTurnQueuedClear(rearmedOutput)!!
        assertTrue(rearmed.rearmed)
        assertEquals(4000L, rearmed.evidenceAgeSecs)
        val rearmedMessage = TerminalUtil.buildLiveTurnQueuedClearMessage("tmp/doc.md", rearmed)
        assertEquals(
            "Clear Session Context re-armed for tmp/doc.md: still one clear, it runs when the turn in pane %185 ends.",
            rearmedMessage.lineSequence().first(),
        )
        assertTrue(rearmedMessage.contains("(live-turn evidence: turn_active_lease, 66m 40s old)"))

        // An unknown age (no lease marker or receipt time) shows the evidence only.
        val unknownAge = TerminalUtil.parseLiveTurnQueuedClear(first.replace("evidence_age_secs=42", "evidence_age_secs=unknown"))!!
        assertNull(unknownAge.evidenceAgeSecs)
        assertTrue(
            TerminalUtil.buildLiveTurnQueuedClearMessage("tmp/doc.md", unknownAge)
                .contains("(live-turn evidence: turn_active_lease)"),
        )
    }

    // #clearctxslow: live 2026-10-15 on haiven-dev tasks/sdk.md — Run Agent Doc's prompt was
    // cancelled before Claude Code recorded it, and the queued clear's notice said "it runs when
    // the turn in pane %180 ends" over an idle pane. The binary now reports the settling prompt
    // and when the clear runs; the notice says that instead of claiming a live turn.
    @Test
    fun `clear queued behind a cancelled prompt says when it runs, not that a turn is live`() {
        val output = """
            session_clear queued for /repo/src/haiven-dev/tasks/sdk.md — the last prompt in pane %180 has not started a turn (harness_turn_live=pending evidence=abandoned_prompt_settling settles_in_secs=16 settle_window_secs=20, source=authoritative_actor): Claude Code wrote no prompt record after its snapshot, which is what a prompt cancelled during its `UserPromptSubmit` hooks leaves. A prompt can still start up to 20s after its snapshot, so no `/clear` was typed yet. One clear is queued and the supervisor delivers it automatically in about 16s; pressing Clear Session Context again does not make it sooner. It is dropped instead if that prompt or a newer one starts first or the session restarts. ui_outcome=queued ui_outcome_class=queued next_action=none unblocker=none rearmed=false. Run `agent-doc session interrupt-clear /repo/src/haiven-dev/tasks/sdk.md` to clear now.
        """.trimIndent()

        val queued = TerminalUtil.parseLiveTurnQueuedClear(output)!!
        assertEquals("/repo/src/haiven-dev/tasks/sdk.md", queued.file)
        assertEquals("%180", queued.pane)
        assertEquals("abandoned_prompt_settling", queued.evidence)
        assertEquals(16L, queued.settlesInSecs)
        assertFalse(queued.rearmed)
        assertNull(TerminalUtil.parseBusySessionClearRefusal(output))
        assertFalse(TerminalUtil.isDeferredQueuePreemptClear(output))
        val message = TerminalUtil.buildLiveTurnQueuedClearMessage("tasks/sdk.md", queued)
        assertEquals(
            "Clear Session Context queued for tasks/sdk.md: it runs automatically in about 16s.",
            message.lineSequence().first(),
        )
        assertTrue(message.contains("The last prompt in pane %180 never started a turn"))
        assertFalse(message.contains("the turn in pane"))
        assertFalse(message.contains("running turn"))

        val again = TerminalUtil.parseLiveTurnQueuedClear(
            output.replace("settles_in_secs=16", "settles_in_secs=10")
                .replace("about 16s", "about 10s")
                .replace("rearmed=false", "rearmed=true"),
        )!!
        assertTrue(again.rearmed)
        assertEquals(
            "Clear Session Context is already queued for tasks/sdk.md: it runs automatically in about 10s.",
            TerminalUtil.buildLiveTurnQueuedClearMessage("tasks/sdk.md", again).lineSequence().first(),
        )

        // A live-turn queue (no settle) keeps the turn-end wording.
        val live = TerminalUtil.parseLiveTurnQueuedClear(
            "session_clear queued for /repo/tmp/doc.md — a turn is live in pane %185 (harness_turn_live=true evidence=turn_active_lease evidence_age_secs=42, source=authoritative_actor). rearmed=false",
        )!!
        assertNull(live.settlesInSecs)
        assertTrue(TerminalUtil.buildLiveTurnQueuedClearMessage("tmp/doc.md", live).contains("when the turn in pane %185 ends"))
    }

    // #clearaftrun: live 2026-10-15 19:40Z on haiven-dev tasks/sdk.md — Run Agent Doc was accepted,
    // its trigger was not typed for ~22s, and the Clear pressed in that window was queued behind it.
    // The notice said "it runs when the turn in pane %180 ends. Nothing was typed into the running
    // turn" over an idle pane and offered Interrupt and clear, which cannot clear sooner there.
    @Test
    fun `clear queued behind an accepted run says no turn is running and it runs after that run`() {
        val output = """
            session_clear queued for /repo/src/haiven-dev/tasks/sdk.md — Run Agent Doc for pane %180 was accepted 12s ago and has not started its turn yet (harness_turn_live=pending evidence=dispatch_turn_pending evidence_age_secs=12 pre_turn_grace_secs=120, source=authoritative_actor); its trigger has not reached the harness or is still being submitted, so no turn is running. A clear pressed after Run Agent Doc runs after that run, so no `/clear` was typed ahead of its trigger. One clear is queued and the supervisor delivers it automatically when that run's turn ends; that run does not drop it, but a prompt started after this press or a session restart does. If that run never starts, the clear runs within about 108s, when its pre-turn grace expires. Pressing Clear Session Context again, or Interrupt and clear, does not make it sooner: there is no turn to interrupt yet. ui_outcome=queued ui_outcome_class=queued next_action=none unblocker=none rearmed=false.
        """.trimIndent()

        val queued = TerminalUtil.parseLiveTurnQueuedClear(output)!!
        assertEquals("/repo/src/haiven-dev/tasks/sdk.md", queued.file)
        assertEquals("%180", queued.pane)
        assertEquals("dispatch_turn_pending", queued.evidence)
        assertEquals(12L, queued.evidenceAgeSecs)
        assertEquals(120L, queued.preTurnGraceSecs)
        assertNull(queued.settlesInSecs)
        assertFalse(queued.rearmed)
        assertNull(TerminalUtil.parseBusySessionClearRefusal(output))
        assertFalse(TerminalUtil.isDeferredQueuePreemptClear(output))
        val message = TerminalUtil.buildLiveTurnQueuedClearMessage("tasks/sdk.md", queued)
        assertEquals(
            "Clear Session Context queued for tasks/sdk.md: it runs after the Run Agent Doc accepted 12s ago finishes.",
            message.lineSequence().first(),
        )
        assertTrue(message.contains("That run has not started its turn in pane %180 yet, so nothing is running"))
        assertTrue(message.contains("If the run never starts, the clear runs within about 108s."))
        assertTrue(message.contains("Interrupt and clear does not make it sooner"))
        assertFalse(message.contains("the turn in pane"))
        assertFalse(message.contains("running turn"))
        assertFalse(message.contains("Use Interrupt and clear to clear immediately"))

        val again = TerminalUtil.parseLiveTurnQueuedClear(
            output.replace("evidence_age_secs=12", "evidence_age_secs=unknown")
                .replace("rearmed=false", "rearmed=true"),
        )!!
        assertTrue(again.rearmed)
        assertNull(again.evidenceAgeSecs)
        val againMessage = TerminalUtil.buildLiveTurnQueuedClearMessage("tasks/sdk.md", again)
        assertEquals(
            "Clear Session Context is already queued for tasks/sdk.md: it runs after the Run Agent Doc already accepted finishes.",
            againMessage.lineSequence().first(),
        )
        assertTrue(againMessage.contains("within about 2m 0s."))
    }

    @Test
    fun `busy clear refusal parses generic command wrapper output`() {
        val output = """
            agent-doc command failed (exit 1): Error: session_clear refused for /home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md because pane %13 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="● Tip Override global tool settings per agent configuration"). Run `agent-doc session status /home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.
        """.trimIndent()

        val refusal = TerminalUtil.parseBusySessionClearRefusal(output)

        assertNotNull(refusal)
        assertEquals("/home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md", refusal!!.file)
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
    fun `compact lint failure is concise and actionable in a small notification`() {
        val output = """
            Error: project controller command `compact_document` failed: [lint-gate] INTERRUPTED: 1 blocking lint finding(s) for /repo/tasks/api.md (mode=warn, source=default). Fix the directives below, then retry the interrupted command. /repo/tasks/api.md:449:1 error: attribute `queu0000e` on `agent:backlog` is missing `=value` [agent-doc/malformed-attr] hint: try `queu0000e= `
        """.trimIndent()

        val message = TerminalUtil.buildCommandFailureMessage("compact this document", 1, output)

        assertEquals(
            """
                Agent Doc couldn't compact this document because a document directive is malformed.

                api.md, line 449
                Attribute `queu0000e` on `agent:backlog` is missing `=value`.

                Suggested fix: `queu0000e= `

                Fix the directive, then try again.
            """.trimIndent(),
            message,
        )
        assertFalse(message.contains("project controller command"))
        assertFalse(message.contains("agent_doc_lint_dialect"))
        assertTrue(message.length < 300)
    }

    @Test
    fun `notification html keeps lint hint angle-bracket placeholders visible`() {
        val output = listOf(
            "Error: project controller command `compact_document` failed: [lint-gate] INTERRUPTED: 1 blocking lint finding(s) for /repo/tasks/doc.md (mode=warn, source=default).",
            "/repo/tasks/doc.md:14:1 error: attribute `preset=` on `agent:queue` has empty value [agent-doc/empty-attr-value]",
            "  hint: provide a value: `preset=<value>`",
        ).joinToString("\n")

        val message = TerminalUtil.buildCommandFailureMessage("compact this document", 1, output)
        assertTrue(message.contains("`preset=<value>`"))

        val html = TerminalUtil.notificationHtml(message)
        assertTrue(html, html.contains("`preset=&lt;value&gt;`"))
        assertFalse(html, html.contains("<value>"))
        assertTrue(html, html.contains("doc.md, line 14<br>"))
        assertEquals("a &amp; b<br>&quot;c&quot;", TerminalUtil.notificationHtml("a & b\r\n\"c\""))
    }

    @Test
    fun `compact lint failure extracts CRLF diagnostic before dogfood trailer`() {
        val output = listOf(
            "Error: project controller command `compact_document` failed: [lint-gate] INTERRUPTED: 1 blocking lint finding(s) for /repo/tasks/sample.md (mode=warn, source=default).",
            "/repo/tasks/sample.md:616:1 error: attribute `subagents` on `agent:backlog` is missing `=value` [agent-doc/malformed-attr]",
            "  hint: try `subagents=<value>`",
            "",
            "[dogfood] ACTIONABLE_AGENT_DOC_FIX_PROMPT issue_class=interrupted_closeout",
            "Agent Doc did not complete this turn successfully.",
        ).joinToString("\r\n")

        val message = TerminalUtil.buildCommandFailureMessage("compact this document", 1, output)

        assertTrue(message.contains("sample.md, line 616"))
        assertTrue(message.contains("Attribute `subagents`"))
        assertTrue(message.contains("Suggested fix: `subagents=<value>`"))
        assertFalse(message.contains("dogfood"))
        assertFalse(message.contains("project controller command"))
    }

    @Test
    fun `generic command failure is bounded for a small notification`() {
        val message = TerminalUtil.buildCommandFailureMessage(
            action = "complete the command",
            exitCode = 7,
            output = "x".repeat(900),
        )

        assertTrue(message.startsWith("Agent Doc couldn't complete the command (exit 7)."))
        assertTrue(message.endsWith("..."))
        assertTrue(message.length < 550)
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
    fun `restart agent uses explicit agent session command`() {
        assertEquals(
            listOf(
                "agent-doc",
                "session",
                "restart-agent",
                "tasks/agent-doc/agent-doc-bugs2.md",
            ),
            TerminalUtil.buildSessionCommand(
                "agent-doc",
                listOf("restart-agent"),
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
    fun `interrupt restart preserves the original restart target`() {
        assertEquals(
            listOf("restart-agent"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.AGENT, force = false),
        )
        assertEquals(
            listOf("restart-agent", "--force"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.AGENT, force = true),
        )
        assertEquals(
            listOf("restart-supervisor"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.SUPERVISOR, force = false),
        )
        assertEquals(
            listOf("restart-supervisor", "--force"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.SUPERVISOR, force = true),
        )
    }

    /**
     * `#restartagentharness` EDT fix: run [launchRestartCommandOffEdt] from the real Swing EDT with a [readAgent]
     * that carries `AgentDocLib`'s EDT guard, and return the launched args plus the threads that
     * ran the native read and the launch.
     */
    private fun launchRestartFromEdt(
        target: TerminalUtil.RestartTarget,
        editorText: String?,
        readAgent: (String) -> String?,
        executeOffEdt: (Runnable) -> Unit = { task -> Thread(task, "restart-off-edt-test").start() },
    ): Triple<List<String>?, Boolean?, Boolean?> {
        val launched = java.util.concurrent.CountDownLatch(1)
        val args = java.util.concurrent.atomic.AtomicReference<List<String>?>()
        val readOnEdt = java.util.concurrent.atomic.AtomicReference<Boolean?>()
        val launchOnEdt = java.util.concurrent.atomic.AtomicReference<Boolean?>()
        val failure = java.util.concurrent.atomic.AtomicReference<Throwable?>()
        javax.swing.SwingUtilities.invokeAndWait {
            try {
                TerminalUtil.launchRestartCommandOffEdt(
                    target = target,
                    force = false,
                    editorText = editorText,
                    readAgent = { text ->
                        readOnEdt.set(javax.swing.SwingUtilities.isEventDispatchThread())
                        check(!javax.swing.SwingUtilities.isEventDispatchThread()) {
                            "agent-doc native calls are forbidden on the IDEA event-dispatch thread"
                        }
                        readAgent(text)
                    },
                    executeOffEdt = executeOffEdt,
                ) { built ->
                    launchOnEdt.set(javax.swing.SwingUtilities.isEventDispatchThread())
                    args.set(built)
                    launched.countDown()
                }
            } catch (error: Throwable) {
                failure.set(error)
                launched.countDown()
            }
        }
        assertTrue("restart command was never launched", launched.await(10, java.util.concurrent.TimeUnit.SECONDS))
        failure.get()?.let { throw AssertionError("Restart Agent's command build threw on the EDT", it) }
        return Triple(args.get(), readOnEdt.get(), launchOnEdt.get())
    }

    @Test
    fun `restart agent reads the editor visible agent off the EDT`() {
        val (args, readOnEdt, launchOnEdt) = launchRestartFromEdt(
            TerminalUtil.RestartTarget.AGENT,
            editorText = "---\nagent: grok\n---\nbody\n",
            readAgent = { "grok" },
        )
        assertEquals(listOf("restart-agent", "--expect-agent", "grok"), args)
        assertEquals("the native frontmatter read must not run on the EDT", false, readOnEdt)
        assertEquals("the restart launch must not run on the EDT", false, launchOnEdt)
    }

    @Test
    fun `an on EDT executor trips the modeled native guard and drops the expect agent evidence`() {
        // Proves the modeled guard bites: a synchronous executor puts the read back on the
        // EDT (the 0.35.558 regression), so the guard fires and the evidence is lost.
        var thrown: Throwable? = null
        javax.swing.SwingUtilities.invokeAndWait {
            thrown = runCatching {
                TerminalUtil.launchRestartCommandOffEdt(
                    target = TerminalUtil.RestartTarget.AGENT,
                    force = false,
                    editorText = "---\nagent: grok\n---\n",
                    readAgent = {
                        check(!javax.swing.SwingUtilities.isEventDispatchThread()) { "native on EDT" }
                        "grok"
                    },
                    executeOffEdt = { task -> task.run() },
                ) { built ->
                    assertEquals(listOf("restart-agent"), built)
                }
            }.exceptionOrNull()
        }
        // The helper contains the failed read (no --expect-agent) rather than throwing; the
        // production wiring is pinned to a pooled executor by the source test below.
        assertNull(thrown)
    }

    @Test
    fun `restart agent falls back to no expect agent when the native read fails or no worker runs`() {
        val (failedArgs, _, _) = launchRestartFromEdt(
            TerminalUtil.RestartTarget.AGENT,
            editorText = "---\nagent: grok\n---\n",
            readAgent = { throw UnsupportedOperationException("native library predates the export") },
        )
        assertEquals(listOf("restart-agent"), failedArgs)

        val (noTextArgs, noTextReadOnEdt, _) = launchRestartFromEdt(
            TerminalUtil.RestartTarget.AGENT,
            editorText = null,
            readAgent = { "grok" },
        )
        assertEquals(listOf("restart-agent"), noTextArgs)
        assertNull("no editor buffer means no native read", noTextReadOnEdt)

        val (supervisorArgs, supervisorReadOnEdt, _) = launchRestartFromEdt(
            TerminalUtil.RestartTarget.SUPERVISOR,
            editorText = "---\nagent: grok\n---\n",
            readAgent = { "grok" },
        )
        assertEquals(listOf("restart-supervisor"), supervisorArgs)
        assertNull("restart supervisor never reads the agent", supervisorReadOnEdt)

        val (rejectedArgs, rejectedReadOnEdt, _) = launchRestartFromEdt(
            TerminalUtil.RestartTarget.AGENT,
            editorText = "---\nagent: grok\n---\n",
            readAgent = { "grok" },
            executeOffEdt = { throw java.util.concurrent.RejectedExecutionException("unloading") },
        )
        assertEquals(listOf("restart-agent"), rejectedArgs)
        assertNull("a rejected worker never runs the native read on the EDT", rejectedReadOnEdt)
    }

    @Test
    fun `restart command wires the native agent read through the pooled off EDT launch`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ).toFile().readText()
        val runRestart = source.substringAfter("private fun runRestartCommand(")
            .substringBefore("internal fun launchRestartCommandOffEdt(")
        assertTrue(runRestart.contains("launchRestartCommandOffEdt("))
        assertTrue(runRestart.contains("readAgent = NativePatching::frontmatterAgent"))
        assertTrue(runRestart.contains("GenerationWork.executeOnPooledThread(task)"))
        assertFalse(
            "runRestartCommand runs on the EDT and must not call native code directly",
            runRestart.contains("NativePatching.frontmatterAgent("),
        )
        val bufferRead = source.substringAfter("private fun editorVisibleDocumentText(")
            .substringBefore("internal fun buildAdminQueueResumeCommand(")
        assertFalse(bufferRead.contains("NativePatching"))
        assertFalse(source.contains("editorVisibleFrontmatterAgent("))
    }

    @Test
    fun `restart agent carries the editor visible agent and supervisor restart does not`() {
        assertEquals(
            listOf("restart-agent", "--expect-agent", "grok"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.AGENT, force = false, expectAgent = "grok"),
        )
        assertEquals(
            listOf("restart-agent", "--force", "--expect-agent", "grok"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.AGENT, force = true, expectAgent = " grok "),
        )
        assertEquals(
            listOf("restart-agent"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.AGENT, force = false, expectAgent = " "),
        )
        assertEquals(
            listOf("restart-supervisor"),
            TerminalUtil.restartCommandArgs(TerminalUtil.RestartTarget.SUPERVISOR, force = false, expectAgent = "grok"),
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

    @Test
    fun `lint document uses read only lint command`() {
        assertEquals(
            listOf("agent-doc", "lint", "tasks/sample.md"),
            TerminalUtil.buildLintDocumentCommand("agent-doc", "tasks/sample.md"),
        )
    }

    @Test
    fun `document command failures are written to the durable idea log and editor strip`() {
        val terminalUtilPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
                    ),
                )
                .first { Files.exists(it) }
        val source = Files.readString(terminalUtilPath)
        val commandRunner =
            source.substringAfter("private fun runDocumentCommand(")
                .substringBefore("fun resolveAgentDoc(")

        assertTrue(commandRunner.contains("LOG.warn("))
        assertTrue(commandRunner.contains("[document-command] failed file="))
        assertTrue(commandRunner.contains("exit=${'$'}exitCode"))
        assertTrue(commandRunner.contains("output=${'$'}{output.ifBlank"))
        assertTrue(commandRunner.contains("[document-command] failed to start file="))
        assertTrue(commandRunner.contains("showEditorFailureStatus("))
    }

    @Test
    fun `missing actor failure tells the operator how to create a tmux pane`() {
        val (label, tooltip) = TerminalUtil.editorFailurePresentation(
            "restart the agent",
            "operator command `session_restart` rejected: stage=missing_actor receipt_id=1427",
        )

        assertEquals(
            "⚠ agent-doc: no tmux pane exists — use Load Tmux Window",
            label,
        )
        assertTrue(tooltip.contains("stage=missing_actor"))
    }

    @Test
    fun `malformed frontmatter failure is actionable in the editor`() {
        val (label, tooltip) = TerminalUtil.editorFailurePresentation(
            "open the tmux pane",
            "sync auto-start frontmatter: invalid YAML frontmatter at line 4 column 1",
        )

        assertEquals(
            "⚠ agent-doc: session frontmatter is invalid — fix YAML, then retry",
            label,
        )
        assertTrue(tooltip.contains("invalid YAML frontmatter"))
    }

    @Test
    fun `fixed frontmatter is identified as a stale controller diagnostic`() {
        val detail = """
            invalid YAML frontmatter at line 4 column 1
              > 4 | : resume:
        """.trimIndent()
        val editorText = """
            ---
            agent_doc_session: session-1
            agent: codex
            resume:
            ---
        """.trimIndent()

        val (label, tooltip) = TerminalUtil.editorFailurePresentation(
            "open the tmux pane",
            detail,
            editorText,
        )

        assertEquals(
            "⚠ agent-doc: the controller has stale frontmatter — editor sync is retrying",
            label,
        )
        assertTrue(tooltip.contains("> 4 | : resume:"))
    }

    @Test
    fun `generic document command failure keeps exact diagnostics in the tooltip`() {
        val detail = "socket refused the request"
        val (label, tooltip) = TerminalUtil.editorFailurePresentation(
            "open the tmux pane",
            detail,
        )

        assertEquals(
            "⚠ agent-doc: failed to open the tmux pane — open Event Log for details",
            label,
        )
        assertEquals(detail, tooltip)
    }

    @Test
    fun `agent-doc executable candidates honor PATH before fixed install locations`() {
        val pathDir = Files.createTempDirectory("agent-doc-path")
        val executable = pathDir.resolve("agent-doc")
        Files.writeString(executable, "#!/bin/sh\n")
        executable.toFile().setExecutable(true)
        try {
            assertEquals(
                listOf(
                    executable.toAbsolutePath().toString(),
                    "/workspace/.bin/agent-doc",
                    "/operator/bin/agent-doc",
                    "/operator/.local/bin/agent-doc",
                    "/operator/.cargo/bin/agent-doc",
                    "/usr/local/bin/agent-doc",
                ),
                TerminalUtil.agentDocCandidates("/workspace", "/operator", pathDir.toString()),
            )
        } finally {
            Files.deleteIfExists(executable)
            Files.deleteIfExists(pathDir)
        }
    }

    @Test
    fun `native loader uses shared executable resolution with auditable attempts`() {
        val nativeLibPath =
            listOf(
                    Paths.get("src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"
                    ),
                )
                .first { Files.exists(it) }
        val source = Files.readString(nativeLibPath)
        val resolver =
            source.substringAfter("private fun resolveLibPath()")
                .substringBefore("private val LOG")

        assertTrue(resolver.contains("TerminalUtil.resolveAgentDoc()"))
        assertTrue(resolver.contains("TerminalUtil.agentDocResolutionAttempts()"))
        assertFalse(resolver.contains("ProcessBuilder(\"agent-doc\""))
        assertFalse(resolver.contains("redirectErrorStream(true)"))
        assertTrue(resolver.contains("redirectErrorStream(false)"))
        assertTrue(resolver.contains("redirectError(ProcessBuilder.Redirect.INHERIT)"))
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
