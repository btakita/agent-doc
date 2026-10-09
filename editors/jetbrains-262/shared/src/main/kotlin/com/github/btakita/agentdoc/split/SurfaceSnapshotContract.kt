@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split

import com.intellij.platform.project.ProjectId
import com.intellij.platform.rpc.RemoteApiProviderService
import fleet.rpc.RemoteApi
import fleet.rpc.Rpc
import fleet.rpc.remoteApiDescriptor
import kotlinx.coroutines.flow.Flow
import kotlinx.serialization.Serializable

const val SURFACE_SNAPSHOT_SCHEMA_VERSION: Int = 1
const val MAIN_SURFACE_PANE_ID: String = "root"

@Serializable
enum class SurfaceRole {
    MAIN,
    DETACHED,
}

@Serializable
data class EditorWindowSnapshot(
    val ordinal: Int,
    val selectedPath: String?,
    val openPaths: List<String>,
    val visiblePaths: List<String>,
)

@Serializable
data class FrontendSurface(
    val paneId: String,
    /** Frontend-owned incarnation; changes when the pane component is replaced or reopened. */
    val surfaceGeneration: Long,
    val role: SurfaceRole,
    val focused: Boolean,
    val windows: List<EditorWindowSnapshot>,
)

@Serializable
data class FrontendSurfaceSnapshot(
    val schemaVersion: Int = SURFACE_SNAPSHOT_SCHEMA_VERSION,
    val frontendInstanceId: String,
    val sequence: Long,
    val projectId: ProjectId,
    val complete: Boolean,
    val surfaces: List<FrontendSurface>,
)

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
}
