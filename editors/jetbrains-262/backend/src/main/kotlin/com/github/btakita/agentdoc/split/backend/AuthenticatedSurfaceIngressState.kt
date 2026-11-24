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
import com.github.btakita.agentdoc.split.frontendLeasePluginVersion
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

    override fun selectedFrontendReleasePackageUrl() =
        runCatching { FrontendImplementationReleaseSource.packageUrl() }.getOrNull()

    override fun selectedFrontendRelease() =
        runCatching { FrontendImplementationReleaseSource.current() }
            .onFailure { LOG.warn("[frontend-impl] frontend release handoff unreadable", it) }
            .getOrNull()

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
            frontendPluginMismatchTitle(mismatch, runCatching { pluginVersion() }.getOrNull()),
            "${mismatch.detail}. " +
                (if (mismatch.unansweredNegotiation) "Layout sync needs a lease that negotiated schema " +
                    "$SURFACE_SNAPSHOT_SCHEMA_VERSION: " else "Layout sync needs both halves of one ZIP: ") +
                "${currentFrontendPluginMismatchRemedy(mismatch)}.",
            NotificationType.WARNING,
        )
    AgentDocNotices.show(project, notification, "ControllerSurfaceIngress.frontendPluginMismatch")
}

/**
 * GH #435/#440: the notification title. A Client that cannot load this backend's release (another
 * shell, so the backend withholds it) leads with the version comparison; a schema-1 lease on the
 * same shell keeps the unanswered-negotiation one.
 */
internal fun frontendPluginMismatchTitle(mismatch: FrontendPluginMismatch, backendVersion: String?): String {
    val backend = backendVersion?.takeIf { it.isNotBlank() && it != "unknown" }
        ?: mismatch.shellRefusal?.backendPluginVersion
    mismatch.shellRefusal?.let { refusal ->
        val client = refusal.clientPluginVersion?.let { "JetBrains Client plugin $it" } ?: "the JetBrains Client plugin"
        val order = refusal.clientVersionOrder(backend)
        return when {
            order == null || order == 0 ->
                "Agent Doc: $client cannot load the backend's ${backend ?: "plugin"}: different Agent Doc shell"
            order < 0 -> "Agent Doc: $client is older than the backend's ${backend ?: "plugin"}: update the Client plugin"
            else -> "Agent Doc: $client is newer than the backend's ${backend ?: "plugin"}: install one version on both"
        }
    }
    return if (mismatch.unansweredNegotiation) {
        "Agent Doc: the JetBrains Client frontend publishes layout without geometry"
    } else {
        "Agent Doc: the JetBrains Client frontend is not the backend's plugin ${backend ?: "build"}"
    }
}

/** One project's ingress: the controller publisher and layout forwarder behind the lease state. */
private class ControllerSurfaceIngress(private val project: Project) : SurfaceIngress {
    private val layoutForwarder = ControllerRemoteClientLayoutForwarder(project)
    private val state = AuthenticatedSurfaceIngressState(
        ControllerSurfaceSnapshotPublisher(),
        layoutForwarder,
        backendActivationReceipt = ShellGenerationHost.getInstance().receipt(),
        backendActivationReceiptProvider = BackendFrontendReleaseActivation::receipt,
        clientShellEvidence = FrontendReleasePollLedger.shared,
        backendPluginVersion = { runCatching { pluginVersion() }.getOrNull() },
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
    /**
     * GH #440: what the shell's RPC layer learned about each Client's frontend build (its build
     * report, or a whole-package release poll only Clients before 0.2.1052 make).
     */
    private val clientShellEvidence: FrontendClientShellEvidenceSource = FrontendClientShellEvidenceSource { null },
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
    /**
     * GH #441: this backend's Agent Doc plugin version. A schema-1 snapshot from a frontend that
     * names the same version in its lease is provisional (it asks for the schema again after a
     * refusal), so it is reported only if it is still schema 1 after that refusal.
     */
    private val backendPluginVersion: () -> String? = { runCatching { pluginVersion() }.getOrNull() },
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
                "readiness=${mutableLayoutPublicationReadiness.value.wire()} " +
                "frontend_plugin=${frontendLeasePluginVersion(frontendInstanceId) ?: "unnamed"}",
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
        publications[lease.leaseId]?.takeIf { !it.shellRefusalReported }?.let { publication ->
            // GH #440: a Client proven unable to load this backend's release shell is reported at once,
            // and once more if that proof arrives after a first report (its first whole-package
            // poll can land after its first snapshot): the shell refusal replaces the negotiation
            // story. Everything else is reported once per lease, after GH #441's provisional hold.
            val mismatch = frontendPluginMismatch(
                snapshot,
                ActivationReceipt.decodeOrNull(snapshot.activationReceipt),
                backendActivationReceiptProvider().shellDigest,
                clientShellEvidence.evidence(authenticatedClientId),
                backendPluginVersion(),
                frontendLeasePluginVersion(current.frontendInstanceId),
            )
            if (mismatch?.shellRefusal != null) {
                publication.pluginChecked = true
                publication.shellRefusalReported = true
                onFrontendPluginMismatch(authenticatedClientId, lease.connectionGeneration, mismatch)
            } else if (!publication.pluginChecked) {
                if (mismatch != null && provisionalSchemaOne(current, publication, mismatch)) {
                    if (!publication.provisionalDeferred) {
                        publication.provisionalDeferred = true
                        log(
                            false,
                            "[split-surface] provisional schema-1 snapshot client=$authenticatedClientId " +
                                "generation=${lease.connectionGeneration} " +
                                "frontend_plugin=${frontendLeasePluginVersion(current.frontendInstanceId)} " +
                                "(the same plugin asks for the schema again after a refusal; a plugin mismatch " +
                                "is reported only if its next snapshot is still schema 1)",
                        )
                    }
                } else {
                    publication.pluginChecked = true
                    mismatch?.let { onFrontendPluginMismatch(authenticatedClientId, lease.connectionGeneration, it) }
                }
            }
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
            if (!current.activationRevoked && current.acceptedSequence > 0 &&
                awaitsFrontendActivation(current, snapshot, receipt)
            ) {
                // GH #435 review (cycles 1 and 4): this lease's accepted snapshots carried no
                // activated receipt (a shell-only backend admitted them; on a cold backend that is
                // a late first schema answer's provisional schema 1, or a schema 5 with a
                // shell-only receipt), so the fence never granted it an activation to revoke.
                // Refuse like any not-yet-activated snapshot: the Client asks for the schema again
                // or fetches the release after this refusal, and its next snapshot carries the
                // paired receipt, which this lease then admits.
                if (!current.awaitingActivation) {
                    connections[authenticatedClientId] = current.copy(awaitingActivation = true)
                    refreshFrontendAuthority()
                    log(
                        false,
                        "[split-surface] snapshot awaits frontend activation client=$authenticatedClientId " +
                            "generation=${lease.connectionGeneration} sequence=${snapshot.sequence} " +
                            "schema=${snapshot.schemaVersion} accepted_sequence=${current.acceptedSequence} " +
                            "(the backend peer activated after this lease's receipt-less or shell-only " +
                            "snapshots were accepted; the lease stays open for the Client's activated snapshot)",
                    )
                }
            } else if (!current.activationRevoked && current.acceptedSequence > 0) {
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
            awaitingActivation = false,
        )
        if (receipt != null && receipt != current.activationReceipt) {
            onFrontendReceipt(authenticatedClientId, lease.connectionGeneration, receipt)
        }
        refreshFrontendAuthority()
        publications[lease.leaseId]?.takeIf { it.provisionalDeferred && !it.pluginChecked }?.let { publication ->
            // GH #441: only a refusal makes the frontend ask for the schema again, so an accepted
            // schema-1 snapshot stays schema 1 (no geometry) for the rest of this lease.
            publication.pluginChecked = true
            frontendPluginMismatch(
                snapshot,
                null,
                backendActivationReceiptProvider().shellDigest,
                clientShellEvidence.evidence(authenticatedClientId),
                backendPluginVersion(),
                frontendLeasePluginVersion(current.frontendInstanceId),
            )?.let { onFrontendPluginMismatch(authenticatedClientId, lease.connectionGeneration, it) }
        }
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

    /**
     * GH #441: whether [mismatch] is a schema-1 snapshot the frontend published provisionally and
     * will replace: the lease names this backend's own plugin (whose frontend asks for the schema
     * again after a refusal) and no snapshot of this lease has been refused yet. A schema 1 that
     * follows a refusal, or one from a frontend that names another plugin or none, is reported.
     */
    private fun provisionalSchemaOne(
        current: ConnectionState,
        publication: LeasePublication,
        mismatch: FrontendPluginMismatch,
    ): Boolean {
        if (!mismatch.unansweredNegotiation || publication.refusals > 0) return false
        val backend = backendPluginVersion()?.takeIf { it.isNotBlank() && it != "unknown" } ?: return false
        return frontendLeasePluginVersion(current.frontendInstanceId) == backend
    }

    /**
     * GH #435 review (cycle 1): whether a fence refusal of [current], which already had a snapshot
     * accepted, waits for the Client's activation instead of revoking the lease. Only a lease whose
     * accepted snapshots carried no activated receipt waits: the shell-only backend admitted them
     * under the R1 rule (receipt-less, or with a shell-only receipt), so the fence never granted
     * this lease an activation it could now take back. A lease that had an activated receipt
     * accepted still revokes on the first mismatch, and so does a lease whose Client is proven
     * unable to load this backend's release (GH #440): no snapshot of that Client can ever pass
     * the fence, so its layout is held, not left on the last shell-only snapshot.
     *
     * GH #435 review (cycle 4): a shell-only receipt counts as no receipt. The delivery gate keeps
     * its negotiation and peer-activation claim per Client, not per lease, so a cold backend can
     * accept a schema-5 snapshot with a shell-only receipt before its peer activates: a second
     * project's lease opening while this one's first snapshot is in flight, or a failed first
     * release discovery that the next release poll retries. Revoking such a lease would stop its
     * layout sync until a reconnect, although the Client's paired receipt is still on its way.
     */
    private fun awaitsFrontendActivation(
        current: ConnectionState,
        snapshot: FrontendSurfaceSnapshot,
        receipt: ActivationReceipt?,
    ): Boolean {
        if (current.activationReceipt?.activated() == true) return false
        return frontendShellRefusal(
            snapshot,
            receipt,
            backendActivationReceiptProvider().shellDigest,
            clientShellEvidence.evidence(current.clientId),
            backendPluginVersion(),
            frontendLeasePluginVersion(current.frontendInstanceId),
        ) == null
    }

    /** Whether this receipt reports an activated implementation, not a shell-only JVM. */
    private fun ActivationReceipt.activated(): Boolean = implSha256 != null || epoch != 0L

    private fun admitsFrontendReceipt(current: ConnectionState, receipt: ActivationReceipt?): Boolean {
        val backend = backendActivationReceiptProvider()
        if (!backend.activated()) {
            // Legacy snapshots and a shell-only frontend remain valid in R1.
            return receipt == null || !receipt.activated()
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
            layoutConnections.any { it.acceptedSequence == 0L || it.activationRevoked || it.awaitingActivation } ->
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
        /** GH #441: a provisional schema-1 snapshot was seen and its mismatch report deferred. */
        @Volatile var provisionalDeferred: Boolean = false
        /** GH #440: this lease's shell refusal was reported; nothing more is reported for it. */
        @Volatile var shellRefusalReported: Boolean = false
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
        /**
         * GH #435 review (cycles 1 and 4): the backend peer activated after this lease's
         * receipt-less or shell-only snapshots were accepted, and the fence refused the next one
         * without revoking
         * ([awaitsFrontendActivation]). Layout readiness is blocked until a snapshot is accepted.
         */
        val awaitingActivation: Boolean = false,
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
 * [shellRefusal] (GH #440) marks a Client whose Agent Doc shell is not this backend's: it cannot
 * load this backend's frontend release (since GH #435 the backend withholds it from such a Client),
 * and only the backend's plugin in the Client helps.
 */
internal data class FrontendPluginMismatch(
    val detail: String,
    val unansweredNegotiation: Boolean = false,
    val shellRefusal: FrontendShellRefusal? = null,
)

/**
 * GH #440: the proof that the JetBrains Client cannot load this backend's frontend release. A
 * generation built against another shell cannot load: a Client refuses one whose shell digest is
 * not its own, and since GH #435 [FrontendReleaseDeliveryGate] withholds the release from such a
 * Client, so it never receives it. [clientShell] is null when the Client never reported it, which
 * is the case for every Client before 0.2.1052 ([legacyReleasePoller]); those poll the whole
 * package, and their shell predates this backend's.
 */
internal data class FrontendShellRefusal(
    val clientShell: String?,
    val backendShell: String,
    val clientPluginVersion: String? = null,
    val clientPluginPath: String? = null,
    val backendPluginVersion: String? = null,
    val legacyReleasePoller: Boolean = false,
    val legacyPackagePolls: Int = 0,
) {
    /**
     * Negative when the Client plugin is older than [backendVersion], positive when newer, zero
     * when equal, null when unknown. A legacy poller is older than any backend from 0.2.1052 on.
     */
    fun clientVersionOrder(backendVersion: String? = backendPluginVersion): Int? {
        compareAgentDocVersions(clientPluginVersion, backendVersion)?.let { return it }
        if (!legacyReleasePoller) return null
        val backendServesIdentity = compareAgentDocVersions(backendVersion, IDENTITY_POLL_FIRST_VERSION)
        return if (backendServesIdentity != null && backendServesIdentity >= 0) -1 else null
    }

    /** `client_plugin=0.2.1046` facts, in the order an operator compares them. */
    fun facts(): String =
        "client_shell=${clientShell?.take(12) ?: "unreported"} backend_shell=${backendShell.take(12)} " +
            "client_plugin=${clientPluginVersion ?: if (legacyReleasePoller) "before_$IDENTITY_POLL_FIRST_VERSION" else "unknown"} " +
            "backend_plugin=${backendPluginVersion ?: "unknown"}"

    companion object {
        /** The first Agent Doc Client that asks for the release identity before the package. */
        const val IDENTITY_POLL_FIRST_VERSION = "0.2.1052"
    }
}

/** GH #440: dotted numeric order of two Agent Doc versions, or null when either is not one. */
internal fun compareAgentDocVersions(left: String?, right: String?): Int? {
    fun parts(value: String?): List<Long>? = value?.trim()?.removePrefix("v")?.split('.')
        ?.map { it.toLongOrNull() ?: return null }
        ?.takeIf { it.isNotEmpty() }
    val l = parts(left) ?: return null
    val r = parts(right) ?: return null
    for (index in 0 until maxOf(l.size, r.size)) {
        val order = l.getOrElse(index) { 0 }.compareTo(r.getOrElse(index) { 0 })
        if (order != 0) return order
    }
    return 0
}

/**
 * GH #435: why the JetBrains Client's frontend is not this backend's Agent Doc build, or null when
 * it is (or cannot be told apart). Both halves of one modular ZIP share `agent.doc.shared`, so one
 * install reports one shell digest; a frontend that negotiated a lower snapshot schema (2-4)
 * predates this backend. Schema 1 is different: it is the frontend's fallback when the
 * negotiation went unanswered, which on Coder (2026-11-24) was a 0.2.1046 frontend paired with a
 * 0.2.1046 backend. An upgrade run on a Remote Dev backend never updates the Client, so this is
 * the version check that names the stale side.
 *
 * GH #440: [clientEvidence] is what the shell's RPC layer learned about the same Client. A Client
 * whose shell is proven different (its schema-5 receipt or its own build report), or that polls
 * the whole release package the way only Clients before 0.2.1052 do, cannot load this backend's
 * release, which the backend withholds from it; that leads, because asking for the schema again
 * cannot fix it. Matching shells keep the unanswered-negotiation story.
 */
internal fun frontendPluginMismatch(
    snapshot: FrontendSurfaceSnapshot,
    frontendReceipt: ActivationReceipt?,
    backendShellDigest: String,
    clientEvidence: FrontendClientShellEvidence? = null,
    backendPluginVersion: String? = null,
    leasePluginVersion: String? = null,
): FrontendPluginMismatch? {
    frontendShellRefusal(snapshot, frontendReceipt, backendShellDigest, clientEvidence, backendPluginVersion, leasePluginVersion)
        ?.let { refusal ->
            return FrontendPluginMismatch(
                // No `frontend_schema=` token: `agent-doc upgrade` (src/plugin.rs) reads
                // `frontend_schema=1` as the reconnect case, and a shell refusal needs the ZIP.
                "${refusal.facts()} (${frontendShellRefusalStory(refusal)})",
                shellRefusal = refusal,
            )
        }
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
    if (frontendReceipt?.shellDigest == null) {
        return FrontendPluginMismatch(
            "frontend_receipt=unreadable (the JetBrains Client runs a different Agent Doc frontend)",
        )
    }
    return null
}

/**
 * GH #440: the shell refusal [snapshot]'s Client is proven to make, or null. A proven client shell
 * that equals the backend's (or an unbuilt `unknown` identity on either side) proves no refusal.
 */
internal fun frontendShellRefusal(
    snapshot: FrontendSurfaceSnapshot,
    frontendReceipt: ActivationReceipt?,
    backendShellDigest: String,
    clientEvidence: FrontendClientShellEvidence?,
    backendPluginVersion: String?,
    /** GH #441: the plugin version the Client named in its lease (0.2.1053 and later). */
    leasePluginVersion: String? = null,
): FrontendShellRefusal? {
    if (backendShellDigest == ShellIdentity.UNKNOWN) return null
    val receiptShell = frontendReceipt?.shellDigest
        ?.takeIf { snapshot.schemaVersion >= ACTIVATION_RECEIPT_SURFACE_SNAPSHOT_SCHEMA_VERSION }
    val clientShell = receiptShell ?: clientEvidence?.shellDigest
    if (clientShell != null) {
        if (clientShell == ShellIdentity.UNKNOWN || clientShell == backendShellDigest) return null
        return FrontendShellRefusal(
            clientShell = clientShell,
            backendShell = backendShellDigest,
            clientPluginVersion = clientEvidence?.pluginVersion ?: leasePluginVersion,
            clientPluginPath = clientEvidence?.pluginPath,
            backendPluginVersion = backendPluginVersion,
        )
    }
    if (clientEvidence?.legacyReleasePoller != true) return null
    // GH #440 review: a Client that names its plugin in its lease (GH #441) is 0.2.1053 or newer,
    // so its whole-package poll is a fallback (a 0.2.1055+ Client whose release offer went
    // unanswered), not proof of a shell before 0.2.1052. Its shell is unknown: no refusal.
    if (leasePluginVersion != null) return null
    return FrontendShellRefusal(
        clientShell = null,
        backendShell = backendShellDigest,
        backendPluginVersion = backendPluginVersion,
        legacyReleasePoller = true,
        legacyPackagePolls = clientEvidence.legacyPackagePolls,
    )
}

/**
 * GH #440 review (cycle 1): what the operator can check. Since GH #435 [FrontendReleaseDeliveryGate]
 * withholds the release from a Client on another shell, so that Client never receives it and never
 * logs the `[frontend-impl] refused:` line older backends caused; the backend's own idea.log has
 * the withholding instead.
 */
internal fun frontendShellRefusalStory(refusal: FrontendShellRefusal): String {
    val why = if (refusal.legacyReleasePoller) {
        "the JetBrains Client polled this backend's whole frontend release ${refusal.legacyPackagePolls} " +
            "time(s) without asking for its identity, which only Agent Doc Clients before " +
            "${FrontendShellRefusal.IDENTITY_POLL_FIRST_VERSION} do; their shell is older than this backend's, " +
            "so this backend withholds the release from that Client (backend idea.log: " +
            "`[frontend-impl] release withheld client=... legacy_poll=true`) and the Client cannot load it"
    } else {
        // GH #440 review (cycle 2): the backend logs one of its own lines for every such Client
        // (BackendSurfaceSnapshotRpcApi for a schema-5 receipt, BackendFrontendReleaseRpcApi for a
        // 0.2.1055+ build report); only Clients from 0.2.1055 on log a line of their own.
        // GH #440 review (cycle 3): the quotes carry no `client=` token, so neither an
        // `agent-doc upgrade` parser nor a grep for `client shell mismatch client=` /
        // `client plugin mismatch client=` takes this line for the backend's own.
        val client = refusal.clientShell?.take(12) ?: "..."
        val backend = refusal.backendShell.take(12)
        "the JetBrains Client runs a different Agent Doc shell than this backend, so this backend " +
            "withholds its frontend release from that Client and the Client cannot load it (backend " +
            "idea.log: `[frontend-impl] client shell mismatch ... client_shell=$client ... " +
            "backend_shell=$backend` or `[frontend-impl] client plugin mismatch ... " +
            "client_shell=$client ... backend_shell=$backend`; a Client from 0.2.1055 on also logs " +
            "`[frontend-impl] client plugin mismatch compatibility=... backend_shell=$backend " +
            "client_shell=$client` in its idea.log)"
    }
    return "$why. Asking for the schema again or reconnecting cannot change the Client's shell; only this " +
        "backend's plugin in the Client can"
}

/**
 * GH #435: the remedy for [mismatch]. A schema-1 lease is repaired by a new lease, which asks for
 * the schema again; only a Client log that names another plugin calls for the reinstall.
 */
internal fun frontendPluginMismatchRemedy(
    mismatch: FrontendPluginMismatch,
    version: String?,
    ideDataDir: String?,
    packageUrl: String? = null,
): String {
    val install = frontendPluginMismatchRemedy(version, ideDataDir, packageUrl)
    mismatch.shellRefusal?.let { refusal -> return frontendShellRefusalRemedy(refusal, version, ideDataDir, install) }
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
internal fun frontendPluginMismatchRemedy(version: String?, ideDataDir: String?, packageUrl: String? = null): String {
    val asset = (
        version?.takeIf { it.isNotBlank() && it != "unknown" }
            ?.let { "agent-doc-jetbrains-262-$it.zip" }
            ?: "agent-doc-jetbrains-262 ZIP of this backend's version"
        ) + (packageUrl?.takeIf { it.startsWith("https://") }?.let { " from $it" } ?: "")
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

/**
 * GH #440: the remedy for a Client that cannot load this backend's release. It leads with the version
 * comparison and the exact folder to replace, then the full install ([install]), then how the
 * Client log proves it worked. The text carries no angle brackets (the notification body is HTML).
 */
internal fun frontendShellRefusalRemedy(
    refusal: FrontendShellRefusal,
    version: String?,
    ideDataDir: String?,
    install: String,
): String {
    val backend = version?.takeIf { it.isNotBlank() && it != "unknown" } ?: refusal.backendPluginVersion
    val backendName = backend?.let { "this backend's $it" } ?: "this backend's plugin"
    val client = refusal.clientPluginVersion?.let { "Client plugin $it" }
        ?: if (refusal.legacyReleasePoller) {
            "the Client plugin (older than ${FrontendShellRefusal.IDENTITY_POLL_FIRST_VERSION})"
        } else {
            "the Client plugin"
        }
    val order = refusal.clientVersionOrder(backend)
    val comparison = when {
        order == null -> "$client is not $backendName"
        order < 0 -> "$client is older than $backendName"
        order == 0 -> "$client has the same version as $backendName but another shell"
        else -> "$client is newer than $backendName"
    }
    val ide = ideDataDir?.takeIf(String::isNotBlank) ?: "IntelliJIdea2026.3"
    val folder = refusal.clientPluginPath?.takeIf(String::isNotBlank)
        ?: "plugins/frontend/agent-doc-jetbrains-262 (~/.local/share/JetBrains/$ide/frontend/agent-doc-jetbrains-262 on Linux)"
    val verify = backend?.let { "`Loaded custom plugins: Agent Doc, $it` and " } ?: ""
    return "update the JetBrains Client plugin: $comparison. Replace the Client's $folder with " +
        "$backendName ZIP: $install. Afterwards the Client idea.log shows $verify`[frontend-impl] activated`"
}

/** GH #439: the backend IDE's data directory name (`IntelliJIdea2026.3`), from its config path. */
internal fun backendIdeDataDirName(): String? =
    runCatching { Path.of(PathManager.getConfigPath()).fileName?.toString() }.getOrNull()

internal fun currentFrontendPluginMismatchRemedy(): String =
    frontendPluginMismatchRemedy(runCatching { pluginVersion() }.getOrNull(), backendIdeDataDirName())

internal fun currentFrontendPluginMismatchRemedy(mismatch: FrontendPluginMismatch): String =
    frontendPluginMismatchRemedy(
        mismatch,
        runCatching { pluginVersion() }.getOrNull(),
        backendIdeDataDirName(),
        mismatch.shellRefusal?.let { runCatching { FrontendImplementationReleaseSource.packageUrl() }.getOrNull() },
    )

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
