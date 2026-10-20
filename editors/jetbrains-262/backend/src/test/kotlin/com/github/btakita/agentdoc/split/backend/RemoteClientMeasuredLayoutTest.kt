package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.EditorDerivation
import com.github.btakita.agentdoc.split.EditorWindowBounds
import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.forSchema
import com.google.gson.JsonParser
import com.intellij.platform.project.ProjectId
import fleet.util.UID
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * GH #320 option B: Remote Dev pane order comes from the JetBrains Client's measured editor-window
 * bounds, grouped exactly like the classic `LayoutDetector.buildColumnsFromSnapshots`.
 *
 * GH #326: the native `client_measured_layout` derivation owns these rules (pinned by the Rust
 * `window_layout` tests with the same cases); the cases here pin the frozen Kotlin floor a native
 * library that cannot answer falls back to, and that a native answer is used when present.
 */
class RemoteClientMeasuredLayoutTest {
    private val projectId = ProjectId(UID.random())
    private val sessions = setOf("/repo/a.md", "/repo/b.md", "/repo/c.md")
    private val isSession: (String) -> Boolean = { it in sessions }

    @Test
    fun twoWindowsAtDifferentXPositionsAreEditorOrderedByGeometryNotListOrder() {
        // The client lists the right split first; only the measured x decides the order.
        val before = snapshot(
            window(0, "/repo/b.md", x = 640),
            window(1, "/repo/a.md", x = 0),
        )
        assertEquals(
            MeasuredMainLayout.Measured(listOf(listOf("/repo/a.md"), listOf("/repo/b.md")), "/repo/b.md"),
            deriveFrozen(before, isSession),
        )

        // The operator moves a.md to the opposite split (the GH #320 trace): same membership,
        // new geometry, so the measured order follows the move.
        val moved = snapshot(
            window(0, "/repo/b.md", x = 0),
            window(1, "/repo/a.md", x = 640),
        )
        assertEquals(
            listOf(listOf("/repo/b.md"), listOf("/repo/a.md")),
            (deriveFrozen(moved, isSession) as MeasuredMainLayout.Measured).columns,
        )
    }

    @Test
    fun stackedWindowsShareAColumnTopToBottom() {
        val stacked = snapshot(
            window(0, "/repo/b.md", x = 0, y = 500),
            window(1, "/repo/a.md", x = 4, y = 40),
            window(2, "/repo/c.md", x = 640, y = 40),
        )
        assertEquals(
            listOf(listOf("/repo/a.md", "/repo/b.md"), listOf("/repo/c.md")),
            (deriveFrozen(stacked, isSession) as MeasuredMainLayout.Measured).columns,
        )
    }

    /** A frontend that predates schema 2 (or one that negotiated schema 1) has no geometry. */
    @Test
    fun schemaOneSnapshotHasNoMeasuredOrder() {
        val legacy = snapshot(
            window(0, "/repo/a.md", x = 0),
            window(1, "/repo/b.md", x = 640),
        ).forSchema(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertEquals(
            MeasuredMainLayout.Unavailable("schema_1_without_geometry"),
            deriveFrozen(legacy, isSession),
        )
    }

    @Test
    fun unmeasuredOrSharedOriginWindowsNeverClaimEditorOrder() {
        assertEquals(
            MeasuredMainLayout.Unavailable("window_unmeasured"),
            deriveFrozen(
                snapshot(window(0, "/repo/a.md", x = 0), window(1, "/repo/b.md", bounds = null)),
                isSession,
            ),
        )
        assertEquals(
            MeasuredMainLayout.Unavailable("shared_origin"),
            deriveFrozen(
                snapshot(window(0, "/repo/a.md", x = 0), window(1, "/repo/b.md", x = 0)),
                isSession,
            ),
        )
    }

    @Test
    fun aSourceTabNextToSessionTabsHoldsTheLayoutWhileASourceOnlySplitContributesNothing() {
        val sourceTab = EditorWindowSnapshot(
            ordinal = 1,
            selectedPath = "/repo/Main.kt",
            openPaths = listOf("/repo/Main.kt", "/repo/b.md"),
            visiblePaths = listOf("/repo/Main.kt"),
            bounds = EditorWindowBounds(640, 40, 600, 800),
        )
        assertEquals(
            MeasuredMainLayout.Unavailable("split_shows_non_session_tab"),
            deriveFrozen(snapshot(window(0, "/repo/a.md", x = 0), sourceTab), isSession),
        )
        val sourceOnly = sourceTab.copy(openPaths = listOf("/repo/Main.kt"))
        assertEquals(
            listOf(listOf("/repo/a.md")),
            (deriveFrozen(snapshot(window(0, "/repo/a.md", x = 0), sourceOnly), isSession)
                as MeasuredMainLayout.Measured).columns,
        )
    }

    @Test
    fun detachedSurfacesNeverContributeMainColumns() {
        val main = FrontendSurface("root", 1, SurfaceRole.MAIN, true, listOf(window(0, "/repo/a.md", x = 0)))
        val detached = FrontendSurface(
            "detached-1",
            2,
            SurfaceRole.DETACHED,
            false,
            listOf(window(0, "/repo/b.md", x = 1800)),
        )
        val layout = deriveFrozen(
            FrontendSurfaceSnapshot(
                frontendInstanceId = "frontend-1",
                sequence = 3,
                projectId = projectId,
                complete = true,
                surfaces = listOf(main, detached),
            ),
            isSession,
        )
        assertEquals(listOf(listOf("/repo/a.md")), (layout as MeasuredMainLayout.Measured).columns)
    }

    /** The forwarded `editor_surface` payload labels measured order `editor` and owns no focus. */
    @Test
    fun forwardedSurfacePayloadIsEditorOrderedAndPreservesFocus() {
        val root = Files.createTempDirectory("rdgeometry-payload").toRealPath()
        Files.createDirectories(root.resolve(".agent-doc"))
        val a = root.resolve("a.md").toString()
        val b = root.resolve("b.md").toString()

        val forwarded = RemoteClientSurfacePayload.build(
            root.toString(),
            MeasuredMainLayout.Measured(listOf(listOf(b), listOf(a)), focused = a),
        )
        val payload = JsonParser.parseString(forwarded.surfaceJson).asJsonObject

        assertEquals(root.toString(), forwarded.projectRoot)
        assertEquals("editor", payload.get("column_order").asString)
        assertTrue(payload.get("preserve_focus").asBoolean)
        assertEquals(
            listOf(listOf(b), listOf(a)),
            payload.getAsJsonArray("columns").map { column ->
                column.asJsonObject.getAsJsonArray("files").map { it.asString }
            },
        )
    }

    @Test
    fun eachFrontendConnectionPublishesItsOwnEndpoint() {
        assertEquals("remote-client:client-a:7", remoteClientSurfaceId("client-a", 7))
        assertTrue(remoteClientSurfaceId("client-a", 7) != remoteClientSurfaceId("client-a", 8))
    }

    /** The frozen compatibility floor: no native generation answers. */
    private fun deriveFrozen(
        snapshot: FrontendSurfaceSnapshot,
        isSessionDocument: (String) -> Boolean,
    ) = RemoteClientMeasuredLayout.derive(snapshot, isSessionDocument, EditorDerivation.Unavailable)

    @Test
    fun nativeDerivationReceivesWindowEvidenceAndItsAnswerWins() {
        var asked: String? = null
        var evidence: String? = null
        val layout = RemoteClientMeasuredLayout.derive(
            snapshot(window(0, "/repo/b.md", x = 640), window(1, "/repo/a.md", x = 0)),
            isSession,
        ) { derivation, inputJson ->
            asked = derivation
            evidence = inputJson
            """{"outcome":"measured","columns":[["/repo/b.md"],["/repo/a.md"]],"focused":"/repo/a.md"}"""
        }
        assertEquals("client_measured_layout", asked)
        val window = JsonParser.parseString(evidence).asJsonObject.getAsJsonArray("windows")[0].asJsonObject
        assertEquals(0, window.get("ordinal").asInt)
        assertEquals("/repo/b.md", window.get("selected_session").asString)
        assertTrue(window.get("holds_session_document").asBoolean)
        assertEquals(640, window.get("x").asInt)
        assertEquals(
            MeasuredMainLayout.Measured(listOf(listOf("/repo/b.md"), listOf("/repo/a.md")), "/repo/a.md"),
            layout,
        )
    }

    @Test
    fun anUnreadableOrUnknownNativeAnswerFallsBackToTheFrozenFloor() {
        val split = snapshot(window(0, "/repo/b.md", x = 640), window(1, "/repo/a.md", x = 0))
        val expected = MeasuredMainLayout.Measured(listOf(listOf("/repo/a.md"), listOf("/repo/b.md")), "/repo/b.md")
        assertEquals(expected, RemoteClientMeasuredLayout.derive(split, isSession) { _, _ -> """{"outcome":"later"}""" })
        assertEquals(expected, RemoteClientMeasuredLayout.derive(split, isSession) { _, _ -> "not json" })
        assertEquals(
            MeasuredMainLayout.Unavailable("shared_origin"),
            RemoteClientMeasuredLayout.parseMeasuredLayout("""{"outcome":"unavailable","reason":"shared_origin"}"""),
        )
    }

    private fun window(
        ordinal: Int,
        path: String,
        x: Int = 0,
        y: Int = 40,
        bounds: EditorWindowBounds? = EditorWindowBounds(x, y, 600, 400),
    ) = EditorWindowSnapshot(ordinal, path, listOf(path), listOf(path), bounds)

    private fun snapshot(vararg windows: EditorWindowSnapshot) = FrontendSurfaceSnapshot(
        frontendInstanceId = "frontend-1",
        sequence = 1,
        projectId = projectId,
        complete = true,
        surfaces = listOf(FrontendSurface("root", 1, SurfaceRole.MAIN, true, windows.toList())),
    )
}
