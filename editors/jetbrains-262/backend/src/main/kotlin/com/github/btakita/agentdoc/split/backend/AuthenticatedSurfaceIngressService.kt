@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.MAIN_SURFACE_PANE_ID
import com.github.btakita.agentdoc.split.isSupportedSurfaceSnapshotSchema
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.openapi.Disposable
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import java.util.UUID
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

@Service(Service.Level.PROJECT)
internal class AuthenticatedSurfaceIngressService(project: Project) : Disposable {
    private val layoutForwarder = ControllerRemoteClientLayoutForwarder(project)
    private val state = AuthenticatedSurfaceIngressState(
        ControllerSurfaceSnapshotPublisher(project),
        layoutForwarder,
    )

    override fun dispose() {
        layoutForwarder.dispose()
    }

    val frontendSnapshotAvailable: StateFlow<Boolean> = state.frontendSnapshotAvailable

    suspend fun open(authenticatedClientId: String, frontendInstanceId: String): SurfaceIngressLease =
        state.open(authenticatedClientId, frontendInstanceId)

    suspend fun publish(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        snapshot: FrontendSurfaceSnapshot,
    ): SurfaceIngressAck = state.publish(authenticatedClientId, lease, snapshot)

    suspend fun retire(authenticatedClientId: String, lease: SurfaceIngressLease) =
        state.retire(authenticatedClientId, lease)

    suspend fun publishPresentationReceipt(
        authenticatedClientId: String,
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck = state.publishPresentationReceipt(authenticatedClientId, lease, receipt)

    companion object {
        fun getInstance(project: Project): AuthenticatedSurfaceIngressService = project.service()
    }
}

internal class AuthenticatedSurfaceIngressState(
    private val publisher: SurfaceSnapshotPublisher,
    private val layoutForwarder: RemoteClientLayoutForwarder = RemoteClientLayoutForwarder.Disabled,
) {
    private val generation = AtomicLong(0)
    private val mutex = Mutex()
    private val connections = mutableMapOf<String, ConnectionState>()
    private val mutableFrontendSnapshotAvailable = MutableStateFlow(false)
    val frontendSnapshotAvailable: StateFlow<Boolean> = mutableFrontendSnapshotAvailable.asStateFlow()

    suspend fun open(
        authenticatedClientId: String,
        frontendInstanceId: String,
    ): SurfaceIngressLease = mutex.withLock {
        connections.remove(authenticatedClientId)?.let { previous ->
            layoutForwarder.retire(previous.clientId, previous.lease.connectionGeneration)
            publisher.retire(previous.clientId, previous.lease.connectionGeneration)
        }
        val lease = SurfaceIngressLease(
            leaseId = UUID.randomUUID().toString(),
            connectionGeneration = generation.incrementAndGet(),
        )
        connections[authenticatedClientId] = ConnectionState(
            clientId = authenticatedClientId,
            frontendInstanceId = frontendInstanceId,
            lease = lease,
            acceptedSequence = 0,
            acceptedSurfaces = emptyMap(),
        )
        refreshFrontendSnapshotAvailability()
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
        connections[authenticatedClientId] = current.copy(
            acceptedSequence = snapshot.sequence,
            acceptedSurfaces = snapshot.surfaces.associate { it.paneId to it.surfaceGeneration },
        )
        refreshFrontendSnapshotAvailability()
        // GH #320 option B: the controller policy has now seen the complete snapshot, so its
        // measured main-window geometry may drive pane layout. The pane-layout surface fences
        // its own generation/sequence; the policy's projection status (frozen without terminal
        // capability on 263, or stale behind a pre-restart generation) is about detached views.
        layoutForwarder.forward(authenticatedClientId, lease.connectionGeneration, snapshot)
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
        layoutForwarder.retire(authenticatedClientId, lease.connectionGeneration)
        publisher.retire(authenticatedClientId, lease.connectionGeneration)
        refreshFrontendSnapshotAvailability()
    }

    private fun refreshFrontendSnapshotAvailability() {
        mutableFrontendSnapshotAvailable.value = connections.values.any { it.acceptedSequence > 0 }
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
    )
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
        return null
    }

    private fun failure(status: SurfaceIngressStatus, diagnostic: String) =
        SnapshotValidationFailure(status, diagnostic)
}
