package com.github.btakita.agentdoc.split

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
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

    /** Schema 5 adds the activation receipt; schemas 1 through 4 stay accepted. */
    @Test
    fun schemaFiveIsCurrentAndOlderSchemasStaySupported() {
        assertEquals(5, SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertTrue(isSupportedSurfaceSnapshotSchema(1))
        assertTrue(isSupportedSurfaceSnapshotSchema(2))
        assertTrue(isSupportedSurfaceSnapshotSchema(3))
        assertTrue(isSupportedSurfaceSnapshotSchema(4))
        assertTrue(isSupportedSurfaceSnapshotSchema(5))
        assertFalse(isSupportedSurfaceSnapshotSchema(0))
        assertFalse(isSupportedSurfaceSnapshotSchema(6))
    }

    /**
     * GH #339 R1: the frontend's activation receipt travels only in schema 5. A schema-4 backend
     * decodes with `ignoreUnknownKeys = false`, so the key must be absent below schema 5.
     */
    @Test
    fun activationReceiptTravelsOnlyInSchemaFive() {
        val receipt = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND).encode()
        val snapshot = FrontendSurfaceSnapshot(
            frontendInstanceId = "frontend-1",
            sequence = 1,
            projectId = ProjectId(UID.random()),
            complete = true,
            surfaces = listOf(FrontendSurface("root", 1, SurfaceRole.MAIN, true, emptyList())),
            activationReceipt = receipt,
        )
        val current = RPC_JSON.encodeToString(FrontendSurfaceSnapshot.serializer(), snapshot)
        assertEquals(
            receipt,
            RPC_JSON.decodeFromString(FrontendSurfaceSnapshot.serializer(), current).activationReceipt,
        )
        assertEquals(
            ActivationReceipt.ROLE_FRONTEND,
            ActivationReceipt.decode(snapshot.forSchema(5).activationReceipt!!).role,
        )
        val schemaFour = snapshot.forSchema(FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertNull(schemaFour.activationReceipt)
        val schemaFourJson = RPC_JSON.encodeToString(FrontendSurfaceSnapshot.serializer(), schemaFour)
        assertFalse(schemaFourJson.contains("activationReceipt"), schemaFourJson)
        assertFalse(
            RPC_JSON.encodeToString(FrontendSurfaceSnapshot.serializer(), snapshot.copy(activationReceipt = null))
                .contains("activationReceipt"),
        )
    }

    /** The receipt's shell identity is generated into this module, shared by both content modules. */
    @Test
    fun sharedModuleCarriesTheGeneratedShellIdentity() {
        assertTrue(Regex("[0-9a-f]{64}").matches(ShellIdentity.shellDigest), ShellIdentity.shellDigest)
        assertTrue(Regex("[0-9a-f]{64}").matches(ShellIdentity.implDigest), ShellIdentity.implDigest)
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
        val surface = FrontendSurface(
            "root", 1, SurfaceRole.MAIN, true, listOf(measured),
            layoutColumns = listOf(FrontendLayoutColumn(listOf("/repo/b.md"))),
            focusedEditorPath = "/repo/b.md",
        )

        val legacy = surface.copy(
            windows = surface.windows.map { it.copy(bounds = null) },
            layoutColumns = null,
            focusedEditorPath = null,
        )
        val legacyJson = RPC_JSON.encodeToString(FrontendSurface.serializer(), legacy)
        assertFalse(legacyJson.contains("bounds"), legacyJson)
        assertFalse(legacyJson.contains("layoutColumns"), legacyJson)
        assertFalse(legacyJson.contains("focusedEditorPath"), legacyJson)
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
            surfaces = listOf(
                FrontendSurface(
                    "root", 3, SurfaceRole.MAIN, true, listOf(window),
                    layoutColumns = listOf(FrontendLayoutColumn(listOf("/repo/a.md"))),
                    focusedEditorPath = "/repo/a.md",
                ),
            ),
        )
        assertEquals(SURFACE_SNAPSHOT_SCHEMA_VERSION, snapshot.schemaVersion)

        val legacy = snapshot.forSchema(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertEquals(1, legacy.schemaVersion)
        assertNull(legacy.surfaces.single().windows.single().bounds)
        assertNull(legacy.surfaces.single().layoutColumns)
        assertNull(legacy.surfaces.single().focusedEditorPath)
        assertEquals("/repo/a.md", legacy.surfaces.single().windows.single().selectedPath)
        assertEquals(snapshot.sequence, legacy.sequence)

        val current = snapshot.forSchema(SURFACE_SNAPSHOT_SCHEMA_VERSION)
        assertEquals(EditorWindowBounds(0, 40, 600, 800), current.surfaces.single().windows.single().bounds)
        assertEquals(listOf("/repo/a.md"), current.surfaces.single().layoutColumns!!.single().paths)
        assertEquals("/repo/a.md", current.surfaces.single().focusedEditorPath)
        val currentJson = RPC_JSON.encodeToString(FrontendSurfaceSnapshot.serializer(), current)
        assertTrue(currentJson.contains("\"layoutColumns\""), currentJson)
        assertTrue(currentJson.contains("\"focusedEditorPath\""), currentJson)
        val decodedCurrent = RPC_JSON.decodeFromString(FrontendSurfaceSnapshot.serializer(), currentJson)
        assertEquals(
            listOf("/repo/a.md"),
            decodedCurrent.surfaces.single().layoutColumns!!.single().paths,
        )
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
