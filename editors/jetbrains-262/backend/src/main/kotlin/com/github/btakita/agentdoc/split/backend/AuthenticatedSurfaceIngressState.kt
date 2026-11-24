@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.github.btakita.agentdoc.shell.ShellGenerationHost
import com.github.btakita.agentdoc.split.ACTIVATION_RECEIPT_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.MAIN_SURFACE_PANE_ID
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.isSupportedSurfaceSnapshotSchema
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.AgentDocNotices
import com.github.btakita.agentdoc.pluginVersion
import com.intellij.notification.NotificationGroupManager
import com.intellij.openapi.application.PathManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import java.nio.file.Path
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.TimeUnit
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

    override fun currentFrontendRelease() =
        FrontendImplementationReleaseSource.current()?.takeIf { release ->
            runCatching { BackendFrontendReleaseActivation.activate(release) }
                .onFailure { LOG.warn("[frontend-impl] backend peer activation refused", it) }
                .isSuccess
        }

    private companion object {
        val LOG = Logger.getInstance(ControllerSurfaceIngressFactory::class.java)
    }
}

/** GH #435: tell the operator, in the Client UI, that the Client's frontend is a different build. */
private fun notifyFrontendPluginMismatch(project: Project, mismatch: FrontendPluginMismatch) {
    if (project.isDisposed) return
    val notification = NotificationGroupManager.getInstance()
        .getNotificationGroup("Agent Doc")
        .createNotification(
            if (mismatch.unansweredNegotiation) {
                "Agent Doc: the JetBrains Client frontend publishes layout without geometry"
            } else {
                "Agent Doc: the JetBrains Client frontend is not the backend's plugin ${pluginVersion()}"
            },
            "${mismatch.detail}. " +
                (if (mismatch.unansweredNegotiation) "Layout sync needs a lease that negotiated schema " +
                    "$SURFACE_SNAPSHOT_SCHEMA_VERSION: " else "Layout sync needs both halves of one ZIP: ") +
                "${currentFrontendPluginMismatchRemedy(mismatch)}.",
            NotificationType.WARNING,
        )
    AgentDocNotices.show(project, notification, "ControllerSurfaceIngress.frontendPluginMismatch")
}

/** One project's ingress: the controller publisher and layout forwarder behind the lease state. */
private class ControllerSurfaceIngress(private val project: Project) : SurfaceIngress {
    private val layoutForwarder = ControllerRemoteClientLayoutForwarder(project)
    private val state = AuthenticatedSurfaceIngressState(
        ControllerSurfaceSnapshotPublisher(),
        layoutForwarder,
        backendActivationReceipt = ShellGenerationHost.getInstance().receipt(),
        backendActivationReceiptProvider = BackendFrontendReleaseActivation::receipt,
        onFrontendPluginMismatch = { clientId, generation, mismatch ->
            val line = frontendPluginMismatchLine(
                clientId,
                generation,
                mismatch.detail,
                currentFrontendPluginMismatchRemedy(mismatch),
            )
            Logger.getInstance(ControllerSurfaceIngress::class.java).warn(line)
            notifyFrontendPluginMismatch(project, mismatch)
        },
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
    /**
     * `#viewsnapshotgenseedmac`: the first lease's connection generation is `generationSeed + 1`.
     * The controller fences `editor_view_snapshot_observe` by `(connection_generation, sequence)`
     * per client, and this state (with its counter) is recreated with the IDE process. A counter
     * starting at 0 made every restarted backend publish generation 1 again, below the dead IDE's
     * high-water mark, and only the controller's peer-pid successor check (`#viewsnapshotgenreset`,
     * `SO_PEERCRED`) could tell the restart apart; macOS Unix sockets report no peer pid, so there
     * every snapshot stayed stale. Seeding from wall-clock milliseconds makes each process start
     * above every generation its predecessor issued (one per reconnect, far fewer than the
     * milliseconds it lived), so a restart reads as an ordinary newer generation on every platform
     * and needs no wire field or transport identity.
     */
    generationSeed: Long = System.currentTimeMillis(),
    /** The shell creates this ingress for one backend generation. A swap must replace the ingress. */
    private val backendActivationReceipt: ActivationReceipt =
        ActivationReceipt.shellOnly(ActivationReceipt.ROLE_BACKEND),
    /** R2 may advance the backend peer after this platform-held ingress was created. */
    private val backendActivationReceiptProvider: () -> ActivationReceipt = { backendActivationReceipt },
    /** GH #339 R1: called when a connection reports a new frontend activation receipt. */
    private val onFrontendReceipt: (clientId: String, connectionGeneration: Long, receipt: ActivationReceipt) -> Unit =
        ::logFrontendReceipt,
    /**
     * GH #435: runs the connected-without-publication check [FRONTEND_AUTHORITY_DIAGNOSTIC_DELAY_MS]
     * after a remote layout lease opens. Injected so tests run it synchronously.
     */
    private val scheduleDiagnostic: (delayMs: Long, check: () -> Unit) -> Unit = ::scheduleOnDiagnosticTimer,
    /** GH #435: one backend idea.log line per publication-state change (refusal, acceptance, stall). */
    private val log: (warn: Boolean, line: String) -> Unit = ::logPublication,
    /**
     * GH #435: called once per lease when the Client's frontend is not the backend's own build
     * ([frontendPluginMismatch]). An upgrade on the Remote Dev backend never reaches the Client.
     */
    private val onFrontendPluginMismatch: (
        clientId: String,
        connectionGeneration: Long,
        mismatch: FrontendPluginMismatch,
    ) -> Unit = { clientId, generation, mismatch ->
        logPublication(
            true,
            frontendPluginMismatchLine(clientId, generation, mismatch.detail, currentFrontendPluginMismatchRemedy(mismatch)),
        )
    },
) {
    private val generation = AtomicLong(generationSeed)
    private val mutex = Mutex()
    private val connections = mutableMapOf<String, ConnectionState>()
    private val mutableFrontendAuthorityActive = MutableStateFlow(false)
    val frontendAuthorityActive: StateFlow<Boolean> = mutableFrontendAuthorityActive.asStateFlow()
    private val mutableLayoutAuthorityActive = MutableStateFlow(false)
    val layoutAuthorityActive: StateFlow<Boolean> = mutableLayoutAuthorityActive.asStateFlow()
    private val mutableLayoutPublicationReadiness = MutableStateFlow(LayoutPublicationReadiness.INACTIVE)
    val layoutPublicationReadiness: StateFlow<LayoutPublicationReadiness> =
        mutableLayoutPublicationReadiness.asStateFlow()

    /**
     * GH #435: what the frontend -> backend -> controller channel last did for each open lease,
     * read without the mutex by the delayed stall check. Keyed by lease id; removed on retire.
     */
    private val publications = ConcurrentHashMap<String, LeasePublication>()

    suspend fun open(
        authenticatedClientId: String,
        frontendInstanceId: String,
    ): SurfaceIngressLease = mutex.withLock {
        connections.remove(authenticatedClientId)?.let { previous ->
            publications.remove(previous.lease.leaseId)
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
        val publication = LeasePublication(authenticatedClientId, lease.connectionGeneration)
        publications[lease.leaseId] = publication
        refreshFrontendAuthority()
        log(
            false,
            "[split-surface] lease opened client=$authenticatedClientId generation=${lease.connectionGeneration} " +
                "layout_authority=${if (layoutAuthority) "remote_measured" else "local_client"} " +
                "readiness=${mutableLayoutPublicationReadiness.value.wire()}",
        )
        if (layoutAuthority) {
            scheduleDiagnostic(FRONTEND_AUTHORITY_DIAGNOSTIC_DELAY_MS) { reportStalledPublication(lease.leaseId) }
        }
        lease
    }

    /**
     * GH #435 / GH #352: a remote layout lease that has not had one snapshot accepted after the
     * schema-negotiation window leaves structural routing blocked. Say so in the backend log with
     * the last refusal, so the failing hop (frontend never published, controller delivery failed,
     * no controller root, validation) is visible without the Client's idea.log.
     */
    internal fun reportStalledPublication(leaseId: String) {
        val publication = publications[leaseId] ?: return
        if (publication.acceptedSequence > 0) return
        log(
            true,
            frontendAuthorityUnavailableDiagnostic(
                publication.clientId,
                "remote",
                FRONTEND_AUTHORITY_CONNECTED_WITHOUT_PUBLICATION,
            ) + " generation=${publication.generation} refusals=${publication.refusals} " +
                "last_refusal=${publication.lastRefusal ?: "none(the_frontend_published_no_snapshot)"}",
        )
    }

    private fun recordRefusal(current: ConnectionState, ack: SurfaceIngressAck, sequence: Long): SurfaceIngressAck {
        val publication = publications[current.lease.leaseId] ?: return ack
        val refusal = "${ack.status}:${ack.diagnostic ?: ""}"
        publication.refusals++
        val changed = publication.lastRefusal != refusal
        publication.lastRefusal = refusal
        if (current.layoutAuthority) {
            layoutForwarder.refused(current.clientId, current.lease.connectionGeneration, ack.status.name, ack.diagnostic)
        }
        if (changed) {
            log(
                true,
                "[split-surface] snapshot refused client=${current.clientId} " +
                    "generation=${current.lease.connectionGeneration} sequence=$sequence status=${ack.status} " +
                    "readiness=${mutableLayoutPublicationReadiness.value.wire()} " +
                    "diagnostic=${ack.diagnostic ?: "none"}",
            )
        }
        return ack
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
        publications[lease.leaseId]?.takeIf { !it.pluginChecked }?.let { publication ->
            publication.pluginChecked = true
            frontendPluginMismatch(
                snapshot,
                ActivationReceipt.decodeOrNull(snapshot.activationReceipt),
                backendActivationReceiptProvider().shellDigest,
            )?.let { detail -> onFrontendPluginMismatch(authenticatedClientId, lease.connectionGeneration, detail) }
        }
        val validation = SurfaceSnapshotValidator.validate(snapshot)
        if (validation != null) {
            return@withLock recordRefusal(current, rejected(validation.status, lease, validation.diagnostic), snapshot.sequence)
        }
        if (snapshot.sequence <= current.acceptedSequence) {
            return@withLock rejected(
                SurfaceIngressStatus.STALE_SEQUENCE,
                lease,
                "sequence ${snapshot.sequence} is not newer than ${current.acceptedSequence}",
            )
        }
        val receipt = if (snapshot.schemaVersion >= ACTIVATION_RECEIPT_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
            ActivationReceipt.decodeOrNull(snapshot.activationReceipt)
        } else {
            null
        }
        if (current.activationRevoked || !admitsFrontendReceipt(current, receipt)) {
            if (!current.activationRevoked && current.acceptedSequence > 0) {
                connections[authenticatedClientId] = current.copy(activationRevoked = true, activationReceipt = null)
                refreshFrontendAuthority()
                if (current.layoutAuthority) {
                    layoutForwarder.hold(authenticatedClientId, lease.connectionGeneration)
                }
                // A retired controller generation cannot be reused. A fresh authenticated lease
                // is required to publish again, even if a late matching snapshot arrives.
                publisher.retire(authenticatedClientId, lease.connectionGeneration)
            }
            return@withLock recordRefusal(
                current,
                rejected(
                    SurfaceIngressStatus.DELIVERY_FAILED,
                    lease,
                    FRONTEND_AUTHORITY_UNAVAILABLE_READINESS,
                ),
                snapshot.sequence,
            )
        }
        val delivery = publisher.publish(
            clientId = authenticatedClientId,
            generation = lease.connectionGeneration,
            snapshot = snapshot,
        )
        if (!delivery.accepted) {
            return@withLock recordRefusal(
                current,
                rejected(
                    SurfaceIngressStatus.DELIVERY_FAILED,
                    lease,
                    delivery.diagnostic ?: "controller delivery failed",
                ),
                snapshot.sequence,
            )
        }
        connections[authenticatedClientId] = current.copy(
            acceptedSequence = snapshot.sequence,
            acceptedSurfaces = snapshot.surfaces.associate { it.paneId to it.surfaceGeneration },
            activationReceipt = receipt ?: current.activationReceipt,
        )
        if (receipt != null && receipt != current.activationReceipt) {
            onFrontendReceipt(authenticatedClientId, lease.connectionGeneration, receipt)
        }
        refreshFrontendAuthority()
        publications[lease.leaseId]?.let { publication ->
            val firstAcceptance = publication.acceptedSequence == 0L
            val recovered = publication.lastRefusal
            publication.acceptedSequence = snapshot.sequence
            publication.lastRefusal = null
            if (firstAcceptance || recovered != null) {
                log(
                    false,
                    "[split-surface] snapshot accepted client=$authenticatedClientId " +
                        "generation=${lease.connectionGeneration} sequence=${snapshot.sequence} " +
                        "schema=${snapshot.schemaVersion} surfaces=${snapshot.surfaces.size} " +
                        "measured_windows=${snapshot.measuredWindowCount()} " +
                        "layout_authority=${if (current.layoutAuthority) "remote_measured" else "local_client"} " +
                        "readiness=${mutableLayoutPublicationReadiness.value.wire()} " +
                        "first=$firstAcceptance after_refusal=${recovered ?: "none"}",
                )
            }
        }
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

    private fun admitsFrontendReceipt(current: ConnectionState, receipt: ActivationReceipt?): Boolean {
        val backend = backendActivationReceiptProvider()
        if (backend.implSha256 == null && backend.epoch == 0L) {
            // Legacy snapshots and a shell-only frontend remain valid in R1.
            return receipt == null || (receipt.implSha256 == null && receipt.epoch == 0L)
        }
        if (receipt == null || backend.role != ActivationReceipt.ROLE_BACKEND ||
            receipt.role != ActivationReceipt.ROLE_FRONTEND ||
            backend.jvmInstanceId.isBlank() || receipt.jvmInstanceId.isBlank() ||
            backend.jvmInstanceId == receipt.jvmInstanceId ||
            !SHA_256.matches(backend.shellDigest) || receipt.shellDigest != backend.shellDigest ||
            backend.spiVersion <= 0 || receipt.spiVersion != backend.spiVersion ||
            backend.implSha256?.let(SHA_256::matches) != true ||
            backend.generation <= 0 || backend.epoch <= 0
        ) {
            return false
        }
        val previousJvm = current.activationReceipt?.jvmInstanceId
        return (previousJvm == null || previousJvm == receipt.jvmInstanceId) &&
            receipt.implSha256 == backend.implSha256 &&
            receipt.generation > 0 && receipt.epoch == backend.epoch
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
        if (current.activationRevoked) {
            return@withLock rejected(
                SurfaceIngressStatus.DELIVERY_FAILED,
                lease,
                FRONTEND_AUTHORITY_UNAVAILABLE_READINESS,
            )
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
        publications.remove(lease.leaseId)
        if (current.layoutAuthority) {
            layoutForwarder.retire(authenticatedClientId, lease.connectionGeneration)
        }
        publisher.retire(authenticatedClientId, lease.connectionGeneration)
        refreshFrontendAuthority()
        log(
            false,
            "[split-surface] lease retired client=$authenticatedClientId generation=${lease.connectionGeneration} " +
                "accepted_sequence=${current.acceptedSequence}",
        )
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
            layoutConnections.any { it.acceptedSequence == 0L || it.activationRevoked } ->
                LayoutPublicationReadiness.CONNECTED_BLOCKED
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

    /** GH #435: mutable publication bookkeeping of one lease; see [publications]. */
    private class LeasePublication(val clientId: String, val generation: Long) {
        @Volatile var acceptedSequence: Long = 0
        @Volatile var lastRefusal: String? = null
        @Volatile var refusals: Long = 0
        @Volatile var pluginChecked: Boolean = false
    }

    private data class ConnectionState(
        val clientId: String,
        val frontendInstanceId: String,
        val lease: SurfaceIngressLease,
        val acceptedSequence: Long,
        val acceptedSurfaces: Map<String, Long>,
        val layoutAuthority: Boolean,
        val activationReceipt: ActivationReceipt? = null,
        val activationRevoked: Boolean = false,
    )

    private companion object {
        val LOG = Logger.getInstance(AuthenticatedSurfaceIngressState::class.java)
        val SHA_256 = Regex("[0-9a-f]{64}")

        fun logFrontendReceipt(clientId: String, connectionGeneration: Long, receipt: ActivationReceipt) {
            LOG.info("[shell] frontend client=$clientId connection_generation=$connectionGeneration ${receipt.encode()}")
        }

        fun logPublication(warn: Boolean, line: String) {
            if (warn) LOG.warn(line) else LOG.info(line)
        }

        private val diagnosticTimer: ScheduledExecutorService by lazy {
            Executors.newSingleThreadScheduledExecutor { runnable ->
                Thread(runnable, "agent-doc-surface-ingress-diagnostic").apply { isDaemon = true }
            }
        }

        fun scheduleOnDiagnosticTimer(delayMs: Long, check: () -> Unit) {
            diagnosticTimer.schedule(
                {
                    try {
                        check()
                    } catch (failure: Exception) {
                        LOG.warn("[split-surface] publication stall check failed: ${failure.message}", failure)
                    }
                },
                delayMs,
                TimeUnit.MILLISECONDS,
            )
        }
    }
}

/**
 * GH #435: why one lease's frontend snapshots cannot carry this backend's layout authority.
 * [unansweredNegotiation] marks a schema-1 snapshot: every frontend since schema negotiation
 * (2026-10-20) publishes schema 1 only when the backend gave no schema answer within its 5 s
 * window, so the remedy is a new lease (a Client reconnect), not a reinstall.
 */
internal data class FrontendPluginMismatch(
    val detail: String,
    val unansweredNegotiation: Boolean = false,
)

/**
 * GH #435: why the JetBrains Client's frontend is not this backend's Agent Doc build, or null when
 * it is (or cannot be told apart). Both halves of one modular ZIP share `agent.doc.shared`, so one
 * install reports one shell digest; a frontend that negotiated a lower snapshot schema (2-4)
 * predates this backend. Schema 1 is different: it is the frontend's fallback when the
 * negotiation went unanswered, which on Coder (2026-11-24) was a 0.2.1046 frontend paired with a
 * 0.2.1046 backend. An upgrade run on a Remote Dev backend never updates the Client, so this is
 * the version check that names the stale side.
 */
internal fun frontendPluginMismatch(
    snapshot: FrontendSurfaceSnapshot,
    frontendReceipt: ActivationReceipt?,
    backendShellDigest: String,
): FrontendPluginMismatch? {
    if (snapshot.schemaVersion <= LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
        return FrontendPluginMismatch(
            "frontend_schema=${snapshot.schemaVersion} backend_schema=$SURFACE_SNAPSHOT_SCHEMA_VERSION " +
                "(the JetBrains Client frontend published schema ${snapshot.schemaVersion} without this " +
                "backend's schema answer: its 5 s schema negotiation went unanswered on this lease, or it " +
                "predates negotiation; schema ${snapshot.schemaVersion} carries no geometry and no activation receipt)",
            unansweredNegotiation = true,
        )
    }
    if (snapshot.schemaVersion < SURFACE_SNAPSHOT_SCHEMA_VERSION) {
        return FrontendPluginMismatch(
            "frontend_schema=${snapshot.schemaVersion} backend_schema=$SURFACE_SNAPSHOT_SCHEMA_VERSION " +
                "(the JetBrains Client runs an older Agent Doc frontend)",
        )
    }
    val frontendShell = frontendReceipt?.shellDigest
        ?: return FrontendPluginMismatch(
            "frontend_receipt=unreadable (the JetBrains Client runs a different Agent Doc frontend)",
        )
    if (frontendShell == ShellIdentity.UNKNOWN || backendShellDigest == ShellIdentity.UNKNOWN) return null
    if (frontendShell == backendShellDigest) return null
    return FrontendPluginMismatch(
        "frontend_shell=${frontendShell.take(12)} backend_shell=${backendShellDigest.take(12)} " +
            "(the JetBrains Client and the Remote Dev backend run different Agent Doc builds)",
    )
}

/**
 * GH #435: the remedy for [mismatch]. A schema-1 lease is repaired by a new lease, which asks for
 * the schema again; only a Client log that names another plugin calls for the reinstall.
 */
internal fun frontendPluginMismatchRemedy(
    mismatch: FrontendPluginMismatch,
    version: String?,
    ideDataDir: String?,
): String {
    val install = frontendPluginMismatchRemedy(version, ideDataDir)
    if (!mismatch.unansweredNegotiation) return install
    val known = version?.takeIf { it.isNotBlank() && it != "unknown" }
    return "reconnect the JetBrains Client so it opens a new lease and asks this backend for its schema " +
        "again: quit and restart the JetBrains Client, or close and reopen this project in it. Its idea.log " +
        "line `snapshot schema negotiated version=1 backend_offered=none` confirms the unanswered " +
        "negotiation; Agent Doc 0.2.1052 and later ask again by themselves after a refusal. Only if the " +
        "Client idea.log has no `frontend surface publisher started` line, or that line names a plugin " +
        "other than ${known ?: "this backend's version"}, " + install
}

/**
 * GH #435/#439: how to put this backend's ZIP into the JetBrains Client. It names the real asset
 * (a `<version>` placeholder rendered as an empty HTML tag in the Client notification, which showed
 * `agent-doc-jetbrains-262-.zip`) and the directory a Client started by Toolbox or Gateway actually
 * loads: the backend IDE's data directory name with `plugins/frontend` on Windows and macOS and with
 * `frontend` on Linux, whose plugins path has no `plugins` segment; not `JetBrainsClient<v>`.
 * The text carries no angle brackets because the notification body is HTML.
 */
internal fun frontendPluginMismatchRemedy(version: String?, ideDataDir: String?): String {
    val asset = version?.takeIf { it.isNotBlank() && it != "unknown" }
        ?.let { "agent-doc-jetbrains-262-$it.zip" }
        ?: "agent-doc-jetbrains-262 ZIP of this backend's version"
    val ide = ideDataDir?.takeIf(String::isNotBlank) ?: "IntelliJIdea2026.3"
    return "install the same $asset in the JetBrains Client: quit the Client, delete every " +
        "agent-doc-jetbrains, agent-doc-jetbrains-262 and agent-doc-jetbrains-263 folder from its plugins " +
        "directory (a Client started by Toolbox or Gateway loads them from %APPDATA%\\JetBrains\\$ide\\plugins\\frontend " +
        "on Windows, ~/Library/Application Support/JetBrains/$ide/plugins/frontend on macOS, or " +
        "~/.local/share/JetBrains/$ide/frontend on Linux), extract " +
        "the ZIP there, save the release's jetbrains-frontend-release-ed25519.pub in that directory as " +
        ".agent-doc-frontend-release/trusted-ed25519-public-keys, and start the Client again " +
        "(`agent-doc upgrade` on the backend prints the exact download URLs and steps; run on the Client " +
        "machine it does all of this)"
}

/** GH #439: the backend IDE's data directory name (`IntelliJIdea2026.3`), from its config path. */
internal fun backendIdeDataDirName(): String? =
    runCatching { Path.of(PathManager.getConfigPath()).fileName?.toString() }.getOrNull()

internal fun currentFrontendPluginMismatchRemedy(): String =
    frontendPluginMismatchRemedy(runCatching { pluginVersion() }.getOrNull(), backendIdeDataDirName())

internal fun currentFrontendPluginMismatchRemedy(mismatch: FrontendPluginMismatch): String =
    frontendPluginMismatchRemedy(mismatch, runCatching { pluginVersion() }.getOrNull(), backendIdeDataDirName())

internal fun frontendPluginMismatchLine(
    clientId: String,
    generation: Long,
    detail: String,
    remedy: String = currentFrontendPluginMismatchRemedy(),
): String =
    "[split-surface] frontend/backend plugin mismatch client=$clientId generation=$generation $detail " +
        "remedy=$remedy"

/** GH #435: the editor windows of [this] snapshot that carry measured on-screen bounds, as `n/total`. */
internal fun FrontendSurfaceSnapshot.measuredWindowCount(): String {
    val windows = surfaces.flatMap { it.windows }
    return "${windows.count { it.bounds != null }}/${windows.size}"
}

private fun LayoutPublicationReadiness.wire(): String = name.lowercase()

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
