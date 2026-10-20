@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.AgentDocSessionFiles
import com.github.btakita.agentdoc.CpRouteClient
import com.github.btakita.agentdoc.EditorLayout
import com.github.btakita.agentdoc.EditorTabSyncListener
import com.github.btakita.agentdoc.LayoutColumn
import com.github.btakita.agentdoc.LayoutDetector
import com.github.btakita.agentdoc.RemoteClientLayoutAuthority
import com.github.btakita.agentdoc.SyncLayoutAction
import com.github.btakita.agentdoc.TerminalUtil
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.openapi.client.ClientKind
import com.intellij.openapi.client.ClientSessionsManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * GH #320 option B: the main window's layout as the JetBrains Client measured it.
 *
 * The frontend publishes each editor window's on-screen bounds (state). The backend derives the
 * left-to-right columns from them with the classic grouping
 * ([LayoutDetector.buildColumnsFromSnapshots]) and its order provenance
 * ([LayoutDetector.measuredColumnOrder]); nothing is inferred from editor events.
 */
internal sealed interface MeasuredMainLayout {
    /** Columns (absolute paths) in measured left-to-right order, plus the selected document. */
    data class Measured(val columns: List<List<String>>, val focused: String) : MeasuredMainLayout {
        val visible: List<String>
            get() = columns.flatten().distinct()
    }

    /** No measured order this snapshot; the last forwarded layout stays in force. */
    data class Unavailable(val reason: String) : MeasuredMainLayout
}

internal object RemoteClientMeasuredLayout {
    fun derive(
        snapshot: FrontendSurfaceSnapshot,
        isSessionDocument: (String) -> Boolean,
    ): MeasuredMainLayout {
        if (snapshot.schemaVersion < SURFACE_SNAPSHOT_SCHEMA_VERSION) {
            return MeasuredMainLayout.Unavailable("schema_${snapshot.schemaVersion}_without_geometry")
        }
        val main = snapshot.surfaces.singleOrNull { it.role == SurfaceRole.MAIN }
            ?: return MeasuredMainLayout.Unavailable("no_main_surface")
        val measured = mutableListOf<LayoutDetector.LayoutWindowSnapshot>()
        for (window in main.windows.sortedBy { it.ordinal }) {
            val selected = window.selectedPath?.takeIf(isSessionDocument)
            if (selected == null) {
                // `#stickymdpane`: a split showing a source tab next to its session documents is
                // still on screen; its column is unknown, not gone. Keep the last forwarded layout
                // rather than collapse it. A window that holds no session document contributes no
                // column, so closing a split still collapses the layout.
                if (window.openPaths.any(isSessionDocument)) {
                    return MeasuredMainLayout.Unavailable("split_shows_non_session_tab")
                }
                continue
            }
            val bounds = window.bounds ?: return MeasuredMainLayout.Unavailable("window_unmeasured")
            measured += LayoutDetector.LayoutWindowSnapshot(bounds.x, bounds.y, selected)
        }
        if (measured.isEmpty()) return MeasuredMainLayout.Unavailable("no_session_documents")
        if (LayoutDetector.measuredColumnOrder(measured) != "editor") {
            return MeasuredMainLayout.Unavailable("shared_origin")
        }
        val columns = LayoutDetector.buildColumnsFromSnapshots(measured)
            .map { column -> column.files.filter(String::isNotBlank).distinct() }
            .filter(List<String>::isNotEmpty)
        if (columns.isEmpty()) return MeasuredMainLayout.Unavailable("no_session_documents")
        val focused = measured.firstNotNullOf { it.file }
        return MeasuredMainLayout.Measured(columns, focused)
    }
}

/** The controller surface a measured layout publishes: its root and the `editor_surface` payload. */
internal data class ForwardedRemoteSurface(val projectRoot: String, val surfaceJson: String)

internal object RemoteClientSurfacePayload {
    private val GSON = com.google.gson.Gson()

    /**
     * Same root choice and path normalization as the classic surface ([SyncLayoutAction]); the
     * order provenance is `editor` because the columns came from measured geometry. The forward
     * owns pane membership and order only (`preserve_focus`): the snapshot names each split's
     * selected document but not which split has keyboard focus, so it must not move tmux focus.
     */
    fun build(basePath: String?, layout: MeasuredMainLayout.Measured): ForwardedRemoteSurface {
        val focusedRoot = TerminalUtil.resolveProjectPath(basePath, layout.focused).first
        val projectRoot = SyncLayoutAction.chooseSyncProjectRoot(basePath, focusedRoot, layout.visible)
        val editorLayout = SyncLayoutAction.absolutizeEditorLayout(
            projectRoot,
            SyncLayoutAction.normalizeEditorLayout(
                basePath,
                projectRoot,
                EditorLayout(layout.columns.map(::LayoutColumn), columnOrder = "editor"),
            ),
        )
        val surface = EditorTabSyncListener.SurfaceReport.buildSurface(
            focusedFile = layout.focused,
            visibleMdFiles = layout.visible,
            openMdFiles = layout.visible,
            editorLayout = editorLayout,
            forceReconcile = false,
            preserveFocus = true,
        )
        return ForwardedRemoteSurface(projectRoot, GSON.toJson(surface))
    }
}

/**
 * Forwards an accepted frontend snapshot's measured main-window layout into the controller's
 * pane-layout surface Source (`editor_surface_observe`). The pane-tagged
 * `editor_view_snapshot_observe` request for the same snapshot has already been delivered, so the
 * main/detached policy sees the complete snapshot before any layout effect.
 */
internal interface RemoteClientLayoutForwarder {
    fun forward(clientId: String, generation: Long, snapshot: FrontendSurfaceSnapshot)

    fun retire(clientId: String, generation: Long)

    object Disabled : RemoteClientLayoutForwarder {
        override fun forward(clientId: String, generation: Long, snapshot: FrontendSurfaceSnapshot) = Unit

        override fun retire(clientId: String, generation: Long) = Unit
    }
}

/** One endpoint per authenticated frontend connection, so a reconnect is never fenced. */
internal fun remoteClientSurfaceId(clientId: String, generation: Long): String =
    "remote-client:$clientId:$generation"

/**
 * Delivery happens on one serialized worker so a slow controller never holds the ingress lock,
 * and forwards and retirements stay in arrival order.
 */
internal class ControllerRemoteClientLayoutForwarder(private val project: Project) : RemoteClientLayoutForwarder {
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "agent-doc-remote-client-layout").apply { isDaemon = true }
    }

    /** Endpoints whose connection is still live; read by the transport's re-registration check. */
    private val liveSurfaces = ConcurrentHashMap.newKeySet<String>()

    // Worker-confined state.
    private val forwarded = mutableMapOf<String, ForwardedRemoteSurface>()
    private val lastUnavailable = mutableMapOf<String, String>()
    private val familyRetiredRoots = mutableSetOf<String>()

    override fun forward(clientId: String, generation: Long, snapshot: FrontendSurfaceSnapshot) {
        val surfaceId = remoteClientSurfaceId(clientId, generation)
        liveSurfaces += surfaceId
        execute { deliver(clientId, surfaceId, snapshot) }
    }

    override fun retire(clientId: String, generation: Long) {
        val surfaceId = remoteClientSurfaceId(clientId, generation)
        liveSurfaces -= surfaceId
        execute { retireSurface(surfaceId, reason = "frontend_connection_retired") }
    }

    fun dispose() {
        liveSurfaces.clear()
        execute { forwarded.keys.toList().forEach { retireSurface(it, reason = "backend_disposed") } }
        executor.shutdown()
    }

    private fun execute(task: () -> Unit) {
        try {
            executor.execute {
                try {
                    task()
                } catch (failure: Exception) {
                    LOG.warn("[rd-layout] measured layout forward failed: ${failure.message}", failure)
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            // Disposed: the project is closing and its forwarded surfaces are being retired.
        }
    }

    private fun deliver(clientId: String, surfaceId: String, snapshot: FrontendSurfaceSnapshot) {
        if (surfaceId !in liveSurfaces || project.isDisposed) return
        if (!isRemoteClient(clientId)) {
            // A monolithic IDE's own frontend: the classic window path already measures this
            // process's splits, so there is nothing to forward.
            noteUnavailable(surfaceId, "local_client")
            return
        }
        val layout = when (val derived = RemoteClientMeasuredLayout.derive(snapshot, ::isSessionDocument)) {
            is MeasuredMainLayout.Unavailable -> {
                noteUnavailable(surfaceId, derived.reason)
                return
            }
            is MeasuredMainLayout.Measured -> derived
        }
        lastUnavailable.remove(surfaceId)
        val pending = RemoteClientSurfacePayload.build(project.basePath, layout)
        val previous = forwarded[surfaceId]
        if (previous == pending) return
        if (previous != null && previous.projectRoot != pending.projectRoot) {
            CpRouteClient.forgetRemoteClientSurface(previous.projectRoot, surfaceId)
        }
        if (familyRetiredRoots.add(pending.projectRoot)) {
            // A previous backend JVM's forwarded surfaces on this root are dead frames; retire the
            // installation's `jetbrains-rd` family before this JVM's first publication there.
            CpRouteClient.forgetRemoteClientSurface(pending.projectRoot)
        }
        val receipt = CpRouteClient.observeRemoteClientSurface(
            projectRoot = pending.projectRoot,
            surfaceJson = pending.surfaceJson,
            surfaceId = surfaceId,
            mayReregister = { surfaceId in liveSurfaces },
        )
        if (receipt.exitCode != 0) {
            forwarded.remove(surfaceId)
            LOG.warn("[rd-layout] measured layout forward unavailable surface=$surfaceId: ${receipt.output}")
            return
        }
        forwarded[surfaceId] = pending
        RemoteClientLayoutAuthority.activate(project, surfaceId)
        LOG.info(
            "[rd-layout] forwarded measured columns surface=$surfaceId root=${pending.projectRoot} " +
                "columns=${layout.columns.size} [${layout.columns.joinToString(" | ") { it.joinToString(",") }}] " +
                "column_order=editor sequence=${snapshot.sequence} receipt=${receipt.output}",
        )
    }

    private fun retireSurface(surfaceId: String, reason: String) {
        lastUnavailable.remove(surfaceId)
        RemoteClientLayoutAuthority.release(project, surfaceId)
        val previous = forwarded.remove(surfaceId) ?: return
        val forgotten = CpRouteClient.forgetRemoteClientSurface(previous.projectRoot, surfaceId)
        LOG.info(
            "[rd-layout] retired measured layout surface=$surfaceId root=${previous.projectRoot} " +
                "reason=$reason forgotten=$forgotten",
        )
    }

    private fun noteUnavailable(surfaceId: String, reason: String) {
        if (lastUnavailable.put(surfaceId, reason) != reason) {
            LOG.info(
                "[rd-layout] measured layout unavailable surface=$surfaceId reason=$reason " +
                    "forwarded=${surfaceId in forwarded}",
            )
        }
    }

    private fun isRemoteClient(clientId: String): Boolean =
        ClientSessionsManager.getProjectSessions(project, ClientKind.ALL).any { session ->
            session.clientId.value == clientId && BackendRemoteSessionGate.observes(session.type)
        }

    private fun isSessionDocument(path: String): Boolean {
        val file = LocalFileSystem.getInstance().findFileByPath(path) ?: return false
        return AgentDocSessionFiles.isSessionDocument(file)
    }

    companion object {
        private val LOG = Logger.getInstance(ControllerRemoteClientLayoutForwarder::class.java)
    }
}
