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
 * sees and separately submits the focused document to that document's own controller so
 * cross-project splits get an immediate pane handoff.
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
    fun `focus dispatch is generation fenced and requires the active project window`() {
        assertTrue(
            EditorTabSyncListener.shouldDispatchFocus(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldDispatchFocus(
                requestedGeneration = 6,
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldDispatchFocus(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = false,
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
    fun `transient opposite editor focus cannot replace a pending document selection`() {
        assertFalse(
            EditorTabSyncListener.ObservationProjection.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                currentFile = "/repo/left-next.md",
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.ComponentFocus,
                incomingFile = "/repo/right.md",
            ),
        )
    }

    @Test
    fun `a later component focus replaces a completed surface projection`() {
        assertTrue(
            EditorTabSyncListener.ObservationProjection.shouldReplace(
                currentAuthority = null,
                currentFile = null,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.ComponentFocus,
                incomingFile = "/repo/right.md",
            ),
        )
    }

    @Test
    fun `captured selection releases precedence while controller delivery is in flight`() {
        val captured = Any()
        val newerFocus = Any()
        val slot = AtomicReference<Any?>(captured)

        assertTrue(
            EditorTabSyncListener.ObservationDeliveryOwnership.releaseForDelivery(
                slot,
                captured,
            ),
        )
        assertTrue(slot.compareAndSet(null, newerFocus))
        assertFalse(
            EditorTabSyncListener.ObservationDeliveryOwnership.retainAfterFailure(
                slot,
                captured,
            ),
        )
        assertEquals(newerFocus, slot.get())
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
    fun `newer document selection replaces every older observation authority`() {
        assertTrue(
            EditorTabSyncListener.ObservationProjection.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.ComponentFocus,
                currentFile = "/repo/right.md",
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                incomingFile = "/repo/left-next.md",
            ),
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
        assertFalse(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.Layout,
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
    fun `document selection publishes layout and immediate document-root focus`() {
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
        assertTrue(selection.contains("requestImmediateFocus(project, file)"))
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
        assertTrue(focusGained.contains("requestImmediateFocus(project, file)"))
        assertTrue(focusGained.contains("requestObservation("))
        assertFalse(focusGained.contains("delayMs"))

        val immediateFocus =
            source
                .substringAfter("private fun requestImmediateFocus(project: Project, file: VirtualFile)")
                .substringBefore("private fun shutdown()")
        assertTrue(immediateFocus.contains("TerminalUtil.resolveProject(project, file)"))
        assertTrue(immediateFocus.contains("CpRouteClient.submitFocusDocumentPane("))
        assertTrue(immediateFocus.contains("TmuxPaneFocusSync.recordEditorFocusIntent("))
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
        assertTrue(fileOpened.contains("if (!file.name.endsWith(\".md\")) return"))
        assertTrue(fileOpened.contains("onEditorLayoutChanged(source.project)"))
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
        assertFalse(reportBody.contains("requestObservation("))
        assertFalse(reportBody.contains("NativeAdminControls.editorSurface"))
        assertFalse(reportBody.contains("editorSurfaceObserve("))
        assertFalse(reportBody.contains("syncHintFromReceipt("))
    }
}
