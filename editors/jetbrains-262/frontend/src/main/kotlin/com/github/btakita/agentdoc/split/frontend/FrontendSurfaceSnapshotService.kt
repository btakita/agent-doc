@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.shell.receipt.SHELL_SPI_VERSION
import com.github.btakita.agentdoc.shell.receipt.ShellIdentity
import com.github.btakita.agentdoc.split.FrontendClientBuild
import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.FrontendReleaseGeneration
import com.github.btakita.agentdoc.split.FrontendReleaseOffer
import com.github.btakita.agentdoc.split.FrontendReleaseRpcApi
import com.github.btakita.agentdoc.split.FrontendShellCompatibility
import com.github.btakita.agentdoc.split.FrontendPresentationCapability
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendPresentationReceiptOutcome
import com.github.btakita.agentdoc.split.FrontendPresentationProjection
import com.github.btakita.agentdoc.split.GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.MAIN_SURFACE_PANE_ID
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi
import com.github.btakita.agentdoc.split.agentDocPluginDescriptorFact
import com.github.btakita.agentdoc.split.forSchema
import com.github.btakita.agentdoc.split.frontendLeaseInstanceId
import com.github.btakita.agentdoc.split.generation
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.project.projectId
import fleet.rpc.client.durable
import java.awt.AWTEvent
import java.awt.Toolkit
import java.awt.event.AWTEventListener
import java.net.URI
import java.util.IdentityHashMap
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

@Service(Service.Level.PROJECT)
class FrontendSurfaceSnapshotService(
    private val project: Project,
    private val coroutineScope: CoroutineScope,
) : Disposable {
    private val started = AtomicBoolean(false)
    private val sequence = AtomicLong(0)
    /** GH #441: names this frontend's plugin version, so a backend can tell provisional schema 1. */
    private val frontendInstanceId = frontendLeaseInstanceId(
        UUID.randomUUID().toString(),
        agentDocPluginDescriptorFact("getVersion"),
    )
    private val schemaNegotiationGate = SchemaNegotiationGate()
    private val surfaceIdentities = FrontendSurfaceIdentityTracker()
    private val presentationAdapter = Exact262DetachedPresentationAdapter.create(project, this)
    private val captures = MutableSharedFlow<Unit>(
        replay = 1,
        extraBufferCapacity = 1,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )
    private val awtListener = AWTEventListener { requestCapture() }

    fun start() {
        if (!started.compareAndSet(false, true)) return
        // GH #435: the first Client-side proof that `agent.doc.frontend` loaded for this project.
        LOG.info(
            "[split-surface] frontend surface publisher started project=${project.name} " +
                "plugin=${agentDocPluginDescriptorFact("getVersion") ?: "unknown"} " +
                "plugin_path=${agentDocPluginDescriptorFact("getPluginPath") ?: "unknown"} " +
                "instance=$frontendInstanceId schema_max=$SURFACE_SNAPSHOT_SCHEMA_VERSION " +
                "receipt=${FrontendActivationReceipt.line}",
        )

        project.messageBus.connect(this).subscribe(
            FileEditorManagerListener.FILE_EDITOR_MANAGER,
            object : FileEditorManagerListener {
                override fun fileOpened(source: FileEditorManager, file: VirtualFile) = requestCapture()
                override fun fileClosed(source: FileEditorManager, file: VirtualFile) = requestCapture()
                override fun selectionChanged(event: FileEditorManagerEvent) = requestCapture()
            },
        )
        Toolkit.getDefaultToolkit().addAWTEventListener(
            awtListener,
            AWTEvent.WINDOW_EVENT_MASK or
                AWTEvent.FOCUS_EVENT_MASK or
                AWTEvent.COMPONENT_EVENT_MASK or
                AWTEvent.HIERARCHY_EVENT_MASK,
        )

        coroutineScope.launch {
            durable {
                maintainIngress()
            }
        }
        coroutineScope.launch {
            pollFrontendImplementationReleases()
        }
        requestCapture()
    }

    private suspend fun pollFrontendImplementationReleases() {
        val poll = FrontendReleasePoll()
        runFrontendReleasePoll(
            gate = schemaNegotiationGate,
            gateTimeoutMs = FRONTEND_RELEASE_FETCH_NEGOTIATION_WAIT_MS,
            onGateTimeout = {
                LOG.info(
                    "[frontend-impl] no lease got a snapshot schema answer within " +
                        "${FRONTEND_RELEASE_FETCH_NEGOTIATION_WAIT_MS}ms; polling the backend release anyway",
                )
            },
            pollOnce = { pollFrontendImplementationReleaseOnce(poll) },
        )
    }

    /**
     * GH #435: one release poll. This Client names its own build to the additive
     * [FrontendReleaseRpcApi] and learns which generation the backend selected, never its bytes. A
     * Client whose shell is not the backend's logs both builds once and stops
     * there: it could only refuse the package. Otherwise the bytes are fetched once per new
     * generation; [runFrontendReleasePoll] already held the first poll until a lease's schema
     * negotiation was answered (GH #441, at most 30 s), so the transfer never races it. A
     * backend that predates [FrontendReleaseRpcApi] (up to 0.2.1054) is asked for the package
     * through the frozen API at most once per [FRONTEND_REFUSED_RELEASE_REFETCH_MS].
     */
    private suspend fun pollFrontendImplementationReleaseOnce(poll: FrontendReleasePoll) {
        try {
            pollFrontendImplementationRelease(poll)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            LOG.info(
                "[frontend-impl] backend release candidate unavailable " +
                    "(${failure.javaClass.simpleName}: ${failure.message})",
            )
        }
    }

    private suspend fun pollFrontendImplementationRelease(poll: FrontendReleasePoll) {
        val offer = askFrontendReleaseOffer()
        if (offer != null) {
            poll.offerLogLine(offer)?.let(LOG::warn)
            if (offer.compatibility != FrontendShellCompatibility.SAME_SHELL) return
        }
        val now = System.currentTimeMillis()
        val fetch = if (offer != null) {
            poll.shouldFetch(offer.release, FrontendReleaseActivation.receipt(), now)
        } else {
            poll.shouldFetchFromLegacyBackend(now)
        }
        if (!fetch) return
        SurfaceSnapshotRpcApi.getInstance().frontendImplementationRelease()?.let { release ->
            val active = FrontendReleaseActivation.receipt()
            if (active.implSha256 == release.implSha256 && active.epoch == release.epoch) return
            when (val outcome = activateFrontendImplementationRelease(release)) {
                is FrontendActivationOutcome.Activated -> {
                    LOG.info(
                        "[frontend-impl] activated digest=${outcome.receipt.implSha256} " +
                            "epoch=${outcome.receipt.epoch}",
                    )
                    requestCapture()
                }
                is FrontendActivationOutcome.Refused -> {
                    poll.refused(release.generation(), System.currentTimeMillis())
                    LOG.warn("[frontend-impl] refused: ${outcome.reason}")
                }
                is FrontendActivationOutcome.RestartRequired -> {
                    poll.refused(release.generation(), System.currentTimeMillis())
                    LOG.warn("[frontend-impl] restart required: ${outcome.reason}")
                }
                is FrontendActivationOutcome.RolledBack -> {
                    poll.refused(release.generation(), System.currentTimeMillis())
                    LOG.warn("[frontend-impl] rolled back: ${outcome.reason}")
                }
            }
        }
    }

    /** The backend's offer, or null when it predates [FrontendReleaseRpcApi] or did not answer. */
    private suspend fun askFrontendReleaseOffer(): FrontendReleaseOffer? = try {
        withTimeoutOrNull(FRONTEND_RELEASE_OFFER_TIMEOUT_MS) {
            FrontendReleaseRpcApi.getInstance().frontendReleaseOffer(currentFrontendClientBuild())
        }
    } catch (cancelled: CancellationException) {
        throw cancelled
    } catch (failure: Exception) {
        null
    }

    /**
     * A backend can accept the RPC and still complete the lease flow without emitting when the
     * frontend project id has not become resolvable there yet. `durable` reconnects transport
     * failures, but a normal empty completion is not a failure. Keep that startup race live with
     * bounded retries instead of permanently leaving two loaded plugin modules disconnected.
     */
    private suspend fun maintainIngress() {
        maintainFrontendIngress(
            openIngress = {
                SurfaceSnapshotRpcApi.getInstance().openIngress(project.projectId(), frontendInstanceId)
            },
            onLease = { lease ->
                LOG.info(
                    "[split-surface] frontend/backend ingress connected " +
                        "generation=${lease.connectionGeneration}",
                )
                publishForLease(lease)
            },
            onNormalCompletion = { observedLease, retryDelayMs ->
                LOG.warn(
                    "[split-surface] frontend/backend ingress completed " +
                        "lease_observed=$observedLease retry_in_ms=$retryDelayMs",
                )
            },
        )
    }

    private suspend fun publishForLease(lease: SurfaceIngressLease) {
        // GH #435: the schema belongs to this lease, but an unanswered negotiation is provisional:
        // a refusal asks the backend again instead of pinning schema 1 for the lease's lifetime.
        // GH #441: the first answer, from this ask or a re-ask after a refusal, opens the release
        // poll. An unanswered ask keeps the multi-megabyte package off the link so the re-ask can
        // get through.
        val negotiation = SnapshotSchemaNegotiation(schemaNegotiationGate, ::askBackendSnapshotSchema)
        negotiation.negotiate()
        LOG.info(negotiation.logLine())
        val outcomes = FrontendPublicationLog(lease.connectionGeneration)
        captures.collectLatest {
            // Coalesce the burst of file-editor, AWT hierarchy, and focus events produced by one
            // dock/undock operation, while still emitting a complete authoritative snapshot.
            delay(FRONTEND_CAPTURE_DEBOUNCE_MS)
            var retry = 0
            var refusals = 0
            do {
                val capture = captureOnEdt()
                val published = capture.snapshot.forSchema(negotiation.schema)
                val ack = SurfaceSnapshotRpcApi.getInstance()
                    .publishSnapshot(lease, published)
                if (ack.status != SurfaceIngressStatus.ACCEPTED) {
                    if (negotiation.renegotiateAfterRefusal()) {
                        // The backend answered this time: republish the same editor state at
                        // the schema it accepts (with geometry and the activation receipt).
                        LOG.info(negotiation.logLine() + " after_refusal=${ack.status}")
                        refusals = 0
                        continue
                    }
                    // GH #435: a refusal the backend can outgrow (its controller was busy, the
                    // project was not registered yet, a frame was not realized) is republished on
                    // a bounded backoff. Before, the snapshot waited for the next editor/AWT
                    // event, so a controller that recovered while the user was idle never
                    // received the measured layout a deferred route was waiting for.
                    val retryInMs = frontendSnapshotRetryDelayMillis(ack.status, refusals++)
                    outcomes.refused(ack, published, retryInMs)?.let(LOG::warn)
                    if (retryInMs == null) break
                    delay(retryInMs)
                    continue
                }
                outcomes.accepted(ack, published)?.let(LOG::info)
                refusals = 0
                val projection = ack.presentation ?: break
                val receipts = applyPresentationOnEdt(capture, projection)
                receipts.forEach { receipt ->
                    val receiptAck = SurfaceSnapshotRpcApi.getInstance()
                        .publishPresentationReceipt(lease, receipt)
                    if (receiptAck.status != SurfaceIngressStatus.ACCEPTED) {
                        LOG.warn(
                            "[split-surface] presentation receipt rejected " +
                                "status=${receiptAck.status} diagnostic=${receiptAck.diagnostic}",
                        )
                    }
                }
                if (!projection.retrySuggested || retry++ >= MAX_PRESENTATION_RECAPTURES) break
                delay(PRESENTATION_RECAPTURE_MS)
            } while (true)
            if (retry > MAX_PRESENTATION_RECAPTURES) {
                LOG.warn("[split-surface] bounded presentation recapture exhausted")
            }
        }
    }

    /**
     * Publish the frontend-authored compatibility columns (schema 3) only to a backend that
     * accepts them. Fleet RPC decodes with `ignoreUnknownKeys = false`; negotiation prevents older
     * peers from seeing new keys. The authenticated lease already owns structure while this call
     * is pending; native derives the live vector from the same snapshot's measured evidence.
     * Null when the backend gave no answer within [FRONTEND_SCHEMA_NEGOTIATION_TIMEOUT_MS] or
     * failed the call (a backend that predates negotiation).
     */
    private suspend fun askBackendSnapshotSchema(): Int? = try {
        withTimeoutOrNull(FRONTEND_SCHEMA_NEGOTIATION_TIMEOUT_MS) {
            SurfaceSnapshotRpcApi.getInstance().supportedSnapshotSchemaVersion()
        }.also { offered ->
            if (offered == null) {
                LOG.info(
                    "[split-surface] backend snapshot schema negotiation unanswered after " +
                        "${FRONTEND_SCHEMA_NEGOTIATION_TIMEOUT_MS}ms; publishing schema " +
                        "$LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION provisionally and asking again after a refusal",
                )
            }
        }
    } catch (cancelled: CancellationException) {
        throw cancelled
    } catch (failure: Exception) {
        LOG.info(
            "[split-surface] backend snapshot schema negotiation unavailable " +
                "(${failure.javaClass.simpleName}); publishing schema $LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION",
        )
        null
    }

    private suspend fun captureOnEdt(): CapturedFrontendSurfaces {
        val result = CompletableDeferred<CapturedFrontendSurfaces>()
        ApplicationManager.getApplication().invokeLater {
            try {
                result.complete(
                    FrontendSurfaceCollector.captureWithTargets(
                        project = project,
                        frontendInstanceId = frontendInstanceId,
                        sequence = sequence.incrementAndGet(),
                        surfaceIdentities = surfaceIdentities,
                    ).let { capture ->
                        capture.copy(
                            snapshot = capture.snapshot.copy(
                                presentationCapability = presentationAdapter?.capability
                                    ?: FrontendPresentationCapability.SNAPSHOT_ONLY,
                                activationReceipt = FrontendActivationReceipt.line,
                            ),
                        )
                    },
                )
            } catch (failure: Throwable) {
                result.completeExceptionally(failure)
            }
        }
        return result.await()
    }

    private suspend fun applyPresentationOnEdt(
        capture: CapturedFrontendSurfaces,
        projection: FrontendPresentationProjection,
    ): List<FrontendPresentationReceipt> {
        val result = CompletableDeferred<List<FrontendPresentationReceipt>>()
        ApplicationManager.getApplication().invokeLater {
            try {
                val adapter = presentationAdapter
                result.complete(
                    if (adapter != null) {
                        adapter.apply(capture, projection)
                    } else {
                        projection.presentations.map { presentation ->
                            FrontendPresentationReceipt(
                                projectId = project.projectId(),
                                identity = presentation.identity,
                                presentationRevision = presentation.presentationRevision,
                                kind = presentation.kind,
                                document = presentation.document,
                                viewSession = presentation.viewSession,
                                outcome = FrontendPresentationReceiptOutcome.REFUSED,
                                diagnostic = "exact-262 detached presentation API shape unavailable",
                            )
                        }
                    },
                )
            } catch (failure: Throwable) {
                LOG.warn("[split-surface] presentation apply failed closed", failure)
                result.complete(
                    projection.presentations.map { presentation ->
                        FrontendPresentationReceipt(
                            projectId = project.projectId(),
                            identity = presentation.identity,
                            presentationRevision = presentation.presentationRevision,
                            kind = presentation.kind,
                            document = presentation.document,
                            viewSession = presentation.viewSession,
                            outcome = FrontendPresentationReceiptOutcome.REFUSED,
                            diagnostic = failure.message ?: failure.javaClass.simpleName,
                        )
                    },
                )
            }
        }
        return result.await()
    }

    private fun requestCapture() {
        captures.tryEmit(Unit)
    }

    override fun dispose() {
        if (started.get()) {
            Toolkit.getDefaultToolkit().removeAWTEventListener(awtListener)
        }
    }

    companion object {
        private const val PRESENTATION_RECAPTURE_MS = 250L
        private const val MAX_PRESENTATION_RECAPTURES = 60
        private val LOG = Logger.getInstance(FrontendSurfaceSnapshotService::class.java)

        fun getInstance(project: Project): FrontendSurfaceSnapshotService = project.service()
    }
}

/** Intentional event-burst quiet window before the next authoritative snapshot capture. */
internal const val FRONTEND_CAPTURE_DEBOUNCE_MS = 75L
internal const val FRONTEND_IMPLEMENTATION_POLL_MS = 30_000L

/** Hang guard for first-lease schema negotiation; authority is already pending during this wait. */
internal const val FRONTEND_SCHEMA_NEGOTIATION_TIMEOUT_MS = 5_000L

/**
 * GH #441: the longest the first release poll waits for a lease to negotiate its snapshot schema
 * (a backend project that is not resolvable yet opens no lease) before it polls anyway.
 */
internal const val FRONTEND_RELEASE_FETCH_NEGOTIATION_WAIT_MS = 30_000L

/**
 * GH #441: settles once a lease's schema negotiation is answered, by its first ask or by a re-ask
 * after a refusal. An unanswered ask does not settle it; the release poll's cap
 * ([FRONTEND_RELEASE_FETCH_NEGOTIATION_WAIT_MS]) bounds the wait.
 *
 * On a cold start the release poll and the first lease started together, and the poll's
 * multi-megabyte package fetch occupied the Remote Dev link while the lease asked for the schema.
 * The backend answers that ask from a constant, yet the answer queued behind the package, the 5 s
 * negotiation went unanswered, and the first snapshot was a provisional schema 1.
 *
 * Review cycle 1: the gate used to settle after an unanswered ask too. When the first answer
 * reached the Client after 5 s, the package then started, the re-ask after the refusal queued
 * behind it, and the next schema 1 made the backend show the false reinstall notice.
 */
internal class SchemaNegotiationGate {
    private val settled = CompletableDeferred<Unit>()

    fun settle() {
        settled.complete(Unit)
    }

    /** True when the negotiation settled within [timeoutMs]. */
    suspend fun await(timeoutMs: Long): Boolean = withTimeoutOrNull(timeoutMs) { settled.await() } != null
}

/**
 * GH #441: the frontend release poll. Its first poll, which may fetch the whole package, waits for
 * [gate] (at most [gateTimeoutMs]), so a lease's schema ask, or its re-ask after a refusal, never
 * queues behind it.
 */
internal suspend fun runFrontendReleasePoll(
    gate: SchemaNegotiationGate,
    gateTimeoutMs: Long,
    onGateTimeout: () -> Unit,
    pollOnce: suspend () -> Unit,
    sleep: suspend (Long) -> Unit = { delay(it) },
) {
    if (!gate.await(gateTimeoutMs)) onGateTimeout()
    while (true) {
        pollOnce()
        sleep(FRONTEND_IMPLEMENTATION_POLL_MS)
    }
}

/**
 * GH #435: the bounded republish delay after [status] refused a snapshot, or null when another
 * capture of the same editor state cannot succeed (a stale lease or sequence, or a malformed
 * snapshot) and only a new editor event or lease should publish again.
 */
internal fun frontendSnapshotRetryDelayMillis(status: SurfaceIngressStatus, consecutiveRefusals: Int): Long? =
    when (status) {
        SurfaceIngressStatus.DELIVERY_FAILED,
        SurfaceIngressStatus.PROJECT_MISMATCH,
        SurfaceIngressStatus.INCOMPLETE,
        -> frontendIngressRetryDelayMillis(consecutiveRefusals)
        else -> null
    }

/**
 * GH #435: the frontend half of the publication log. One line when a lease's first snapshot is
 * accepted or a refusal clears, and one per change of refusal (status + diagnostic), so the
 * Client's idea.log shows whether geometry left this process and why the backend refused it
 * without one line per AWT event.
 */
internal class FrontendPublicationLog(private val connectionGeneration: Long) {
    private var acceptedOnce = false
    private var lastRefusal: String? = null
    private var repeatedRefusals = 0

    fun accepted(ack: SurfaceIngressAck, snapshot: FrontendSurfaceSnapshot): String? {
        val first = !acceptedOnce
        val cleared = lastRefusal
        acceptedOnce = true
        lastRefusal = null
        repeatedRefusals = 0
        if (!first && cleared == null) return null
        return "[split-surface] snapshot accepted generation=$connectionGeneration " +
            "sequence=${ack.acceptedSequence ?: snapshot.sequence} schema=${snapshot.schemaVersion} " +
            "surfaces=${snapshot.surfaces.size} measured_windows=${measuredWindows(snapshot)} " +
            "first=$first after_refusal=${cleared ?: "none"}"
    }

    fun refused(ack: SurfaceIngressAck, snapshot: FrontendSurfaceSnapshot, retryInMs: Long?): String? {
        val refusal = "${ack.status}:${ack.diagnostic ?: ""}"
        if (refusal == lastRefusal) {
            repeatedRefusals++
            return null
        }
        val repeated = repeatedRefusals
        lastRefusal = refusal
        repeatedRefusals = 0
        return "[split-surface] snapshot rejected status=${ack.status} generation=${ack.connectionGeneration} " +
            "sequence=${snapshot.sequence} measured_windows=${measuredWindows(snapshot)} " +
            "retry_in_ms=${retryInMs ?: "none(next_editor_event)"} previous_repeats=$repeated " +
            "diagnostic=${ack.diagnostic}"
    }

    private fun measuredWindows(snapshot: FrontendSurfaceSnapshot): String {
        val windows = snapshot.surfaces.flatMap { it.windows }
        return "${windows.count { it.bounds != null }}/${windows.size}"
    }
}

/** GH #435: this Client's own Agent Doc frontend build, named in every release offer request. */
internal fun currentFrontendClientBuild(): FrontendClientBuild = FrontendClientBuild(
    pluginVersion = agentDocPluginDescriptorFact("getVersion") ?: "unknown",
    pluginPath = agentDocPluginDescriptorFact("getPluginPath") ?: "unknown",
    shellDigest = ShellIdentity.shellDigest,
    spiVersion = SHELL_SPI_VERSION,
)

/** Retry a normally completed, lease-less RPC flow without spinning either IDE process. */
internal fun frontendIngressRetryDelayMillis(consecutiveEmptyCompletions: Int): Long {
    val exponent = consecutiveEmptyCompletions.coerceIn(0, 5)
    return (1_000L * (1L shl exponent)).coerceAtMost(30_000L)
}

internal suspend fun maintainFrontendIngress(
    openIngress: suspend () -> Flow<SurfaceIngressLease>,
    onLease: suspend (SurfaceIngressLease) -> Unit,
    onNormalCompletion: (observedLease: Boolean, retryDelayMs: Long) -> Unit,
    sleep: suspend (Long) -> Unit = { delay(it) },
) {
    var consecutiveEmptyCompletions = 0
    while (true) {
        var observedLease = false
        openIngress().collectLatest { lease ->
            observedLease = true
            consecutiveEmptyCompletions = 0
            onLease(lease)
        }

        val retryDelayMs = frontendIngressRetryDelayMillis(consecutiveEmptyCompletions)
        if (!observedLease) consecutiveEmptyCompletions++
        onNormalCompletion(observedLease, retryDelayMs)
        sleep(retryDelayMs)
    }
}

/**
 * The snapshot schema to publish, from the backend's advertised newest schema (null when the
 * backend predates negotiation). Never above what this frontend writes, never below schema 1.
 */
/** This JVM's current frontend activation receipt, published in every schema-5 snapshot. */
internal object FrontendActivationReceipt {
    val line: String get() = FrontendReleaseActivation.receipt().encode()
}

internal fun activateFrontendImplementationRelease(
    release: FrontendImplementationRelease,
): FrontendActivationOutcome = FrontendReleaseActivation.activate(
    request = FrontendReleaseRequest(
        releaseVersion = release.releaseVersion,
        packageUrl = URI(release.packageUrl),
        signatureUrl = URI(release.signatureUrl),
        implEntry = release.implEntry,
        implSha256 = release.implSha256,
        shellDigest = release.shellDigest,
        spiVersion = release.spiVersion,
        epoch = release.epoch,
    ),
    pluginZip = release.packageBytes,
    detachedSignature = release.signatureBytes,
)

internal fun negotiatedSnapshotSchema(backendOffered: Int?): Int =
    (backendOffered ?: LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION)
        .coerceIn(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, SURFACE_SNAPSHOT_SCHEMA_VERSION)

/**
 * GH #435: the snapshot schema of one ingress lease. An answer from the backend is final for the
 * lease. No answer (the 5 s negotiation timed out, or the backend failed the call) is
 * provisional: the lease publishes schema 1 meanwhile, and every refused publication asks again.
 *
 * Before, one unanswered negotiation pinned schema 1 for the whole lease. A schema-1 snapshot
 * carries no activation receipt, so a backend with an activated implementation generation refused
 * every snapshot (`frontend_authority_absent_or_generation_mismatched`) until the Client
 * reconnected, and reported an "older Agent Doc frontend" the Client did not have.
 */
internal class SnapshotSchemaNegotiation(
    /** GH #441: settled by the first answer, so the release poll moves bytes only after one. */
    private val answeredGate: SchemaNegotiationGate? = null,
    private val ask: suspend () -> Int?,
) {
    @Volatile
    var schema: Int = LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
        private set

    @Volatile
    private var backendOffered: Int? = null

    val answered: Boolean get() = backendOffered != null

    /** Ask the backend; returns the schema to publish. */
    suspend fun negotiate(): Int {
        ask()?.let {
            backendOffered = it
            answeredGate?.settle()
        }
        schema = negotiatedSnapshotSchema(backendOffered)
        return schema
    }

    /** After a refused publication, ask again while unanswered; true when the schema changed. */
    suspend fun renegotiateAfterRefusal(): Boolean {
        if (answered) return false
        val before = schema
        negotiate()
        return schema != before
    }

    fun logLine(): String =
        "[split-surface] snapshot schema negotiated version=$schema backend_offered=${backendOffered ?: "none"} " +
            "geometry=${if (schema >= GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION) "measured" else "unavailable"} " +
            "provisional=${!answered}"
}

/** GH #435: how long the Client waits for [FrontendReleaseRpcApi] before treating the backend as older. */
internal const val FRONTEND_RELEASE_OFFER_TIMEOUT_MS = 10_000L

/** GH #435: how long a generation the Client refused waits before its package is fetched again. */
internal const val FRONTEND_REFUSED_RELEASE_REFETCH_MS = 10L * 60 * 1000

/**
 * GH #435: whether the 30 s release poll moves the backend's package bytes. Only a generation
 * this Client has not activated is fetched, and one it refused (no client trust yet, a shell
 * mismatch) is fetched again only after [FRONTEND_REFUSED_RELEASE_REFETCH_MS], so a missing
 * trust file added later still activates without a restart.
 */
internal class FrontendReleasePoll {
    private var refused: FrontendReleaseGeneration? = null
    private var refusedAtMs: Long = 0
    private var legacyFetchedAtMs: Long? = null
    private var lastOfferReport: String? = null

    /**
     * GH #435: the Client-side mismatch facts for [offer], once per distinct
     * mismatch (and once when it clears), so a 30 s poll does not repeat it.
     */
    fun offerLogLine(offer: FrontendReleaseOffer): String? {
        val report = if (offer.compatibility == FrontendShellCompatibility.SAME_SHELL) {
            null
        } else {
            "[frontend-impl] client plugin mismatch compatibility=${offer.compatibility.name.lowercase()} " +
                "backend_plugin=${offer.backendPluginVersion} backend_plugin_path=${offer.backendPluginPath} " +
                "backend_shell=${offer.backendShellDigest.take(12)} client_shell=${ShellIdentity.shellDigest.take(12)}"
        }
        if (report == lastOfferReport) return null
        val cleared = lastOfferReport != null && report == null
        lastOfferReport = report
        return report ?: if (cleared) {
            "[frontend-impl] client plugin matches the backend ${offer.backendPluginVersion} again"
        } else {
            null
        }
    }

    /**
     * GH #435: a backend up to 0.2.1054 has no offer API, so its whole package is the only way to
     * learn the generation; ask at most once per [FRONTEND_REFUSED_RELEASE_REFETCH_MS].
     */
    fun shouldFetchFromLegacyBackend(nowMs: Long): Boolean {
        val last = legacyFetchedAtMs
        if (last != null && nowMs - last < FRONTEND_REFUSED_RELEASE_REFETCH_MS) return false
        legacyFetchedAtMs = nowMs
        return true
    }

    fun shouldFetch(
        identity: FrontendReleaseGeneration?,
        active: ActivationReceipt,
        nowMs: Long,
    ): Boolean {
        if (identity == null) return false
        if (active.implSha256 == identity.implSha256 && active.epoch == identity.epoch) return false
        return identity != refused || nowMs - refusedAtMs >= FRONTEND_REFUSED_RELEASE_REFETCH_MS
    }

    fun refused(identity: FrontendReleaseGeneration, nowMs: Long) {
        refused = identity
        refusedAtMs = nowMs
    }
}

/**
 * Assigns identity and incarnation from public AWT/WindowManager evidence before RPC.  IDs are
 * scoped by the authenticated frontend connection; replacing or reopening a top-level Window
 * allocates a new generation. Incomplete captures never retire state because they cannot prove
 * absence and are rejected by the backend.
 */
internal class FrontendSurfaceIdentityTracker {
    private var nextDetachedId = 0L
    private var nextGeneration = 0L
    private val active = IdentityHashMap<Any, SurfaceIdentity>()

    fun assign(
        frames: Set<Any>,
        mainFrame: Any?,
        complete: Boolean,
    ): Map<Any, SurfaceIdentity> {
        if (complete) {
            active.keys.removeIf { candidate -> frames.none { it === candidate } }
        }
        return frames.associateWith { frame ->
            active[frame] ?: SurfaceIdentity(
                paneId = if (frame === mainFrame) MAIN_SURFACE_PANE_ID else "detached-${++nextDetachedId}",
                surfaceGeneration = ++nextGeneration,
                role = if (frame === mainFrame) SurfaceRole.MAIN else SurfaceRole.DETACHED,
            ).also { active[frame] = it }
        }
    }

    data class SurfaceIdentity(
        val paneId: String,
        val surfaceGeneration: Long,
        val role: SurfaceRole,
    )
}
