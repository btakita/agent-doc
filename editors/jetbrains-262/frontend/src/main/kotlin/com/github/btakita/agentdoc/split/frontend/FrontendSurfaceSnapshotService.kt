@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
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
import com.intellij.openapi.fileEditor.impl.EditorWindow
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.WindowManager
import com.intellij.platform.project.projectId
import fleet.rpc.client.durable
import java.awt.AWTEvent
import java.awt.Component
import java.awt.KeyboardFocusManager
import java.awt.Point
import java.awt.Toolkit
import java.awt.Window
import java.awt.event.AWTEventListener
import java.util.IdentityHashMap
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import javax.swing.SwingUtilities
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
            val snapshot = captureOnEdt()
            val ack = SurfaceSnapshotRpcApi.getInstance().publishSnapshot(lease, snapshot)
            if (ack.status != SurfaceIngressStatus.ACCEPTED) {
                LOG.warn(
                    "[split-surface] snapshot rejected status=${ack.status} " +
                        "generation=${ack.connectionGeneration} diagnostic=${ack.diagnostic}",
                )
            }
        }
    }

    private suspend fun captureOnEdt(): FrontendSurfaceSnapshot {
        val result = CompletableDeferred<FrontendSurfaceSnapshot>()
        ApplicationManager.getApplication().invokeLater {
            try {
                result.complete(
                    FrontendSurfaceCollector.capture(
                        project = project,
                        frontendInstanceId = frontendInstanceId,
                        sequence = sequence.incrementAndGet(),
                        surfaceIdentities = surfaceIdentities,
                    ),
                )
            } catch (failure: Throwable) {
                result.completeExceptionally(failure)
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
        private val LOG = Logger.getInstance(FrontendSurfaceSnapshotService::class.java)

        fun getInstance(project: Project): FrontendSurfaceSnapshotService = project.service()
    }
}

internal object FrontendSurfaceCollector {
    fun capture(
        project: Project,
        frontendInstanceId: String,
        sequence: Long,
        surfaceIdentities: FrontendSurfaceIdentityTracker,
    ): FrontendSurfaceSnapshot {
        check(SwingUtilities.isEventDispatchThread()) { "frontend surface capture must run on EDT" }
        val activeFrame = KeyboardFocusManager.getCurrentKeyboardFocusManager().activeWindow
        val mainFrame = WindowManager.getInstance().getFrame(project)
        val editorWindows = FileEditorManagerEx.getInstanceEx(project).windows.toList()
        val tagged = editorWindows.mapNotNull { editorWindow ->
            val component = editorWindow.tabbedPane.component
            val frame = SwingUtilities.getWindowAncestor(component) ?: return@mapNotNull null
            TaggedEditorWindow(
                frame = frame,
                position = componentPosition(component),
                snapshot = editorWindowSnapshot(editorWindow),
            )
        }
        val complete = mainFrame != null && tagged.size == editorWindows.size
        val grouped = tagged.groupBy(TaggedEditorWindow::frame).toMutableMap()
        if (mainFrame != null) grouped.putIfAbsent(mainFrame, emptyList())
        val assignments = surfaceIdentities.assign(
            frames = grouped.keys,
            mainFrame = mainFrame,
            complete = complete,
        )
        val surfaces = grouped
            .map { (frame, surfaceWindows) ->
                val identity = assignments.getValue(frame)
                FrontendSurface(
                    paneId = identity.paneId,
                    surfaceGeneration = identity.surfaceGeneration,
                    role = identity.role,
                    focused = frame === activeFrame,
                    windows = surfaceWindows
                        .sortedWith(compareBy({ it.position.x }, { it.position.y }))
                        .mapIndexed { ordinal, taggedWindow ->
                            taggedWindow.snapshot.copy(ordinal = ordinal)
                        },
                )
            }
            .sortedWith(compareBy({ it.role != SurfaceRole.MAIN }, FrontendSurface::paneId))
        return FrontendSurfaceSnapshot(
            frontendInstanceId = frontendInstanceId,
            sequence = sequence,
            projectId = project.projectId(),
            complete = complete,
            surfaces = surfaces,
        )
    }

    internal fun roleForPaneId(paneId: String): SurfaceRole =
        if (paneId == MAIN_SURFACE_PANE_ID) SurfaceRole.MAIN else SurfaceRole.DETACHED

    private fun editorWindowSnapshot(window: EditorWindow): EditorWindowSnapshot {
        val openPaths = window.fileList.map(VirtualFile::getPath).distinct()
        val selectedPath = window.selectedFile?.path
        return EditorWindowSnapshot(
            ordinal = 0,
            selectedPath = selectedPath,
            openPaths = openPaths,
            visiblePaths = listOfNotNull(selectedPath),
        )
    }

    private fun componentPosition(component: Component): Point =
        try {
            component.locationOnScreen
        } catch (_: IllegalStateException) {
            component.location
        }

    private data class TaggedEditorWindow(
        val frame: Window,
        val position: Point,
        val snapshot: EditorWindowSnapshot,
    )
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
