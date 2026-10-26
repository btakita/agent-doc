package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `surface_layout_reference_v1` (`specs/13-state-backbone.md` "State Propagation, Not Effect
 * Inference"): Run Agent Doc and Sync Tmux Panes name the surface the controller already retains
 * instead of building `--col` arguments, and keep the `--col` snapshot only for an older
 * controller.
 */
class SurfaceCommandReferenceTest {

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

    private val capableReceipt =
        """{"intent":"Idle","idle":true,"capabilities":["surface_layout_reference_v1"]}"""

    private fun inlinePayload(submit: JsonObject): JsonObject {
        val bytes = submit.getAsJsonObject("payload").getAsJsonArray("Inline")
            .map { it.asInt.toByte() }
            .toByteArray()
        return JsonParser.parseString(String(bytes, Charsets.UTF_8)).asJsonObject
    }

    private fun decide(
        receipt: String = capableReceipt,
        exitCode: Int = 0,
        surfaceRoot: String = "/proj",
        commandRoot: String = "/proj",
        sequence: Long? = 42,
        command: SurfaceCommandReference.Command = SurfaceCommandReference.Command.Route,
        columnGeometry: SurfaceCommandReference.ColumnGeometry =
            SurfaceCommandReference.ColumnGeometry.Known,
    ) = SurfaceCommandReference.decide(
        surfaceRoot = surfaceRoot,
        commandRoot = commandRoot,
        surfaceId = "frame-1",
        publishExitCode = exitCode,
        receiptJson = receipt,
        publishedSequence = sequence,
        command = command,
        columnGeometry = columnGeometry,
    )

    @Test
    fun `a capable controller is sent a reference to the publication it retained`() {
        assertEquals(
            SurfaceCommandReference.Decision.Reference(SurfaceLayoutReference("frame-1", 42)),
            decide(),
        )
    }

    @Test
    fun `flat geometry routes by reference but exact sync preserves retained layout`() {
        val flat = SurfaceCommandReference.ColumnGeometry.Unknown
        val reference = SurfaceLayoutReference("frame-1", 42)

        assertEquals(
            SurfaceCommandReference.Decision.Reference(reference),
            decide(command = SurfaceCommandReference.Command.Route, columnGeometry = flat),
        )
        assertEquals(
            SurfaceCommandReference.Decision.PreserveRetainedLayout(reference),
            decide(command = SurfaceCommandReference.Command.ExactSync, columnGeometry = flat),
        )
    }

    @Test
    fun `exact geometry syncs by reference`() {
        assertEquals(
            SurfaceCommandReference.Decision.Reference(SurfaceLayoutReference("frame-1", 42)),
            decide(command = SurfaceCommandReference.Command.ExactSync),
        )
    }

    @Test
    fun `an older controller without the capability keeps the col snapshot`() {
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback(
                "controller_lacks_surface_layout_reference_v1",
            ),
            decide(receipt = """{"intent":"Idle","idle":true}"""),
        )
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback(
                "controller_lacks_surface_layout_reference_v1",
            ),
            decide(receipt = """{"idle":true,"capabilities":["something_else_v1"]}"""),
        )
    }

    @Test
    fun `an unretained or foreign publication is never named`() {
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback("surface_publish_failed"),
            decide(exitCode = 1),
        )
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback("surface_publish_failed"),
            decide(
                exitCode = 1,
                command = SurfaceCommandReference.Command.ExactSync,
                columnGeometry = SurfaceCommandReference.ColumnGeometry.Unknown,
            ),
        )
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback("surface_publication_rejected=stale"),
            decide(
                receipt =
                    """{"idle":true,"rejected":"stale","capabilities":["surface_layout_reference_v1"]}""",
            ),
        )
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback(
                "cross_root surface_root=/outer command_root=/outer/nested",
            ),
            decide(surfaceRoot = "/outer", commandRoot = "/outer/nested"),
        )
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback("surface_sequence_unknown"),
            decide(sequence = null),
        )
        assertEquals(
            SurfaceCommandReference.Decision.SnapshotFallback("surface_receipt_unreadable"),
            decide(receipt = "not json"),
        )
    }

    @Test
    fun `controller layout refusals keep actionable plugin messages`() {
        assertEquals(
            "Remote Dev frontend structural routing is not ready (frontend_authority_absent_or_generation_mismatched; flat_evidence_has_no_column_geometry); the retained tmux layout was not changed. Remedy: install the same Agent Doc modular plugin ZIP in JetBrains Client/Gateway and the Remote Dev backend, verify both sides report that exact plugin version, restart both (including the JetBrains Client/Gateway process; restarting only the Remote Dev backend cannot replace its already-loaded frontend module), then retry Sync Tmux Layout.",
            SurfaceCommandReference.refusalMessage(
                "sync_tmux_layout rejected: surface_reference_unknown_layout: Editor split could not be read; ...",
            ),
        )
        assertTrue(
            SurfaceCommandReference.UNKNOWN_LAYOUT_MESSAGE.contains(
                SurfaceCommandReference.FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY,
            ),
        )
        assertTrue(
            SurfaceCommandReference.UNKNOWN_LAYOUT_MESSAGE.contains(
                SurfaceCommandReference.FRONTEND_AUTHORITY_UNAVAILABLE_READINESS,
            ),
        )
        assertEquals(
            "Editor focus changed while the split layout was being read; retry after the selection settles.",
            SurfaceCommandReference.refusalMessage("surface_reference_stale_focus: ..."),
        )
        assertNull(SurfaceCommandReference.refusalMessage("pane creation failed"))
    }

    @Test
    fun `sync by reference carries no plugin-built columns, order, or focus`() {
        val request = CpRouteClient.syncTmuxLayoutCommandSubmitRequest(
            projectRoot = "/proj",
            columnsJson = """["/proj/tasks/should-not-be-sent.md"]""",
            window = null,
            focus = "/proj/tasks/two.md",
            noAutostart = false,
            exactVisible = true,
            commandId = "cmd-sync",
            columnOrder = "editor",
            clientId = "jetbrains@test-pid:1",
            surfaceReference = SurfaceLayoutReference("frame-1", 7),
        )
        // The envelope still names the focused document for admission.
        assertEquals("/proj/tasks/two.md", request.get("file").asString)
        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val payload = inlinePayload(message.getAsJsonObject("CommandSubmit"))
        assertFalse(payload.has("columns"))
        assertFalse(payload.has("column_order"))
        assertFalse(payload.has("focus"))
        assertEquals("frame-1", payload.getAsJsonObject("surface_reference").get("surface_id").asString)
        assertEquals(7L, payload.getAsJsonObject("surface_reference").get("min_sequence").asLong)
        assertEquals("jetbrains@test-pid:1", payload.get("client_id").asString)
        assertEquals(true, payload.get("exact_visible").asBoolean)
        assertEquals("manual", payload.get("caller_kind").asString)
    }

    @Test
    fun `route by reference carries no plugin-built col arguments`() {
        val request = CpRouteClient.editorCommandSubmitRequest(
            filePath = "/proj/tasks/one.md",
            relativePath = "tasks/one.md",
            layoutArgs = listOf("--col", "/proj/tasks/should-not-be-sent.md"),
            waitForReadySeconds = 120,
            attemptId = "attempt-1",
            routeKey = "route-1",
            commandId = "cmd-route",
            columnOrder = "editor",
            layoutMode = "exact",
            clientId = "jetbrains@test-pid:1",
            surfaceReference = SurfaceLayoutReference("frame-1", 9),
        )
        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val payload = inlinePayload(message.getAsJsonObject("CommandSubmit"))
        assertEquals(0, payload.getAsJsonArray("layout_args").size())
        assertFalse(payload.has("column_order"))
        assertFalse(payload.has("layout_mode"))
        assertEquals(9L, payload.getAsJsonObject("surface_reference").get("min_sequence").asLong)
        assertEquals("jetbrains@test-pid:1", payload.get("client_id").asString)

        val classic = CpRouteClient.editorRouteRequest(
            filePath = "/proj/tasks/one.md",
            relativePath = "tasks/one.md",
            layoutArgs = listOf("--col", "/proj/tasks/should-not-be-sent.md"),
            waitForReadySeconds = 120,
            attemptId = null,
            routeKey = null,
            surfaceReference = SurfaceLayoutReference("frame-1", 9),
        )
        val classicPayload = JsonParser.parseString(classic.get("diagnostic_payload").asString).asJsonObject
        assertEquals(0, classicPayload.getAsJsonArray("layout_args").size())
        assertTrue(classicPayload.has("surface_reference"))
    }

    @Test
    fun `the snapshot fallback is the former col publication unchanged`() {
        val layout = EditorLayout(
            listOf(LayoutColumn(listOf("/proj/a.md")), LayoutColumn(listOf("/proj/b.md"))),
        )
        val snapshot = routeSnapshot("/proj", listOf("/proj/a.md", "/proj/b.md"), layout, "/proj/a.md")
        assertEquals(
            listOf("--col", "/proj/a.md", "--col", "/proj/b.md", "--focus", "/proj/a.md"),
            snapshot.layoutArgs,
        )
        assertEquals("editor", snapshot.columnOrder)
        assertEquals(TerminalUtil.ROUTE_LAYOUT_MODE_EXACT, snapshot.layoutMode)
        assertNull(snapshot.surfaceReference)
        assertEquals(TerminalUtil.RouteLayoutRequest.STAGE_SNAPSHOT_FALLBACK, snapshot.stage)

        val byReference =
            TerminalUtil.RouteLayoutRequest.bySurfaceReference(SurfaceLayoutReference("frame-1", 3))
        assertTrue(byReference.layoutArgs.isEmpty())
        assertNull(byReference.columnOrder)
        assertNull(byReference.layoutMode)
        assertEquals(listOf("--surface-reference", "frame-1@3"), byReference.diagnosticArgs)
    }

    /**
     * GH #376: a Remote Dev backend whose surface defers to the JetBrains Client's measured layout
     * sends a `--col` fallback that names no document. The route names that authority so the
     * controller takes, or awaits, the measured forward; without it the field is absent.
     */
    @Test
    fun `a measured-authority snapshot fallback names its layout authority`() {
        val nothingVisible = routeSnapshot("/proj", emptyList(), null, null)
        assertTrue(nothingVisible.layoutArgs.isEmpty())
        assertNull(nothingVisible.layoutAuthority)
        assertNull(nothingVisible.withMeasuredLayoutAuthority(active = false).layoutAuthority)
        val measured = nothingVisible.withMeasuredLayoutAuthority(active = true)
        assertEquals("frontend_measured", measured.layoutAuthority)
        assertEquals(nothingVisible.copy(layoutAuthority = "frontend_measured"), measured)

        val request = CpRouteClient.editorCommandSubmitRequest(
            filePath = "/proj/tasks/one.md",
            relativePath = "tasks/one.md",
            layoutArgs = measured.layoutArgs,
            waitForReadySeconds = 120,
            attemptId = "attempt-1",
            routeKey = "route-1",
            commandId = "cmd-route",
            columnOrder = measured.columnOrder,
            layoutMode = measured.layoutMode,
            clientId = "jetbrains@test-pid:1",
            layoutAuthority = measured.layoutAuthority,
        )
        val message = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val payload = inlinePayload(message.getAsJsonObject("CommandSubmit"))
        assertEquals(0, payload.getAsJsonArray("layout_args").size())
        assertEquals("frontend_measured", payload.get("layout_authority").asString)

        val classic = CpRouteClient.editorRouteRequest(
            filePath = "/proj/tasks/one.md",
            relativePath = "tasks/one.md",
            layoutArgs = emptyList(),
            waitForReadySeconds = 120,
            attemptId = null,
            routeKey = null,
        )
        val classicPayload = JsonParser.parseString(classic.get("diagnostic_payload").asString).asJsonObject
        assertFalse(classicPayload.has("layout_authority"))

        val source = Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt")
            .toFile().readText()
        val resolve = source.substringAfter("private fun resolveRouteLayout(")
            .substringBefore("private fun editorSnapshotRouteLayout(")
        assertTrue(resolve.contains("RemoteClientLayoutAuthority.isActive(project)"))
        assertTrue(resolve.contains("withMeasuredLayoutAuthority(measuredAuthority)"))
        assertTrue(source.contains("layoutAuthority = routeLayout.layoutAuthority"))
    }

    @Test
    fun `run builds no col arguments outside the logged snapshot fallback`() {
        val source = Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt")
            .toFile().readText()
        val send = source.substringAfter("internal fun sendToTerminal(")
            .substringBefore("internal fun routePaneNotVisibleNotice(")
        assertFalse(send.contains("LayoutDetector.detectEditorLayout"))
        assertFalse(send.contains("fromSyncLayout("))
        assertFalse(send.contains("SyncLayoutSnapshot.derive("))
        assertTrue(send.contains("captureCommandSurface(project, file)"))
        assertTrue(send.contains("surfaceReference = routeLayout.surfaceReference"))

        val resolve = source.substringAfter("private fun resolveRouteLayout(")
            .substringBefore("private fun editorSnapshotRouteLayout(")
        assertTrue(
            "the reference path publishes before it names the publication",
            resolve.indexOf("publishCommandSurface(") < resolve.indexOf("bySurfaceReference("),
        )
        assertTrue(resolve.contains("sending the --col snapshot"))
        assertTrue(resolve.contains("SurfaceCommandReference.Command.Route"))
    }

    @Test
    fun `sync by reference publishes the surface and submits no columns`() {
        val source = Paths.get("src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt")
            .toFile().readText()
        val byReference = source.substringAfter("private fun syncLayoutBySurfaceReference(")
            .substringBefore("override fun actionPerformed")
        val publish = byReference.indexOf("listener.publishCommandSurface(")
        val submit = byReference.indexOf("CpRouteClient.submitSyncTmuxLayout(")
        assertTrue(publish in 0 until submit)
        assertTrue(byReference.contains("surfaceReference = reference"))
        assertTrue(byReference.contains("columnsJson = \"[]\""))
        assertFalse(byReference.contains("detectEditorLayout"))
        // GH #334: the publication fallback must preserve the first pass's client-scoped focus.
        // Re-entering syncLayout under the backend ClientId loses the Remote Dev selection.
        assertFalse(byReference.contains("syncLayout("))
        assertTrue(byReference.contains("syncLayoutBySnapshot("))
        assertTrue(byReference.contains("request.copy("))
        assertTrue(byReference.contains("terminalPrepared = true"))
        assertTrue(byReference.contains("allowSurfaceReference = false"))
        assertTrue(byReference.contains("request.visibleMdFiles.toSet()"))
        assertTrue(byReference.contains("SurfaceCommandReference.Command.ExactSync"))
        val preserve = byReference
            .substringAfter("is EditorTabSyncListener.CommandSurfacePublication.PreservedLayout ->")
            .substringBefore("} catch")
        assertTrue(preserve.contains("flat_evidence_preserves_retained_layout"))
        assertFalse(preserve.contains("CpRouteClient.submitSyncTmuxLayout("))
        assertFalse(preserve.contains("request.notifyFailure("))

        val sync = source.substringAfter("fun syncLayout(").substringBefore("private data class SyncRequest(")
        val capture = sync.indexOf("captureCommandSurface(project, focusedVFile)")
        assertTrue(
            "the reference path is chosen before the plugin reads its own split",
            capture in 0 until sync.indexOf("LayoutDetector.captureEditorLayoutEvidence("),
        )
        // GH #326 phase 3: the EDT reads editor state only; the layout and the sync decisions are
        // derived on the background thread.
        for (rule in listOf(
            "LayoutDetector.detectEditorLayout(",
            "chooseSyncProjectRoot(",
            "normalizeEditorLayout(",
            "absolutizeEditorLayout(",
            "exactVisibleSyncDecision(",
            "TerminalUtil.resolveProject(",
            "SyncLayoutSnapshot.derive(",
        )) {
            assertFalse("GH #326 phase 3: the EDT half of syncLayout must not run $rule", sync.contains(rule))
        }
        val snapshot = source.substringAfter("private fun syncLayoutBySnapshot(")
            .substringBefore("private fun syncLayoutBySurfaceReference(")
        val thread = snapshot.indexOf("Thread {")
        assertTrue(thread in 0 until snapshot.indexOf("LayoutDetector.detectEditorLayout("))
        assertTrue(snapshot.indexOf("deriveSyncLayout(") in thread until snapshot.indexOf("CpRouteClient.submitSyncTmuxLayout("))
        assertTrue(byReference.indexOf("Thread {") in 0 until byReference.indexOf("deriveSyncLayout("))
    }
}
