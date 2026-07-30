package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The plugin no longer plans (`#jbsurfaceswap`) — focus-vs-sync, dedup, and the retry ladder are
 * the reactive graph's, exercised by `agent-doc-editor-surface`. What is left here is the
 * observation: the editor has to enqueue the surface it actually sees without waiting for
 * controller probes or tmux consequences.
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
    fun `document selection publishes one latest-wins surface observation`() {
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
        assertFalse(selection.contains("requestImmediateFocus"))
        assertFalse(selection.contains("CpRouteClient"))
        assertTrue(selection.contains("forceReconcile = false"))
        assertFalse(
            "ordinary tab focus must not force a competing full layout reconcile",
            selection.contains("forceReconcile = true"),
        )

        val focusGained =
            source
                .substringAfter("fun onEditorFocusGained(project: Project, file: VirtualFile)")
                .substringBefore("fun onEditorLayoutChanged")
        assertTrue(focusGained.contains("requestObservation("))
        assertTrue(focusGained.contains("delayMs = 0L"))
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
    fun `automatic layout observations use short coalescing windows`() {
        assertTrue(EditorTabSyncListener.SURFACE_COALESCE_MS in 1L..50L)
        assertTrue(LayoutChangeDetector.STRUCTURAL_COALESCE_MS in 1L..75L)
    }

    @Test
    fun `layout changes retain selected document intent until the projection is accepted`() {
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
                .substringAfter("private fun reportLatestSurface()")
                .substringBefore("private fun captureSurfaceOnEditorThread")

        assertTrue(layoutChange.contains("latestSurfaceObservation.get()"))
        assertTrue(layoutChange.contains("it.preferredFile != null"))
        assertTrue(layoutChange.contains("SURFACE_COALESCE_MS"))
        assertTrue(report.contains("compareAndSet(observation, null)"))
    }

    @Test
    fun `surface reporting enqueues without waiting for a controller receipt`() {
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
                .substringAfter("private fun reportLatestSurface()")
                .substringBefore("private fun captureSurfaceOnEditorThread")

        assertTrue(reportBody.contains("NativeAdminControls.editorSurfaceEnqueue("))
        assertTrue(reportBody.contains("requestObservation(observation, SURFACE_COALESCE_MS)"))
        assertFalse(reportBody.contains("editorSurfaceObserve("))
        assertFalse(reportBody.contains("syncHintFromReceipt("))
    }
}
