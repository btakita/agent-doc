package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/**
 * The plugin no longer plans (`#jbsurfaceswap`) — focus-vs-sync, dedup, and the
 * retry ladder are the reactive graph's, exercised by
 * `agent-doc-editor-surface`. What is left here is the observation: the editor
 * has to report the surface it actually sees, and read the derived intent back.
 */
class EditorTabSyncListenerTest {
    private val gson = Gson()

    private fun surfaceJson(
        focusedFile: String,
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout? = null,
        forceReconcile: Boolean = false,
    ): String = gson.toJson(
        EditorTabSyncListener.SurfaceReport.buildSurface(
            focusedFile = focusedFile,
            visibleMdFiles = visibleMdFiles,
            editorLayout = editorLayout,
            forceReconcile = forceReconcile,
        )
    )

    @Test
    fun `selection event file wins over stale selected editor file`() {
        val visibleMdFiles = listOf(
            "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            "/repo/tasks/professional/sampleportal.md",
        )

        val activeFile = EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
            preferredActiveFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
            selectedEditorFile = "/repo/tasks/professional/sampleportal.md",
            visibleMdFiles = visibleMdFiles,
        )

        assertEquals("/repo/tasks/agent-doc/agent-doc-bugs2.md", activeFile)
    }

    @Test
    fun `selected editor file is used when no selection event file is supplied`() {
        val activeFile = EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
            preferredActiveFile = null,
            selectedEditorFile = "/repo/tasks/professional/sampleportal.md",
            visibleMdFiles = listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md"),
        )

        assertEquals("/repo/tasks/professional/sampleportal.md", activeFile)
    }

    @Test
    fun `first visible markdown file is the last resort active file`() {
        val activeFile = EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
            preferredActiveFile = "",
            selectedEditorFile = null,
            visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
        )

        assertEquals("/repo/a.md", activeFile)
    }

    @Test
    fun `no visible markdown means no active file to report`() {
        val activeFile = EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
            preferredActiveFile = null,
            selectedEditorFile = null,
            visibleMdFiles = emptyList(),
        )

        assertNull(activeFile)
    }

    @Test
    fun `observation reports the split layout the editor detected`() {
        val json = surfaceJson(
            focusedFile = "/repo/b.md",
            visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
            editorLayout = EditorLayout(
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
        val json = surfaceJson(
            focusedFile = "/repo/b.md",
            visibleMdFiles = listOf("/repo/b.md", "/repo/a.md"),
            editorLayout = EditorLayout(
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
        val json = surfaceJson(
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
        val json = surfaceJson(
            focusedFile = "/repo/a.md",
            visibleMdFiles = listOf("/repo/a.md", "/repo/a.md"),
            editorLayout = EditorLayout(
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
        val forced = JsonParser.parseString(
            surfaceJson("/repo/a.md", listOf("/repo/a.md"), forceReconcile = true)
        ).asJsonObject
        val unforced = JsonParser.parseString(
            surfaceJson("/repo/a.md", listOf("/repo/a.md"), forceReconcile = false)
        ).asJsonObject

        assertTrue(forced.get("force_reconcile").asBoolean)
        assertFalse(unforced.get("force_reconcile").asBoolean)
    }

    @Test
    fun `observation reports no layout_synced field for the controller to answer`() {
        val surface = JsonParser.parseString(
            surfaceJson("/repo/a.md", listOf("/repo/a.md"))
        ).asJsonObject

        assertFalse(surface.has("layout_synced"))
        assertFalse(surface.has("layoutSynced"))
    }

    @Test
    fun `document selection sends latest wins focus before layout reconciliation`() {
        val source = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get(
                    "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                ),
        )
        val selection = source.substringAfter(
            "override fun selectionChanged(event: FileEditorManagerEvent)",
        ).substringBefore("fun onEditorFocusGained")
        assertTrue(selection.contains("requestImmediateFocus(project, file)"))
        assertTrue(
            "focus must be submitted before layout detection and the debounced surface sync",
            selection.indexOf("requestImmediateFocus(project, file)") <
                selection.indexOf("captureSurface(project, file"),
        )

        val fastFocus = source.substringAfter(
            "private fun requestImmediateFocus(project: Project, file: VirtualFile)",
        ).substringBefore("private fun captureSurface")
        assertTrue(fastFocus.contains("focusGeneration.incrementAndGet()"))
        assertTrue(fastFocus.contains("NativeAdminControls.focusDocumentPane("))
        assertTrue(
            "the fast lane must not sleep or inherit the 100ms layout debounce",
            !fastFocus.contains("DEBOUNCE_MS") && !fastFocus.contains("schedule("),
        )
    }

    @Test
    fun `a derived sync intent produces the operator hint`() {
        val hint = EditorTabSyncListener.syncHintFromReceipt(
            """
            {"intent":{"kind":"sync","columns":[{"files":["/repo/a.md"]},{"files":["/repo/b.md"]}],
             "document":"/repo/b.md"},"idle":false,"outcome":"{}","error":null}
            """.trimIndent()
        )

        assertEquals("Sync: --col /repo/a.md --col /repo/b.md [focus: /repo/b.md]", hint)
        assertEquals("sync", EditorTabSyncListener.intentKindFromReceipt(
            """{"intent":{"kind":"sync","columns":[],"document":"/repo/b.md"},"idle":false}"""
        ))
    }

    @Test
    fun `focus and idle intents produce no hint`() {
        assertNull(
            EditorTabSyncListener.syncHintFromReceipt(
                """{"intent":{"kind":"focus","document":"/repo/b.md"},"idle":false}"""
            )
        )
        assertNull(
            EditorTabSyncListener.syncHintFromReceipt("""{"intent":{"kind":"idle"},"idle":true}""")
        )
    }

    @Test
    fun `an unusable receipt is reported as no intent instead of throwing`() {
        assertNull(EditorTabSyncListener.syncHintFromReceipt(null))
        assertNull(EditorTabSyncListener.syncHintFromReceipt(""))
        assertNull(EditorTabSyncListener.syncHintFromReceipt("not json"))
        assertNull(EditorTabSyncListener.intentKindFromReceipt("{}"))
    }
}
