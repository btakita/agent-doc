@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationCapability
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendPresentationReceiptOutcome
import com.github.btakita.agentdoc.split.FrontendPresentationProjection
import com.github.btakita.agentdoc.split.MAIN_SURFACE_PANE_ID
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi
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
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

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
                SurfaceSnapshotRpcApi.getInstance()
                    .openIngress(project.projectId(), frontendInstanceId)
                    .collectLatest(::publishForLease)
            }
        }
        requestCapture()
    }

    private suspend fun publishForLease(lease: SurfaceIngressLease) {
        captures.collectLatest {
            // Coalesce the burst of file-editor, AWT hierarchy, and focus events produced by one
            // dock/undock operation, while still emitting a complete authoritative snapshot.
            delay(CAPTURE_DEBOUNCE_MS)
            var retry = 0
            do {
                val capture = captureOnEdt()
                val ack = SurfaceSnapshotRpcApi.getInstance().publishSnapshot(lease, capture.snapshot)
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
        private const val CAPTURE_DEBOUNCE_MS = 75L
        private const val PRESENTATION_RECAPTURE_MS = 250L
        private const val MAX_PRESENTATION_RECAPTURES = 60
        private val LOG = Logger.getInstance(FrontendSurfaceSnapshotService::class.java)

        fun getInstance(project: Project): FrontendSurfaceSnapshotService = project.service()
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
