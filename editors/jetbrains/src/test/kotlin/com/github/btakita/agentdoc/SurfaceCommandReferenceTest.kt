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
    ) = SurfaceCommandReference.decide(
        surfaceRoot = surfaceRoot,
        commandRoot = commandRoot,
        surfaceId = "frame-1",
        publishExitCode = exitCode,
        receiptJson = receipt,
        publishedSequence = sequence,
    )

    @Test
    fun `a capable controller is sent a reference to the publication it retained`() {
        assertEquals(
            SurfaceCommandReference.Decision.Reference(SurfaceLayoutReference("frame-1", 42)),
            decide(),
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
    fun `controller layout refusals keep the plugin's former sentences`() {
        assertEquals(
            "Editor split could not be read; the retained tmux layout was not changed.",
            SurfaceCommandReference.refusalMessage(
                "sync_tmux_layout rejected: surface_reference_unknown_layout: Editor split could not be read; ...",
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
        val snapshot = TerminalUtil.RouteLayoutRequest.fromEditorSnapshot(
            visibleMdFiles = listOf("/proj/a.md", "/proj/b.md"),
            editorLayout = layout,
            focusedFile = "/proj/a.md",
        )
        assertEquals(
            TerminalUtil.buildRouteLayoutArgs(listOf("/proj/a.md", "/proj/b.md"), layout, "/proj/a.md"),
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

    @Test
    fun `run builds no col arguments outside the logged snapshot fallback`() {
        val source = Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt")
            .toFile().readText()
        val send = source.substringAfter("internal fun sendToTerminal(")
            .substringBefore("internal fun routePaneNotVisibleNotice(")
        assertFalse(send.contains("LayoutDetector.detectEditorLayout"))
        assertFalse(send.contains("buildRouteLayoutArgs("))
        assertTrue(send.contains("captureCommandSurface(project, file)"))
        assertTrue(send.contains("surfaceReference = routeLayout.surfaceReference"))

        val resolve = source.substringAfter("private fun resolveRouteLayout(")
            .substringBefore("private fun editorSnapshotRouteLayout(")
        assertTrue(
            "the reference path publishes before it names the publication",
            resolve.indexOf("publishCommandSurface(") < resolve.indexOf("bySurfaceReference("),
        )
        assertTrue(resolve.contains("sending the --col snapshot"))
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
        assertTrue(byReference.contains("allowSurfaceReference = false"))

        val sync = source.substringAfter("fun syncLayout(").substringBefore("private fun syncLayoutBySurfaceReference(")
        assertTrue(
            "the reference path is chosen before the plugin reads its own split",
            sync.indexOf("captureCommandSurface(project, focusedVFile)") <
                sync.indexOf("LayoutDetector.detectEditorLayout("),
        )
    }
}
