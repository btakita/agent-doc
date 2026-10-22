package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.EditorDerivation
import com.github.btakita.agentdoc.split.EditorWindowBounds
import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendLayoutColumn
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.forSchema
import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.platform.project.ProjectId
import fleet.util.UID
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * GH #320/#343/#346: schema-3 Remote Dev pane order comes from the hot-reloaded native
 * `client_measured_layout` derivation over JetBrains Client geometry. The exact frontend
 * `layoutColumns` vector remains only as the compatibility floor for an older native library.
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
    fun schemaThreeFrontendColumnVectorIsTheFrozenFallbackOverConflictingBounds() {
        val snapshot = FrontendSurfaceSnapshot(
            frontendInstanceId = "frontend-1",
            sequence = 1,
            projectId = projectId,
            complete = true,
            surfaces = listOf(
                FrontendSurface(
                    "root", 1, SurfaceRole.MAIN, true,
                    windows = listOf(
                        window(0, "/repo/a.md", x = 0),
                        window(1, "/repo/b.md", x = 640),
                    ),
                    layoutColumns = listOf(
                        FrontendLayoutColumn(listOf("/repo/b.md")),
                        FrontendLayoutColumn(listOf("/repo/a.md")),
                    ),
                ),
            ),
        )
        assertEquals(
            listOf(listOf("/repo/b.md"), listOf("/repo/a.md")),
            (RemoteClientMeasuredLayout.derive(snapshot, isSession, EditorDerivation.Unavailable)
                as MeasuredMainLayout.Measured).columns,
        )
    }

    @Test
    fun schemaThreeStillAsksNativeAndItsGroupingWinsOverTheFrozenVector() {
        val snapshot = FrontendSurfaceSnapshot(
            frontendInstanceId = "frontend-1",
            sequence = 1,
            projectId = projectId,
            complete = true,
            surfaces = listOf(
                FrontendSurface(
                    "root", 1, SurfaceRole.MAIN, true,
                    windows = listOf(
                        window(0, "/repo/a.md", x = 0),
                        window(1, "/repo/b.md", x = 640),
                    ),
                    layoutColumns = listOf(
                        FrontendLayoutColumn(listOf("/repo/b.md")),
                        FrontendLayoutColumn(listOf("/repo/a.md")),
                    ),
                ),
            ),
        )
        var asked: String? = null
        var evidence: JsonObject? = null
        val layout = RemoteClientMeasuredLayout.derive(snapshot, isSession) { derivation, inputJson ->
            asked = derivation
            evidence = JsonParser.parseString(inputJson).asJsonObject
            """{"outcome":"measured","columns":[["/repo/a.md"],["/repo/b.md"]],"focused":"/repo/a.md"}"""
        }

        assertEquals("client_measured_layout", asked)
        assertEquals(0, evidence!!.getAsJsonArray("windows")[0].asJsonObject.get("x").asInt)
        assertEquals(
            listOf(listOf("/repo/a.md"), listOf("/repo/b.md")),
            (layout as MeasuredMainLayout.Measured).columns,
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
        val main = FrontendSurface(
            "root",
            1,
            SurfaceRole.MAIN,
            true,
            listOf(window(0, "/repo/a.md", x = 0)),
            layoutColumns = listOf(FrontendLayoutColumn(listOf("/repo/a.md"))),
        )
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
            EditorDerivation.Unavailable,
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

    private fun surfaceParityCases(): List<JsonObject> {
        val stream = javaClass.classLoader.getResourceAsStream("editor-derivations/remote_client_surface.json")
            ?: error("GH #326 remote client surface parity fixture not on the test classpath")
        val raw = stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        return JsonParser.parseString(raw).asJsonObject.getAsJsonArray("cases").map { it.asJsonObject }
    }

    /**
     * The fixture omits fields the way an older plugin generation would (every input is optional on
     * the native side); the forwarder always fills them, so overlay the case on a complete one.
     */
    private fun surfaceEvidenceOf(case: JsonObject): RemoteClientSurfacePayload.Evidence {
        val complete = JsonParser.parseString("""{"focused":"","columns":[]}""").asJsonObject
        for ((key, value) in case.getAsJsonObject("input").entrySet()) {
            complete.add(key, value)
        }
        return Gson().fromJson(complete, RemoteClientSurfacePayload.Evidence::class.java)
    }

    private fun wire(forwarded: ForwardedRemoteSurface): JsonObject =
        JsonObject().apply {
            addProperty("project_root", forwarded.projectRoot)
            add("surface", JsonParser.parseString(forwarded.surfaceJson))
        }

    @Test
    fun gh326FrozenRemoteClientSurfaceFloorAnswersEverySharedParityCase() {
        // The same file pins the native derivation in Rust
        // (gh326_remote_client_surface_answers_the_shared_parity_cases).
        val cases = surfaceParityCases()
        assertTrue(cases.size >= 10, "the parity file lost cases")
        for (case in cases) {
            val name = case.get("name").asString
            val roots = case.getAsJsonObject("nearest_roots")?.entrySet()?.associate { it.key to it.value.asString }.orEmpty()
            val forwarded = RemoteClientSurfacePayload.frozen(surfaceEvidenceOf(case)) { file -> roots[file] }
            assertEquals(case.getAsJsonObject("expected"), wire(forwarded), name)
        }
    }

    @Test
    fun gh326RemoteClientSurfaceEvidenceIsTheNativeWireShape() {
        val layout = MeasuredMainLayout.Measured(listOf(listOf("/repo/src/app/b.md"), listOf("/repo/src/app/a.md")), "/repo/src/app/a.md")
        val evidence = RemoteClientSurfacePayload.evidence("/repo", layout) { base, focused ->
            assertEquals("/repo", base)
            assertEquals("/repo/src/app/a.md", focused)
            "/repo/src/app"
        }
        assertEquals(
            JsonParser.parseString(
                """{"base_path":"/repo","fallback_root":"/repo/src/app","focused":"/repo/src/app/a.md",""" +
                    """"columns":[["/repo/src/app/b.md"],["/repo/src/app/a.md"]]}""",
            ),
            JsonParser.parseString(Gson().toJson(evidence)),
        )
    }

    @Test
    fun gh326NativeRemoteClientSurfaceAnswerWinsAndKeepsFieldsTheFloorDoesNotKnow() {
        val root = Files.createTempDirectory("rdgeometry-native").toRealPath().toString()
        val layout = MeasuredMainLayout.Measured(listOf(listOf("$root/a.md")), "$root/a.md")
        var asked: String? = null
        var input: JsonObject? = null
        val forwarded = RemoteClientSurfacePayload.build(root, layout) { derivation, inputJson ->
            asked = derivation
            input = JsonParser.parseString(inputJson).asJsonObject
            """{"project_root":"/native/root","surface":{"focused":"$root/a.md","columns":[],"later_field":7}}"""
        }
        assertEquals("remote_client_surface", asked)
        assertEquals("$root/a.md", input!!.get("focused").asString)
        assertEquals("/native/root", forwarded.projectRoot)
        assertEquals(7, JsonParser.parseString(forwarded.surfaceJson).asJsonObject.get("later_field").asInt)
    }

    @Test
    fun gh326AnUnreadableNativeRemoteClientSurfaceFallsBackToTheFrozenFloor() {
        val root = Files.createTempDirectory("rdgeometry-unreadable").toRealPath().toString()
        val layout = MeasuredMainLayout.Measured(listOf(listOf("$root/b.md"), listOf("$root/a.md")), "$root/a.md")
        val frozen = RemoteClientSurfacePayload.build(root, layout, EditorDerivation.Unavailable)
        for (answer in listOf("not json", """{"surface":{}}""", """{"project_root":"/x","surface":[]}""")) {
            assertEquals(frozen, RemoteClientSurfacePayload.build(root, layout) { _, _ -> answer }, answer)
        }
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
        schemaVersion = GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION,
        frontendInstanceId = "frontend-1",
        sequence = 1,
        projectId = projectId,
        complete = true,
        surfaces = listOf(FrontendSurface("root", 1, SurfaceRole.MAIN, true, windows.toList())),
    )
}
