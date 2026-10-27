@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationCapability
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendPresentationReceiptOutcome
import com.github.btakita.agentdoc.split.FrontendPresentationProjection
import com.github.btakita.agentdoc.split.GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.MAIN_SURFACE_PANE_ID
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi
import com.github.btakita.agentdoc.split.forSchema
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
    private val frontendInstanceId = UUID.randomUUID().toString()
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
        FrontendActivationReceipt.line

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
        requestCapture()
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
        val schema = negotiateSnapshotSchema()
        captures.collectLatest {
            // Coalesce the burst of file-editor, AWT hierarchy, and focus events produced by one
            // dock/undock operation, while still emitting a complete authoritative snapshot.
            delay(FRONTEND_CAPTURE_DEBOUNCE_MS)
            var retry = 0
            do {
                val capture = captureOnEdt()
                val ack = SurfaceSnapshotRpcApi.getInstance()
                    .publishSnapshot(lease, capture.snapshot.forSchema(schema))
                if (ack.status != SurfaceIngressStatus.ACCEPTED) {
                    LOG.warn(
                        "[split-surface] snapshot rejected status=${ack.status} " +
                            "generation=${ack.connectionGeneration} diagnostic=${ack.diagnostic}",
                    )
                    break
                }
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
     */
    private suspend fun negotiateSnapshotSchema(): Int {
        val offered = try {
            withTimeoutOrNull(FRONTEND_SCHEMA_NEGOTIATION_TIMEOUT_MS) {
                SurfaceSnapshotRpcApi.getInstance().supportedSnapshotSchemaVersion()
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
        val schema = negotiatedSnapshotSchema(offered)
        LOG.info(
            "[split-surface] snapshot schema negotiated version=$schema backend_offered=${offered ?: "none"} " +
                "geometry=${if (schema >= GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION) "measured" else "unavailable"}",
        )
        return schema
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

/** Hang guard for first-lease schema negotiation; authority is already pending during this wait. */
internal const val FRONTEND_SCHEMA_NEGOTIATION_TIMEOUT_MS = 5_000L

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
/**
 * GH #339 R1: this JVM's frontend activation receipt. The frontend is shell-only
 * (`SHELL_ONLY`), so it reports no implementation generation; the line is logged once per JVM
 * and travels to the backend in every schema-5 snapshot.
 */
internal object FrontendActivationReceipt {
    val line: String by lazy {
        ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND).encode().also { receipt ->
            Logger.getInstance(FrontendActivationReceipt::class.java).info("[shell] $receipt")
        }
    }
}

internal fun negotiatedSnapshotSchema(backendOffered: Int?): Int =
    (backendOffered ?: LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION)
        .coerceIn(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, SURFACE_SNAPSHOT_SCHEMA_VERSION)

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
