@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.MAIN_SURFACE_PANE_ID
import com.github.btakita.agentdoc.split.isSupportedSurfaceSnapshotSchema
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import java.util.UUID
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/*
 * GH #339 R1: the implementation half of the Fleet RPC surface ingress. The shell's
 * `AuthenticatedSurfaceIngressService` (the platform-held project service the RPC provider calls)
 * asks the active implementation generation for a [SurfaceIngressFactory] and delegates every call
 * to the [SurfaceIngress] it creates. The lease state machine, the snapshot validator, the
 * controller publisher and the remote layout forwarder are all implementation.
 */

/** The `SurfaceIngressFactory` the implementation generation serves (`AgentDocImplGeneration`). */
internal class ControllerSurfaceIngressFactory : SurfaceIngressFactory {
    override fun create(project: Project): SurfaceIngress = ControllerSurfaceIngress(project)
}

/** One project's ingress: the controller publisher and layout forwarder behind the lease state. */
private class ControllerSurfaceIngress(project: Project) : SurfaceIngress {
    private val layoutForwarder = ControllerRemoteClientLayoutForwarder(project)
    private val state = AuthenticatedSurfaceIngressState(
        ControllerSurfaceSnapshotPublisher(),
        layoutForwarder,
    )

    override val frontendAuthorityActive: StateFlow<Boolean> get() = state.frontendAuthorityActive
    override val layoutAuthorityActive: StateFlow<Boolean> get() = state.layoutAuthorityActive
    override val layoutPublicationReadiness: StateFlow<LayoutPublicationReadiness>
        get() = state.layoutPublicationReadiness

    override suspend fun open(authenticatedClientId: String, frontendInstanceId: String): SurfaceIngressLease =
        state.open(authenticatedClientId, frontendInstanceId)

    override suspend fun publish(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceIngressAck = state.publish(authenticatedClientId, lease, snapshot)

    override suspend fun retire(authenticatedClientId: String, lease: SurfaceIngressLease) =
        state.retire(authenticatedClientId, lease)

    override suspend fun publishPresentationReceipt(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck = state.publishPresentationReceipt(authenticatedClientId, lease, receipt)

    override fun dispose() {
        layoutForwarder.dispose()
    }
}

internal class AuthenticatedSurfaceIngressState(
    private val publisher: SurfaceSnapshotPublisher,
    private val layoutForwarder: RemoteClientLayoutForwarder = RemoteClientLayoutForwarder.Disabled,
    /** GH #339 R1: called when a connection reports a new frontend activation receipt. */
    private val onFrontendReceipt: (clientId: String, connectionGeneration: Long, receipt: ActivationReceipt) -> Unit =
        ::logFrontendReceipt,
) {
    private val generation = AtomicLong(0)
    private val mutex = Mutex()
    private val connections = mutableMapOf<String, ConnectionState>()
    private val mutableFrontendAuthorityActive = MutableStateFlow(false)
    val frontendAuthorityActive: StateFlow<Boolean> = mutableFrontendAuthorityActive.asStateFlow()
    private val mutableLayoutAuthorityActive = MutableStateFlow(false)
    val layoutAuthorityActive: StateFlow<Boolean> = mutableLayoutAuthorityActive.asStateFlow()
    private val mutableLayoutPublicationReadiness = MutableStateFlow(LayoutPublicationReadiness.INACTIVE)
    val layoutPublicationReadiness: StateFlow<LayoutPublicationReadiness> =
        mutableLayoutPublicationReadiness.asStateFlow()

    suspend fun open(
        authenticatedClientId: String,
        frontendInstanceId: String,
    ): SurfaceIngressLease = mutex.withLock {
        connections.remove(authenticatedClientId)?.let { previous ->
            if (previous.layoutAuthority) {
                layoutForwarder.retire(previous.clientId, previous.lease.connectionGeneration)
            }
            publisher.retire(previous.clientId, previous.lease.connectionGeneration)
        }
        val lease = SurfaceIngressLease(
            leaseId = UUID.randomUUID().toString(),
            connectionGeneration = generation.incrementAndGet(),
        )
        val layoutAuthority = layoutForwarder.open(authenticatedClientId, lease.connectionGeneration)
        connections[authenticatedClientId] = ConnectionState(
            clientId = authenticatedClientId,
            frontendInstanceId = frontendInstanceId,
            lease = lease,
            acceptedSequence = 0,
            acceptedSurfaces = emptyMap(),
            layoutAuthority = layoutAuthority,
        )
        refreshFrontendAuthority()
        lease
    }

    suspend fun publish(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceIngressAck = mutex.withLock {
        val current = connections[authenticatedClientId]
        if (current == null || current.lease != lease || snapshot.frontendInstanceId != current.frontendInstanceId) {
            return@withLock rejected(SurfaceIngressStatus.STALE_LEASE, lease, "lease or frontend epoch is stale")
        }
        val validation = SurfaceSnapshotValidator.validate(snapshot)
        if (validation != null) {
            return@withLock rejected(validation.status, lease, validation.diagnostic)
        }
        if (snapshot.sequence <= current.acceptedSequence) {
            return@withLock rejected(
                SurfaceIngressStatus.STALE_SEQUENCE,
                lease,
                "sequence ${snapshot.sequence} is not newer than ${current.acceptedSequence}",
            )
        }
        val delivery = publisher.publish(
            clientId = authenticatedClientId,
            generation = lease.connectionGeneration,
            snapshot = snapshot,
        )
        if (!delivery.accepted) {
            return@withLock rejected(
                SurfaceIngressStatus.DELIVERY_FAILED,
                lease,
                delivery.diagnostic ?: "controller delivery failed",
            )
        }
        val receipt = ActivationReceipt.decodeOrNull(snapshot.activationReceipt)
        connections[authenticatedClientId] = current.copy(
            acceptedSequence = snapshot.sequence,
            acceptedSurfaces = snapshot.surfaces.associate { it.paneId to it.surfaceGeneration },
            activationReceipt = receipt ?: current.activationReceipt,
        )
        if (receipt != null && receipt != current.activationReceipt) {
            onFrontendReceipt(authenticatedClientId, lease.connectionGeneration, receipt)
        }
        refreshFrontendAuthority()
        // GH #320 option B: the controller policy has now seen the complete snapshot, so its
        // measured main-window geometry may drive pane layout. The pane-layout surface fences
        // its own generation/sequence; the policy's projection status (frozen without terminal
        // capability on 263, or stale behind a pre-restart generation) is about detached views.
        if (current.layoutAuthority) {
            layoutForwarder.forward(authenticatedClientId, lease.connectionGeneration, snapshot)
        }
        SurfaceIngressAck(
            status = SurfaceIngressStatus.ACCEPTED,
            connectionGeneration = lease.connectionGeneration,
            acceptedSequence = snapshot.sequence,
            presentation = delivery.presentation,
        )
    }

    suspend fun publishPresentationReceipt(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck = mutex.withLock {
        val current = connections[authenticatedClientId]
        if (current == null || current.lease != lease) {
            return@withLock rejected(SurfaceIngressStatus.STALE_LEASE, lease, "lease is stale")
        }
        val expectedClientId = "jetbrains-rd:$authenticatedClientId"
        if (receipt.identity.clientId != expectedClientId ||
            receipt.identity.connectionGeneration != lease.connectionGeneration ||
            receipt.presentationRevision != current.acceptedSequence ||
            current.acceptedSurfaces[receipt.identity.surfaceId] != receipt.identity.surfaceGeneration
        ) {
            return@withLock rejected(
                SurfaceIngressStatus.STALE_SEQUENCE,
                lease,
                "presentation receipt identity or revision is stale",
            )
        }
        val delivery = publisher.publishPresentationReceipt(
            clientId = authenticatedClientId,
            generation = lease.connectionGeneration,
            receipt = receipt,
        )
        if (!delivery.accepted) {
            return@withLock rejected(
                SurfaceIngressStatus.DELIVERY_FAILED,
                lease,
                delivery.diagnostic ?: "controller receipt delivery failed",
            )
        }
        SurfaceIngressAck(
            status = SurfaceIngressStatus.ACCEPTED,
            connectionGeneration = lease.connectionGeneration,
            acceptedSequence = receipt.presentationRevision,
        )
    }

    suspend fun retire(authenticatedClientId: String, lease: SurfaceIngressLease) = mutex.withLock {
        val current = connections[authenticatedClientId]
        if (current?.lease != lease) return@withLock
        connections.remove(authenticatedClientId)
        if (current.layoutAuthority) {
            layoutForwarder.retire(authenticatedClientId, lease.connectionGeneration)
        }
        publisher.retire(authenticatedClientId, lease.connectionGeneration)
        refreshFrontendAuthority()
    }

    /** The latest frontend activation receipt each connected client reported (schema 5). */
    suspend fun frontendActivationReceipts(): Map<String, ActivationReceipt> = mutex.withLock {
        connections.values.mapNotNull { connection ->
            connection.activationReceipt?.let { connection.clientId to it }
        }.toMap()
    }

    private fun refreshFrontendAuthority() {
        mutableFrontendAuthorityActive.value = connections.isNotEmpty()
        mutableLayoutAuthorityActive.value = connections.values.any { it.layoutAuthority }
        val layoutConnections = connections.values.filter { it.layoutAuthority }
        mutableLayoutPublicationReadiness.value = when {
            layoutConnections.isEmpty() -> LayoutPublicationReadiness.INACTIVE
            layoutConnections.any { it.acceptedSequence == 0L } -> LayoutPublicationReadiness.CONNECTED_BLOCKED
            else -> LayoutPublicationReadiness.ACCEPTED
        }
    }

    private fun rejected(
        status: SurfaceIngressStatus,
        lease: SurfaceIngressLease,
        diagnostic: String,
    ): SurfaceIngressAck = SurfaceIngressAck(
        status = status,
        connectionGeneration = lease.connectionGeneration,
        acceptedSequence = null,
        diagnostic = diagnostic,
    )

    private data class ConnectionState(
        val clientId: String,
        val frontendInstanceId: String,
        val lease: SurfaceIngressLease,
        val acceptedSequence: Long,
        val acceptedSurfaces: Map<String, Long>,
        val layoutAuthority: Boolean,
        val activationReceipt: ActivationReceipt? = null,
    )

    private companion object {
        val LOG = Logger.getInstance(AuthenticatedSurfaceIngressState::class.java)

        fun logFrontendReceipt(clientId: String, connectionGeneration: Long, receipt: ActivationReceipt) {
            LOG.info("[shell] frontend client=$clientId connection_generation=$connectionGeneration ${receipt.encode()}")
        }
    }
}

internal data class SnapshotValidationFailure(
    val status: SurfaceIngressStatus,
    val diagnostic: String,
)

internal object SurfaceSnapshotValidator {
    fun validate(snapshot: FrontendSurfaceSnapshot): SnapshotValidationFailure? {
        // GH #320 option B: schema 2 adds measured window bounds. A schema-1 frontend (no
        // geometry) stays valid; its main layout then keeps the remembered-order fallback.
        if (!isSupportedSurfaceSnapshotSchema(snapshot.schemaVersion)) {
            return failure(SurfaceIngressStatus.INVALID_SCHEMA, "unsupported schema ${snapshot.schemaVersion}")
        }
        if (!snapshot.complete) {
            return failure(SurfaceIngressStatus.INCOMPLETE, "frontend snapshot is not complete")
        }
        val main = snapshot.surfaces.filter { it.role == SurfaceRole.MAIN }
        if (main.size != 1 || main.single().paneId != MAIN_SURFACE_PANE_ID) {
            return failure(
                SurfaceIngressStatus.INVALID_MAIN_CARDINALITY,
                "expected exactly one root main surface, found ${main.map(FrontendSurface::paneId)}",
            )
        }
        if (snapshot.surfaces.map(FrontendSurface::paneId).any(String::isBlank) ||
            snapshot.surfaces.map(FrontendSurface::paneId).distinct().size != snapshot.surfaces.size
        ) {
            return failure(SurfaceIngressStatus.INVALID_SURFACE, "pane ids must be non-blank and unique")
        }
        if (snapshot.surfaces.any { it.surfaceGeneration <= 0 }) {
            return failure(SurfaceIngressStatus.INVALID_SURFACE, "surface generations must be positive")
        }
        if (snapshot.surfaces.any { surface ->
                (surface.paneId == MAIN_SURFACE_PANE_ID) != (surface.role == SurfaceRole.MAIN)
            }
        ) {
            return failure(SurfaceIngressStatus.INVALID_SURFACE, "root/main role mismatch")
        }
        for (surface in snapshot.surfaces) {
            val focusedEditorPath = surface.focusedEditorPath ?: continue
            if (
                snapshot.schemaVersion < FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION ||
                    focusedEditorPath.isBlank() ||
                    !surface.focused ||
                    surface.windows.none { it.selectedPath == focusedEditorPath }
            ) {
                return failure(
                    SurfaceIngressStatus.INVALID_SURFACE,
                    "focused editor must be selected in the active surface",
                )
            }
        }
        return null
    }

    private fun failure(status: SurfaceIngressStatus, diagnostic: String) =
        SnapshotValidationFailure(status, diagnostic)
}
