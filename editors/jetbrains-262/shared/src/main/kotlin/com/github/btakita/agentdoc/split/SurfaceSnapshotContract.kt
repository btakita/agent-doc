@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split

import com.intellij.platform.project.ProjectId
import com.intellij.platform.rpc.RemoteApiProviderService
import fleet.rpc.RemoteApi
import fleet.rpc.Rpc
import fleet.rpc.remoteApiDescriptor
import kotlinx.coroutines.flow.Flow
import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable

/**
 * Current snapshot schema. Schema 2 added measured window bounds. Schema 3 added the frontend's
 * frozen compatibility column vector; schema 4 adds the focused editor document proved by the
 * frontend component tree (GH #368). Schema 5 adds the frontend JVM's GH #339 activation receipt.
 * The hot-reloaded native library remains the live grouping authority over the bounds.
 */
const val SURFACE_SNAPSHOT_SCHEMA_VERSION: Int = 5
const val GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION: Int = 2
const val FRONTEND_LAYOUT_SURFACE_SNAPSHOT_SCHEMA_VERSION: Int = 3
const val FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION: Int = 4
const val ACTIVATION_RECEIPT_SURFACE_SNAPSHOT_SCHEMA_VERSION: Int = 5

/** The oldest snapshot schema a backend still accepts (a frontend that predates geometry). */
const val LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION: Int = 1

fun isSupportedSurfaceSnapshotSchema(version: Int): Boolean =
    version in LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION..SURFACE_SNAPSHOT_SCHEMA_VERSION
const val MAIN_SURFACE_PANE_ID: String = "root"

@Serializable
enum class SurfaceRole {
    MAIN,
    DETACHED,
}

/**
 * Frontend presentation support proven by the exact platform range of this artifact.
 *
 * The exact-262 internal mode is admitted only after the frontend runtime verifies every internal
 * editor/terminal shape it invokes. Any missing shape falls back to SNAPSHOT_ONLY.
 */
@Serializable
enum class FrontendPresentationCapability {
    SNAPSHOT_ONLY,
    EXACT_262_INTERNAL,
}

/** Exhaustive admission fence: adding a capability cannot silently turn terminal effects on. */
fun FrontendPresentationCapability.admitsTerminalPresentation(): Boolean = when (this) {
    FrontendPresentationCapability.SNAPSHOT_ONLY -> false
    FrontendPresentationCapability.EXACT_262_INTERNAL -> true
}

@Serializable
enum class FrontendPresentationKind {
    EMPTY,
    TERMINAL,
    PLACEHOLDER,
}

@Serializable
data class FrontendSurfaceIdentity(
    val clientId: String,
    val connectionGeneration: Long,
    val surfaceId: String,
    val surfaceGeneration: Long,
)

@Serializable
data class FrontendSurfacePresentation(
    val identity: FrontendSurfaceIdentity,
    val presentationRevision: Long,
    val kind: FrontendPresentationKind,
    val document: String? = null,
    val reason: String? = null,
    /** Controller-derived from the verified durable Bound receipt. */
    val viewSession: String? = null,
)

@Serializable
enum class FrontendProjectionStatus {
    APPLIED,
    STALE,
    FROZEN,
}

@Serializable
data class FrontendPresentationProjection(
    val status: FrontendProjectionStatus,
    val revision: Long,
    val retrySuggested: Boolean,
    val presentations: List<FrontendSurfacePresentation>,
)

@Serializable
enum class FrontendPresentationReceiptOutcome {
    APPLIED,
    REFUSED,
    STALE,
}

@Serializable
data class FrontendPresentationReceipt(
    val projectId: ProjectId,
    val identity: FrontendSurfaceIdentity,
    val presentationRevision: Long,
    val kind: FrontendPresentationKind,
    val document: String? = null,
    val viewSession: String? = null,
    val outcome: FrontendPresentationReceiptOutcome,
    val diagnostic: String? = null,
)

/**
 * One editor window's measured on-screen rectangle in the JetBrains Client (schema 2).
 *
 * This is observed state, not an order: the backend derives left-to-right columns from it with
 * the same grouping as the classic `LayoutDetector.buildColumnsFromSnapshots`.
 */
@Serializable
data class EditorWindowBounds(
    val x: Int,
    val y: Int,
    val width: Int,
    val height: Int,
)

@OptIn(ExperimentalSerializationApi::class)
@Serializable
data class EditorWindowSnapshot(
    val ordinal: Int,
    val selectedPath: String?,
    val openPaths: List<String>,
    val visiblePaths: List<String>,
    /**
     * Schema 2 measured bounds; null when the window was not showing or the peer predates
     * geometry. Never encoded while null: Fleet RPC decodes with `ignoreUnknownKeys = false`, so
     * a schema-1 backend must never see the key at all.
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val bounds: EditorWindowBounds? = null,
)

/** One frontend-authored editor column, in exact left-to-right order. */
@Serializable
data class FrontendLayoutColumn(val paths: List<String>)

@Serializable
@OptIn(ExperimentalSerializationApi::class)
data class FrontendSurface(
    val paneId: String,
    /** Frontend-owned incarnation; changes when the pane component is replaced or reopened. */
    val surfaceGeneration: Long,
    val role: SurfaceRole,
    val focused: Boolean,
    val windows: List<EditorWindowSnapshot>,
    /**
     * Schema 3 compatibility floor for a native library that cannot derive measured geometry.
     * A live native generation derives the authoritative vector from [windows] instead.
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val layoutColumns: List<FrontendLayoutColumn>? = null,
    /** Selected document of the editor window that owns permanent AWT focus. */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val focusedEditorPath: String? = null,
)

@Serializable
@OptIn(ExperimentalSerializationApi::class)
data class FrontendSurfaceSnapshot(
    val schemaVersion: Int = SURFACE_SNAPSHOT_SCHEMA_VERSION,
    val frontendInstanceId: String,
    val sequence: Long,
    val projectId: ProjectId,
    val complete: Boolean,
    val presentationCapability: FrontendPresentationCapability =
        FrontendPresentationCapability.SNAPSHOT_ONLY,
    val surfaces: List<FrontendSurface>,
    /**
     * Schema 5: the frontend JVM's `agent_doc_activation_receipt/v1` line (GH #339). The backend
     * logs it and, from R4, fences frontend-dependent work on it. Never encoded while null, so a
     * schema-4 backend never sees the key.
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val activationReceipt: String? = null,
)

/**
 * The same snapshot in an older schema the backend accepts. Schema 4 carries no activation
 * receipt; schema 2 carries bounds but no frontend column vector; schema 1 carries neither.
 */
fun FrontendSurfaceSnapshot.forSchema(version: Int): FrontendSurfaceSnapshot {
    require(isSupportedSurfaceSnapshotSchema(version)) { "unsupported surface snapshot schema $version" }
    return copy(
        schemaVersion = version,
        activationReceipt = if (version >= ACTIVATION_RECEIPT_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
            activationReceipt
        } else {
            null
        },
        surfaces = surfaces.map { surface ->
            surface.copy(
                windows = if (version >= GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
                    surface.windows
                } else {
                    surface.windows.map { it.copy(bounds = null) }
                },
                layoutColumns = if (version >= FRONTEND_LAYOUT_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
                    surface.layoutColumns
                } else {
                    null
                },
                focusedEditorPath = if (version >= FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
                    surface.focusedEditorPath
                } else {
                    null
                },
            )
        },
    )
}

@Serializable
data class SurfaceIngressLease(
    val leaseId: String,
    val connectionGeneration: Long,
)

@Serializable
enum class SurfaceIngressStatus {
    ACCEPTED,
    STALE_LEASE,
    STALE_SEQUENCE,
    INVALID_SCHEMA,
    INCOMPLETE,
    INVALID_MAIN_CARDINALITY,
    INVALID_SURFACE,
    PROJECT_MISMATCH,
    DELIVERY_FAILED,
}

@Serializable
data class SurfaceIngressAck(
    val status: SurfaceIngressStatus,
    val connectionGeneration: Long,
    val acceptedSequence: Long?,
    val diagnostic: String? = null,
    val presentation: FrontendPresentationProjection? = null,
)

@Rpc
interface SurfaceSnapshotRpcApi : RemoteApi<Unit> {
    companion object {
        suspend fun getInstance(): SurfaceSnapshotRpcApi =
            RemoteApiProviderService.resolve(remoteApiDescriptor<SurfaceSnapshotRpcApi>())
    }

    /**
     * The returned flow is the connection lease. Its backend `finally` retires the
     * generation when the frontend process, project, or RPC transport disappears.
     */
    suspend fun openIngress(
        projectId: ProjectId,
        frontendInstanceId: String,
    ): Flow<SurfaceIngressLease>

    suspend fun publishSnapshot(
        lease: SurfaceIngressLease,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceIngressAck

    suspend fun publishPresentationReceipt(
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck

    /**
     * The newest snapshot schema this backend accepts (GH #320 option B). Added with schema 2: a
     * backend that predates it fails the call, and the frontend then publishes schema 1 (no
     * geometry), so an old peer keeps working without ever decoding a key it does not know.
     */
    suspend fun supportedSnapshotSchemaVersion(): Int
}
