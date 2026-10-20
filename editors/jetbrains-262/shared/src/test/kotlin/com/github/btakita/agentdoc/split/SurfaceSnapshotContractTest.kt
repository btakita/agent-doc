package com.github.btakita.agentdoc.split

import com.intellij.platform.project.ProjectId
import fleet.util.UID
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

class SurfaceSnapshotContractTest {
    @Test
    fun rootPaneIsTheOnlyMainRoleToken() {
        assertEquals("root", MAIN_SURFACE_PANE_ID)
    }

    @Test
    fun terminalCapabilityAdmissionIsExplicitAndExhaustive() {
        assertEquals(
            listOf(
                FrontendPresentationCapability.SNAPSHOT_ONLY,
                FrontendPresentationCapability.EXACT_262_INTERNAL,
            ),
            FrontendPresentationCapability.entries,
        )
        assertFalse(FrontendPresentationCapability.SNAPSHOT_ONLY.admitsTerminalPresentation())
        assertTrue(FrontendPresentationCapability.EXACT_262_INTERNAL.admitsTerminalPresentation())
    }

    /** GH #320 option B: schema 2 adds geometry; schema 1 stays accepted for an old frontend. */
    @Test
    fun schemaTwoIsCurrentAndSchemaOneStaysSupported() {
        assertEquals(2, SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertTrue(isSupportedSurfaceSnapshotSchema(1))
        assertTrue(isSupportedSurfaceSnapshotSchema(2))
        assertFalse(isSupportedSurfaceSnapshotSchema(0))
        assertFalse(isSupportedSurfaceSnapshotSchema(3))
    }

    /**
     * Fleet RPC decodes with `ignoreUnknownKeys = false` and `encodeDefaults = true`. A schema-1
     * window (bounds dropped) must encode to exactly the pre-geometry shape, so a backend that
     * predates schema 2 decodes it; a measured window carries its bounds.
     */
    @Test
    fun schemaOneWindowEncodesWithoutTheGeometryKeyUnderRpcJson() {
        val measured = EditorWindowSnapshot(
            ordinal = 1,
            selectedPath = "/repo/b.md",
            openPaths = listOf("/repo/b.md"),
            visiblePaths = listOf("/repo/b.md"),
            bounds = EditorWindowBounds(x = 640, y = 40, width = 600, height = 800),
        )
        val surface = FrontendSurface("root", 1, SurfaceRole.MAIN, true, listOf(measured))

        val legacy = surface.copy(windows = surface.windows.map { it.copy(bounds = null) })
        val legacyJson = RPC_JSON.encodeToString(FrontendSurface.serializer(), legacy)
        assertFalse(legacyJson.contains("bounds"), legacyJson)
        val decodedByOldPeer = RPC_JSON.decodeFromString(LegacySurface.serializer(), legacyJson)
        assertEquals(listOf("/repo/b.md"), decodedByOldPeer.windows.single().visiblePaths)

        val currentJson = RPC_JSON.encodeToString(FrontendSurface.serializer(), surface)
        assertEquals(
            EditorWindowBounds(640, 40, 600, 800),
            RPC_JSON.decodeFromString(FrontendSurface.serializer(), currentJson).windows.single().bounds,
        )
    }

    @Test
    fun forSchemaOneDropsGeometryAndKeepsEverythingElse() {
        val window = EditorWindowSnapshot(
            ordinal = 0,
            selectedPath = "/repo/a.md",
            openPaths = listOf("/repo/a.md"),
            visiblePaths = listOf("/repo/a.md"),
            bounds = EditorWindowBounds(0, 40, 600, 800),
        )
        val snapshot = FrontendSurfaceSnapshot(
            frontendInstanceId = "frontend-1",
            sequence = 4,
            projectId = ProjectId(UID.random()),
            complete = true,
            surfaces = listOf(FrontendSurface("root", 3, SurfaceRole.MAIN, true, listOf(window))),
        )
        assertEquals(SURFACE_SNAPSHOT_SCHEMA_VERSION, snapshot.schemaVersion)

        val legacy = snapshot.forSchema(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertEquals(1, legacy.schemaVersion)
        assertNull(legacy.surfaces.single().windows.single().bounds)
        assertEquals("/repo/a.md", legacy.surfaces.single().windows.single().selectedPath)
        assertEquals(snapshot.sequence, legacy.sequence)

        val current = snapshot.forSchema(SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertEquals(EditorWindowBounds(0, 40, 600, 800), current.surfaces.single().windows.single().bounds)
    }

    @Serializable
    private data class LegacyWindow(
        val ordinal: Int,
        val selectedPath: String?,
        val openPaths: List<String>,
        val visiblePaths: List<String>,
    )

    @Serializable
    private data class LegacySurface(
        val paneId: String,
        val surfaceGeneration: Long,
        val role: SurfaceRole,
        val focused: Boolean,
        val windows: List<LegacyWindow>,
    )

    private companion object {
        /** The `fleet.rpc.core.RpcJson` settings (IU-263.6800.24). */
        val RPC_JSON = Json {
            classDiscriminator = "type"
            allowStructuredMapKeys = true
            ignoreUnknownKeys = false
            encodeDefaults = true
        }
    }
}
