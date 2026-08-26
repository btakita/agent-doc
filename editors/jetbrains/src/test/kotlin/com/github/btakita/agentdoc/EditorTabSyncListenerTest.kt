package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import java.nio.file.Files
import java.nio.file.Paths
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The plugin does not plan layout (`#jbsurfaceswap`) — dedup and reconciliation are the reactive
 * graph's, exercised by `agent-doc-editor-surface`. The editor reports the surface it actually
 * sees and separately publishes focused-document state to that document's own controller so
 * cross-project splits get a derived pane handoff.
 */
class EditorTabSyncListenerTest {
    private val gson = Gson()

    private fun surfaceJson(
        focusedFile: String,
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout? = null,
        forceReconcile: Boolean = false,
    ): String =
        gson.toJson(
            EditorTabSyncListener.SurfaceReport.buildSurface(
                focusedFile = focusedFile,
                visibleMdFiles = visibleMdFiles,
                editorLayout = editorLayout,
                forceReconcile = forceReconcile,
            )
        )

    @Test
    fun `focus projection is generation fenced and requires the active project window`() {
        assertTrue(
            EditorTabSyncListener.shouldPublishFocusProjection(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldPublishFocusProjection(
                requestedGeneration = 6,
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldPublishFocusProjection(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = false,
            ),
        )
    }

    @Test
    fun `focus projection installs a handoff lease only after exact pane selection`() {
        assertTrue(
            EditorTabSyncListener.focusProjectionApplied(
                """{"idle":false,"outcome":"{\"focused\":true,\"reason\":\"focused\"}","error":null}""",
            ),
        )
        assertFalse(
            EditorTabSyncListener.focusProjectionApplied(
                """{"idle":false,"outcome":null,"error":"missing_actor_record"}""",
            ),
        )
        assertFalse(
            EditorTabSyncListener.focusProjectionApplied(
                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"missing_actor_record\"}"}""",
            ),
        )
    }

    @Test
    fun `stashed focus projection requests structural layout repair only for visible drift`() {
        assertTrue(
            EditorTabSyncListener.focusProjectionRequiresLayoutRepair(
                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"actor_pane_not_visible\"}"}""",
            ),
        )
        assertFalse(
            EditorTabSyncListener.focusProjectionRequiresLayoutRepair(
                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"missing_actor_record\"}"}""",
            ),
        )
        assertFalse(
            EditorTabSyncListener.focusProjectionRequiresLayoutRepair(
                """{"idle":false,"outcome":"{\"focused\":true,\"reason\":\"focused\"}"}""",
            ),
        )
    }

    @Test
    fun `selection event file wins over stale selected editor file`() {
        val visibleMdFiles =
            listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/tasks/professional/sampleportal.md",
            )

        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                selectedEditorFile = "/repo/tasks/professional/sampleportal.md",
                visibleMdFiles = visibleMdFiles,
            )

        assertEquals("/repo/tasks/agent-doc/agent-doc-bugs2.md", activeFile)
    }

    @Test
    fun `focused document projection is explicitly selection only`() {
        val projection =
            JsonParser.parseString(
                gson.toJson(
                EditorTabSyncListener.SurfaceReport.buildFocusProjection(
                    "/repo/src/sample-app/tasks/sample-app.md",
                ),
            ),
        ).asJsonObject

        assertEquals(
            "/repo/src/sample-app/tasks/sample-app.md",
            projection.get("focused").asString,
        )
        assertTrue(projection.get("focus_only").asBoolean)
        assertTrue(projection.get("force_reconcile").asBoolean)
        assertEquals(0, projection.getAsJsonArray("columns").size())
    }

    @Test
    fun `captured selection releases precedence while controller delivery is in flight`() {
        val captured = Any()
        val newerObservation = Any()
        val slot = AtomicReference<Any?>(captured)

        assertTrue(
            EditorTabSyncListener.ObservationDeliveryOwnership.releaseForDelivery(
                slot,
                captured,
            ),
        )
        assertTrue(slot.compareAndSet(null, newerObservation))
        assertFalse(
            EditorTabSyncListener.ObservationDeliveryOwnership.retainAfterFailure(
                slot,
                captured,
            ),
        )
        assertEquals(newerObservation, slot.get())
    }

    @Test
    fun `failed delivery retains the captured observation when no newer event exists`() {
        val captured = Any()
        val slot = AtomicReference<Any?>(captured)

        assertTrue(
            EditorTabSyncListener.ObservationDeliveryOwnership.releaseForDelivery(
                slot,
                captured,
            ),
        )
        assertTrue(
            EditorTabSyncListener.ObservationDeliveryOwnership.retainAfterFailure(
                slot,
                captured,
            ),
        )
        assertEquals(captured, slot.get())
    }

    @Test
    fun `failed surface delivery retries promptly then settles at a bounded rate`() {
        assertEquals(100L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(1))
        assertEquals(200L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(2))
        assertEquals(1_600L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(5))
        assertEquals(2_000L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(6))
        assertEquals(2_000L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(100))
    }

    @Test
    fun `publishing through a new controller root retires the prior layout authority`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempt("/repo/nested")
        assertTrue(ownership.markPublished("/repo/nested").isEmpty())

        ownership.recordAttempt("/repo")
        assertEquals(listOf("/repo/nested"), ownership.markPublished("/repo"))
        assertTrue(ownership.markForgotten("/repo/nested"))
        assertTrue(ownership.markPublished("/repo").isEmpty())
        assertEquals(listOf("/repo"), ownership.drain())
    }

    @Test
    fun `a fresh plugin process retires controller roots discovered from open documents`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempts(listOf("/repo", "/repo/nested"))

        assertEquals(
            listOf("/repo/nested"),
            ownership.rootsToRetireBeforePublishing("/repo"),
        )
        assertTrue(ownership.markForgotten("/repo/nested"))
        assertTrue(ownership.markPublished("/repo").isEmpty())
        assertEquals(listOf("/repo"), ownership.drain())
    }

    @Test
    fun `prior process roots are retired before replacement publication can block on layout`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempts(listOf("/repo", "/repo/nested"))

        assertEquals(
            listOf("/repo/nested"),
            ownership.rootsToRetireBeforePublishing("/repo"),
        )
        assertTrue(ownership.markForgotten("/repo/nested"))
        assertTrue(ownership.rootsToRetireBeforePublishing("/repo").isEmpty())
    }

    @Test
    fun `failed superseded root retirement remains retryable`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempt("/repo/nested")
        ownership.markPublished("/repo/nested")
        ownership.recordAttempt("/repo")

        assertEquals(listOf("/repo/nested"), ownership.markPublished("/repo"))
        assertEquals(
            "an unacknowledged forget must remain eligible for the next delivery",
            listOf("/repo/nested"),
            ownership.markPublished("/repo"),
        )
    }

    @Test
    fun `surface projection waits while the selected document is absent`() {
        assertEquals(
            EditorTabSyncListener.SurfaceReport.ProjectionReadiness.AwaitingSelectedDocument,
            EditorTabSyncListener.SurfaceReport.projectionReadiness(
                preferredActiveFile = "/repo/left-next.md",
                visibleMdFiles = listOf("/repo/right.md"),
            ),
        )
    }

    @Test
    fun `surface projection becomes current when the selected document is visible`() {
        assertEquals(
            EditorTabSyncListener.SurfaceReport.ProjectionReadiness.Current,
            EditorTabSyncListener.SurfaceReport.projectionReadiness(
                preferredActiveFile = "/repo/left-next.md",
                visibleMdFiles = listOf("/repo/left-next.md", "/repo/right.md"),
            ),
        )
    }

    @Test
    fun `surface projection waits until every restored editor window selects a file`() {
        assertFalse(
            EditorTabSyncListener.SurfaceReport.restoredEditorWindowsReady(
                listOf("/repo/right.md", null),
            ),
        )
        assertTrue(
            EditorTabSyncListener.SurfaceReport.restoredEditorWindowsReady(
                listOf("/repo/right.md", "/repo/left.md"),
            ),
        )
    }

    @Test
    fun `restored window selections remain authoritative while selected-files aggregate lags`() {
        val laggingSelectedFiles = listOf("/repo/nested/right.md")
        val restoredWindowFiles =
            listOf(
                "/repo/tasks/agent-doc/left.md",
                "/repo/nested/right.md",
            )

        assertEquals(1, laggingSelectedFiles.size)
        assertEquals(
            restoredWindowFiles,
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                restoredWindowFiles,
            ),
        )
    }

    @Test
    fun `a window switched to source keeps standing for its last agent-doc file`() {
        // #stickymdpane: two editor columns, the right one detoured to source.
        // The mirrored tmux layout must stay two panes with the last document
        // still selected, not collapse to one.
        assertEquals(
            listOf("/repo/tasks/left.md", "/repo/tasks/right.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/src/Thing.kt"),
                listOf("/repo/tasks/left.md", "/repo/tasks/right.md"),
            ),
        )
    }

    @Test
    fun `a window that never showed a document still contributes no pane`() {
        // Closing a split, or opening a fresh source-only one, must still
        // collapse the layout — the fallback is memory, not invention.
        assertEquals(
            listOf("/repo/tasks/left.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/src/Thing.kt"),
                listOf("/repo/tasks/left.md", null),
            ),
        )
        assertEquals(
            listOf("/repo/tasks/left.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/src/Thing.kt"),
                listOf("/repo/tasks/left.md", "/repo/src/Other.kt"),
            ),
        )
    }

    @Test
    fun `a live selection always beats the remembered document`() {
        assertEquals(
            listOf("/repo/tasks/left.md", "/repo/tasks/now.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/tasks/now.md"),
                listOf("/repo/tasks/left.md", "/repo/tasks/stale.md"),
            ),
        )
    }

    @Test
    fun `a selected non-session markdown plan falls back to the windows agent doc`() {
        val left = "/repo/tasks/left.md"
        val right = "/repo/tasks/backend.md"
        val plan = "/repo/docs/backend-fpe-contracts-sdk-pr-plan.md"

        assertEquals(
            listOf(left, right),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                selectedWindowFiles = listOf(left, plan),
                stickyWindowFallbacks = listOf(left, right),
                sessionDocumentPaths = setOf(left, right),
            ),
        )
    }

    @Test
    fun `stale selected-files projection is reread on later EDT turns`() {
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                remainingPasses = 1,
            ),
        )
        assertFalse(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                remainingPasses = 0,
            ),
        )
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.Layout,
                remainingPasses =
                    EditorTabSyncListener.SelectionProjectionSettling.MAX_REPROJECTION_PASSES,
            ),
        )
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.FileOpened,
                remainingPasses =
                    EditorTabSyncListener.SelectionProjectionSettling.MAX_REPROJECTION_PASSES,
            ),
        )
    }

    @Test
    fun `exhausted selection settling substitutes the event edge into stale editor projections`() {
        val settled =
            EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                preferredFile = "/repo/left-next.md",
                previousFile = "/repo/left-old.md",
                visibleMdFiles = listOf("/repo/left-old.md", "/repo/right.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/left-old.md")),
                            LayoutColumn(listOf("/repo/right.md")),
                        ),
                    ),
            )

        assertEquals(
            listOf("/repo/left-next.md", "/repo/right.md"),
            settled.visibleMdFiles,
        )
        assertEquals(
            listOf(
                LayoutColumn(listOf("/repo/left-next.md")),
                LayoutColumn(listOf("/repo/right.md")),
            ),
            settled.editorLayout?.columns,
        )
    }

    @Test
    fun `exhausted selection settling repairs stale layout after selected files advance`() {
        assertEquals(
            EditorTabSyncListener.SurfaceReport.ProjectionReadiness.AwaitingSelectedDocument,
            EditorTabSyncListener.SurfaceReport.projectionReadiness(
                preferredActiveFile = "/repo/left-next.md",
                visibleMdFiles = listOf("/repo/left-next.md", "/repo/right.md"),
                layoutMdFiles = listOf("/repo/left-old.md", "/repo/right.md"),
            ),
        )

        val settled =
            EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                preferredFile = "/repo/left-next.md",
                previousFile = "/repo/left-old.md",
                visibleMdFiles = listOf("/repo/left-next.md", "/repo/right.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/left-old.md")),
                            LayoutColumn(listOf("/repo/right.md")),
                        ),
                    ),
            )

        assertEquals(
            listOf("/repo/left-next.md", "/repo/right.md"),
            settled.visibleMdFiles,
        )
        assertEquals(
            listOf(
                LayoutColumn(listOf("/repo/left-next.md")),
                LayoutColumn(listOf("/repo/right.md")),
            ),
            settled.editorLayout?.columns,
        )
    }

    @Test
    fun `selection settling never invents a replacement without the prior event file`() {
        val staleVisible = listOf("/repo/left-old.md", "/repo/right.md")
        val staleLayout =
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/left-old.md")),
                    LayoutColumn(listOf("/repo/right.md")),
                ),
            )

        val settled =
            EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                preferredFile = "/repo/left-next.md",
                previousFile = null,
                visibleMdFiles = staleVisible,
                editorLayout = staleLayout,
            )

        assertEquals(staleVisible, settled.visibleMdFiles)
        assertEquals(staleLayout, settled.editorLayout)
    }

    @Test
    fun `selected editor file is used when no selection event file is supplied`() {
        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = null,
                selectedEditorFile = "/repo/tasks/professional/sampleportal.md",
                visibleMdFiles = listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md"),
            )

        assertEquals("/repo/tasks/professional/sampleportal.md", activeFile)
    }

    @Test
    fun `first visible markdown file is the last resort active file`() {
        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = "",
                selectedEditorFile = null,
                visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
            )

        assertEquals("/repo/a.md", activeFile)
    }

    @Test
    fun `no visible markdown means no active file to report`() {
        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = null,
                selectedEditorFile = null,
                visibleMdFiles = emptyList(),
            )

        assertNull(activeFile)
    }

    @Test
    fun `observation reports the split layout the editor detected`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/b.md",
                visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/a.md")),
                            LayoutColumn(listOf("/repo/b.md")),
                        )
                    ),
            )

        val surface = JsonParser.parseString(json).asJsonObject
        assertEquals("/repo/b.md", surface.get("focused").asString)
        assertEquals(
            listOf("/repo/a.md", "/repo/b.md"),
            surface.getAsJsonArray("visible").map { it.asString },
        )
        assertEquals(
            listOf("/repo/b.md", "/repo/a.md"),
            surface.getAsJsonArray("open").map { it.asString },
        )
        val columns = surface.getAsJsonArray("columns")
        assertEquals(2, columns.size())
        assertEquals(
            listOf("/repo/a.md"),
            columns[0].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
        assertEquals(
            listOf("/repo/b.md"),
            columns[1].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
    }

    @Test
    fun `column order is preserved rather than sorted`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/b.md",
                visibleMdFiles = listOf("/repo/b.md", "/repo/a.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/b.md")),
                            LayoutColumn(listOf("/repo/a.md")),
                        )
                    ),
            )

        val columns = JsonParser.parseString(json).asJsonObject.getAsJsonArray("columns")
        assertEquals(
            listOf("/repo/b.md"),
            columns[0].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
    }

    @Test
    fun `an undetected layout reports no columns rather than a synthesized one`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/a.md",
                visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
                editorLayout = null,
            )

        val surface = JsonParser.parseString(json).asJsonObject
        assertEquals(0, surface.getAsJsonArray("columns").size())
        assertEquals(2, surface.getAsJsonArray("visible").size())
    }

    @Test
    fun `blank and duplicate layout entries are dropped from the observation`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/a.md",
                visibleMdFiles = listOf("/repo/a.md", "/repo/a.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/a.md", "", "/repo/a.md")),
                            LayoutColumn(listOf("", "")),
                        )
                    ),
            )

        val surface = JsonParser.parseString(json).asJsonObject
        assertEquals(listOf("/repo/a.md"), surface.getAsJsonArray("visible").map { it.asString })
        val columns = surface.getAsJsonArray("columns")
        assertEquals(1, columns.size())
        assertEquals(
            listOf("/repo/a.md"),
            columns[0].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
    }

    @Test
    fun `force reconcile crosses the wire in the shape the graph reads`() {
        val forced =
            JsonParser.parseString(
                    surfaceJson("/repo/a.md", listOf("/repo/a.md"), forceReconcile = true)
                )
                .asJsonObject
        val unforced =
            JsonParser.parseString(
                    surfaceJson("/repo/a.md", listOf("/repo/a.md"), forceReconcile = false)
                )
                .asJsonObject

        assertTrue(forced.get("force_reconcile").asBoolean)
        assertFalse(unforced.get("force_reconcile").asBoolean)
    }

    @Test
    fun `observation reports no layout_synced field for the controller to answer`() {
        val surface =
            JsonParser.parseString(surfaceJson("/repo/a.md", listOf("/repo/a.md"))).asJsonObject

        assertFalse(surface.has("layout_synced"))
        assertFalse(surface.has("layoutSynced"))
    }

    @Test
fun `document selection publishes focus only when its split is active`() {
val source =
Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    )
            )
        val selection =
            source
                .substringAfter("override fun selectionChanged(event: FileEditorManagerEvent)")
.substringBefore("fun onEditorFocusGained")
assertTrue(selection.contains("requestObservation("))
assertTrue(selection.contains("requestFocusProjection(project, file)"))
assertTrue(selection.contains("FileEditorManagerEx.getInstanceEx(project).currentWindow"))
assertTrue(selection.contains("SelectionFocusAuthority.decide("))
assertTrue(selection.contains("preferredFile = file.takeIf { selectionOwnsFocus }"))
assertFalse(selection.contains("manager.selectedFiles"))
assertFalse(selection.contains("collectVisibleMarkdownFiles"))
        assertFalse(selection.contains("CpRouteClient"))
        assertTrue(selection.contains("forceReconcile = false"))
        assertTrue(selection.contains("previousFile = event.oldFile"))
        assertFalse(
            "ordinary tab focus must not force a competing full layout reconcile",
            selection.contains("forceReconcile = true"),
        )

        val focusGained =
            source
                .substringAfter("fun onEditorFocusGained(project: Project, file: VirtualFile)")
                .substringBefore("fun onEditorLayoutChanged")
        assertTrue(focusGained.contains("requestFocusProjection(project, file)"))
        assertTrue(focusGained.contains("requestObservation("))
        assertTrue(focusGained.contains("preferredFile = file"))
        assertTrue(focusGained.contains("forceReconcile = false"))
        assertTrue(focusGained.contains("authority = ObservationAuthority.EditorFocus"))
        assertFalse(focusGained.contains("delayMs"))

        val focusProjection =
            source
                .substringAfter("private fun requestFocusProjection(project: Project, file: VirtualFile)")
                .substringBefore("private fun shutdown()")
        assertTrue(focusProjection.contains("TerminalUtil.resolveProject(project, file)"))
        assertTrue(focusProjection.contains("CpRouteClient.observeEditorFocus("))
        assertFalse(focusProjection.contains("submitFocusDocumentPane("))
        assertTrue(
            focusProjection.indexOf("focusProjectionApplied(receipt.output)") <
                focusProjection.indexOf("TmuxPaneFocusSync.recordEditorFocusIntent("),
        )
        assertTrue(focusProjection.contains("focusProjectionRequiresLayoutRepair(receipt.output)"))
        assertTrue(focusProjection.contains("forceReconcile = true"))
        assertTrue(focusProjection.contains("authority = ObservationAuthority.EditorFocus"))
    }

@Test
fun `selection focus authority rejects background split events`() {
assertEquals(
EditorTabSyncListener.SelectionFocusAuthority.ActiveEditorSplit,
EditorTabSyncListener.SelectionFocusAuthority.decide(
selectionPath = "/repo/tasks/fpe.md",
activeWindowPath = "/repo/tasks/fpe.md",
),
)
assertEquals(
EditorTabSyncListener.SelectionFocusAuthority.BackgroundOrUnknownSplit,
EditorTabSyncListener.SelectionFocusAuthority.decide(
selectionPath = "/repo/tasks/left.md",
activeWindowPath = "/repo/tasks/fpe.md",
),
)
assertEquals(
EditorTabSyncListener.SelectionFocusAuthority.BackgroundOrUnknownSplit,
EditorTabSyncListener.SelectionFocusAuthority.decide(
selectionPath = "/repo/tasks/fpe.md",
activeWindowPath = null,
),
)
}

    @Test
    fun `editor snapshot leaves project root and controller work off the EDT`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val capture =
            source
                .substringAfter("private fun captureSurface(")
                .substringBefore("private fun resolveSurface(")
        val resolution =
            source
                .substringAfter("private fun resolveSurface(")
                .substringBefore("private fun requestFocusProjection(")

        assertFalse(capture.contains("TerminalUtil.resolveProject("))
        assertFalse(capture.contains("nearestAgentDocProjectRoot"))
        assertTrue(resolution.contains("TerminalUtil.resolveProject("))
        assertTrue(resolution.contains("nearestAgentDocProjectRoot"))
        assertTrue(source.contains("surfaceDeliveryExecutor.execute"))
        assertTrue(source.contains("resolveSurface(captured)"))
    }

    @Test
    fun `IDE activation republishes and settles the visible editor surface`() {
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                EditorTabSyncListener.ObservationAuthority.IdeActivation,
                remainingPasses = 1,
            ),
        )
        assertFalse(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                EditorTabSyncListener.ObservationAuthority.IdeActivation,
                remainingPasses = 0,
            ),
        )
    }

    @Test
    fun `markdown file open republishes the editor surface after the startup seed`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val fileOpenedMarker =
            "override fun fileOpened(source: FileEditorManager, file: VirtualFile)"

        assertTrue(source.contains(fileOpenedMarker))
        val fileOpened =
            source
                .substringAfter(fileOpenedMarker)
                .substringBefore("override fun fileClosed")
        assertTrue(fileOpened.contains("if (!AgentDocSessionFiles.isSessionDocument(file))"))
        assertTrue(fileOpened.contains("authority = ObservationAuthority.Layout"))
        assertTrue(fileOpened.contains("preferredFile = file"))
        assertTrue(fileOpened.contains("authority = ObservationAuthority.FileOpened"))
        assertFalse(fileOpened.contains("onEditorLayoutChanged(source.project)"))
    }

    @Test
    fun `focused and adjacent tabs lead the open document priority`() {
        assertEquals(
            listOf(
                "/repo/b.md",
                "/repo/a.md",
                "/repo/c.md",
                "/repo/d.md",
                "/repo/split.md",
            ),
            EditorTabSyncListener.SurfaceReport.prioritizeOpenDocuments(
                focusedFile = "/repo/b.md",
                nearbyTabs =
                    EditorTabSyncListener.SurfaceReport.tabsByProximity(
                        focusedFile = "/repo/b.md",
                        tabs =
                            listOf(
                                "/repo/a.md",
                                "/repo/b.md",
                                "/repo/c.md",
                                "/repo/d.md",
                            ),
                    ),
                visibleMdFiles = listOf("/repo/b.md", "/repo/split.md"),
                openMdFiles = listOf("/repo/d.md", "/repo/split.md"),
            ),
        )
    }

    @Test
    fun `structural layout detection uses one short event coalescing window`() {
        assertTrue(LayoutChangeDetector.STRUCTURAL_COALESCE_MS in 1L..75L)
    }

    @Test
    fun `layout changes retain selected intent only until the surface is captured`() {
        val listenerPath =
            listOf(
                    Paths.get(
                        "src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                )
                .first { Files.exists(it) }
        val source = Files.readString(listenerPath)
        val layoutChange =
            source
                .substringAfter("fun onEditorLayoutChanged(project: Project)")
                .substringBefore("override fun fileClosed")
        val report =
            source
                .substringAfter(
                    "private fun projectLatestSurfaceOnEditorThread(requestedGeneration: Long)"
                )
                .substringBefore("private fun captureSurface(")

        assertTrue(layoutChange.contains("latestSurfaceObservation.get()"))
        assertTrue(layoutChange.contains("it.preferredFile != null"))
        assertFalse(layoutChange.contains("delayMs"))
        assertTrue(report.contains("ApplicationManager.getApplication().invokeLater"))
        assertTrue(report.contains("SelectionProjectionSettling.shouldReproject"))
        assertTrue(report.contains("remainingSelectionPasses - 1"))
        assertTrue(report.contains("ObservationDeliveryOwnership.releaseForDelivery"))
        assertTrue(report.contains("ObservationDeliveryOwnership.retainAfterFailure"))
        assertFalse(report.contains("requestObservation("))
        assertFalse(report.contains("invokeAndWait"))
        assertFalse(report.contains("schedule("))
    }

    @Test
    fun `surface reporting uses the existing controller socket off the EDT`() {
        val listenerPath =
            listOf(
                    Paths.get(
                        "src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                )
                .first { Files.exists(it) }
        val reportBody =
            Files.readString(listenerPath)
                .substringAfter(
                    "private fun projectLatestSurfaceOnEditorThread(requestedGeneration: Long)"
                )
                .substringBefore("private fun captureSurface(")

        assertTrue(reportBody.contains("CpRouteClient.observeEditorSurface("))
        assertTrue(reportBody.contains("surfaceDeliveryExecutor.execute"))
        assertTrue(reportBody.contains("synchronized(lifecycleLock)"))
        assertTrue(reportBody.contains("if (closed)"))
        assertTrue(reportBody.contains("surfaceRoots.recordAttempts("))
        assertTrue(reportBody.contains("surfaceRoots.markPublished("))
        assertTrue(reportBody.contains("CpRouteClient.forgetEditorSurface("))
        assertTrue(reportBody.contains("surfaceRoots.markForgotten("))
        assertFalse(reportBody.contains("requestObservation("))
        assertFalse(reportBody.contains("NativeAdminControls.editorSurface"))
        assertFalse(reportBody.contains("editorSurfaceObserve("))
        assertFalse(reportBody.contains("syncHintFromReceipt("))
    }

    @Test
    fun `component focus republishes spanning surface and selection state`() {
        val listenerPath =
            listOf(
                    Paths.get(
                        "src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                )
                .first { Files.exists(it) }
        val focusBody =
            Files.readString(listenerPath)
                .substringAfter("fun onEditorFocusGained(project: Project, file: VirtualFile)")
                .substringBefore("fun onEditorLayoutChanged(project: Project)")

        assertTrue(focusBody.contains("requestFocusProjection(project, file)"))
        assertTrue(focusBody.contains("requestObservation("))
        assertTrue(focusBody.contains("preferredFile = file"))
        assertTrue(focusBody.contains("authority = ObservationAuthority.EditorFocus"))
    }

    @Test
    fun `component focus cannot supersede a pending document selection`() {
        assertFalse(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
            ),
        )
        assertFalse(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
            ),
        )
    }

    @Test
    fun `component focus publishes when no document selection is pending`() {
        assertTrue(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = null,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
            ),
        )
        assertTrue(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
            ),
        )
    }
}
