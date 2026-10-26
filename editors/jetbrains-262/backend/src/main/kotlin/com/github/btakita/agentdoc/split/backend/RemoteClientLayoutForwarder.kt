@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.AgentDocSessionFiles
import com.github.btakita.agentdoc.CpRouteClient
import com.github.btakita.agentdoc.EditorDerivation
import com.github.btakita.agentdoc.EditorLayout
import com.github.btakita.agentdoc.EditorTabSyncListener
import com.github.btakita.agentdoc.LayoutColumn
import com.github.btakita.agentdoc.LayoutDetector
import com.github.btakita.agentdoc.RemoteClientLayoutAuthority
import com.github.btakita.agentdoc.SyncLayoutAction
import com.github.btakita.agentdoc.TerminalUtil
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.FRONTEND_LAYOUT_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.google.gson.annotations.SerializedName
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import javax.swing.SwingUtilities

/**
 * GH #320 option B: the main window's layout as the JetBrains Client measured it.
 *
 * The frontend publishes each editor window's on-screen bounds (state). The backend reports each
 * main-frame window's ordinal, selected session document, and measured origin to the
 * hot-reloaded native `client_measured_layout` derivation (GH #326), which owns the grouping and
 * the order provenance; nothing is inferred from editor events. The Kotlin derivation below is
 * the frozen compatibility floor for a native library that cannot answer.
 */
internal sealed interface MeasuredMainLayout {
    /** Columns (absolute paths) in measured left-to-right order, plus the selected document. */
    data class Measured(
        val columns: List<List<String>>,
        val focused: String,
        val focusedEditor: String? = null,
    ) : MeasuredMainLayout {
        val visible: List<String>
            get() = columns.flatten().distinct()
    }

    /** No measured order this snapshot; the last forwarded layout stays in force. */
    data class Unavailable(val reason: String) : MeasuredMainLayout
}

internal object RemoteClientMeasuredLayout {
    private val LOG = Logger.getInstance(RemoteClientMeasuredLayout::class.java)
    private val GSON = com.google.gson.Gson()

    /** GH #326: one main-frame window as observed. Wire shape of Rust `ClientWindowEvidence`. */
    internal data class ClientWindowEvidence(
        val ordinal: Int,
        @SerializedName("selected_session") val selectedSession: String?,
        @SerializedName("holds_session_document") val holdsSessionDocument: Boolean,
        val x: Int?,
        val y: Int?,
    )

    internal data class ClientMeasuredLayoutEvidence(val windows: List<ClientWindowEvidence>)

    fun derive(
        snapshot: FrontendSurfaceSnapshot,
        isSessionDocument: (String) -> Boolean,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): MeasuredMainLayout {
        if (snapshot.schemaVersion < GEOMETRY_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
            return MeasuredMainLayout.Unavailable("schema_${snapshot.schemaVersion}_without_geometry")
        }
        val main = snapshot.surfaces.singleOrNull { it.role == SurfaceRole.MAIN }
            ?: return MeasuredMainLayout.Unavailable("no_main_surface")
        val evidence = ClientMeasuredLayoutEvidence(
            main.windows.map { window ->
                ClientWindowEvidence(
                    ordinal = window.ordinal,
                    selectedSession = window.selectedPath?.takeIf(isSessionDocument),
                    holdsSessionDocument = window.openPaths.any(isSessionDocument),
                    x = window.bounds?.x,
                    y = window.bounds?.y,
                )
            },
        )
        val measured = derivation.derive("client_measured_layout", GSON.toJson(evidence))
            ?.let(::parseMeasuredLayout)
            ?: if (snapshot.schemaVersion >= FRONTEND_LAYOUT_SURFACE_SNAPSHOT_SCHEMA_VERSION) {
                frozenFrontendDerive(main, isSessionDocument)
            } else {
                frozenDerive(evidence)
            }
        if (measured !is MeasuredMainLayout.Measured) return measured
        val focusedEditor =
            main.focusedEditorPath
                ?.takeIf { snapshot.schemaVersion >= FOCUSED_EDITOR_SURFACE_SNAPSHOT_SCHEMA_VERSION }
                ?.takeIf(isSessionDocument)
                ?.takeIf(measured.visible::contains)
        return measured.copy(focusedEditor = focusedEditor)
    }

    /**
     * GH #343 compatibility floor for schema 3. The frontend-authored vector is retained only for
     * a native library that predates the schema; a live native generation always receives the
     * measured evidence above, so grouping-rule changes remain restart-free (GH #346).
     */
    private fun frozenFrontendDerive(
        main: FrontendSurface,
        isSessionDocument: (String) -> Boolean,
    ): MeasuredMainLayout {
        val authoritative = main.layoutColumns
            ?: return MeasuredMainLayout.Unavailable("frontend_columns_missing")
        for (window in main.windows) {
            if (window.selectedPath?.takeIf(isSessionDocument) == null &&
                window.openPaths.any(isSessionDocument)
            ) {
                return MeasuredMainLayout.Unavailable("split_shows_non_session_tab")
            }
        }
        val columns = authoritative.map { column ->
            column.paths.filter(isSessionDocument).distinct()
        }.filter(List<String>::isNotEmpty)
        if (columns.isEmpty()) return MeasuredMainLayout.Unavailable("no_session_documents")
        val focused = main.windows.firstOrNull { it.selectedPath in columns.flatten() }?.selectedPath
            ?: columns.first().first()
        return MeasuredMainLayout.Measured(columns, focused)
    }

    /** Parse the native `client_measured_layout` output (`outcome` = `measured` | `unavailable`). */
    internal fun parseMeasuredLayout(json: String): MeasuredMainLayout? = try {
        val root = com.google.gson.JsonParser.parseString(json).asJsonObject
        when (root.get("outcome")?.asString) {
            "measured" -> MeasuredMainLayout.Measured(
                columns = root.getAsJsonArray("columns").map { column ->
                    column.asJsonArray.map { it.asString }
                },
                focused = root.get("focused").asString,
            )
            "unavailable" -> MeasuredMainLayout.Unavailable(root.get("reason")?.asString ?: "unknown")
            else -> null
        }
    } catch (e: Exception) {
        LOG.warn("[rd-layout] native measured layout unreadable: ${e.message}")
        null
    }

    /**
     * GH #326 compatibility floor: the GH #320 option B rules, frozen. Used only when no native
     * generation answers. Never extend it; change the native derivation instead.
     */
    internal fun frozenDerive(evidence: ClientMeasuredLayoutEvidence): MeasuredMainLayout {
        val measured = mutableListOf<LayoutDetector.LayoutWindowSnapshot>()
        for (window in evidence.windows.sortedBy { it.ordinal }) {
            val selected = window.selectedSession
            if (selected == null) {
                // `#stickymdpane`: a split showing a source tab next to its session documents is
                // still on screen; its column is unknown, not gone. Keep the last forwarded layout
                // rather than collapse it. A window that holds no session document contributes no
                // column, so closing a split still collapses the layout.
                if (window.holdsSessionDocument) {
                    return MeasuredMainLayout.Unavailable("split_shows_non_session_tab")
                }
                continue
            }
            val x = window.x ?: return MeasuredMainLayout.Unavailable("window_unmeasured")
            val y = window.y ?: return MeasuredMainLayout.Unavailable("window_unmeasured")
            measured += LayoutDetector.LayoutWindowSnapshot(x, y, selected)
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

/**
 * GH #326 phase 4: the controller surface a measured layout publishes, derived by the hot-reloaded
 * native `remote_client_surface` derivation.
 *
 * The forwarder reports the measured columns, the selected document, and that document's own
 * project root (a filesystem read it makes on its worker); native owns the root that spans the
 * layout (the `sync_layout` root rule), the paths normalized against it, and the `editor_surface`
 * payload. The order provenance is `editor` because the columns came from measured geometry. The
 * Schema 4 additionally proves the editor window that owns frontend AWT focus. Such a snapshot
 * publishes that document as focus authority; snapshots without the proof still own pane
 * membership and order only (`preserve_focus`). The payload stays JSON, so a field a newer library
 * adds reaches the controller without a plugin generation. [frozen] answers only when no native
 * generation does and is never extended.
 */
internal object RemoteClientSurfacePayload {
    private val LOG = Logger.getInstance(RemoteClientSurfacePayload::class.java)
    private val GSON = com.google.gson.Gson()

    /** One measured layout to forward. Wire shape of Rust `RemoteClientSurfaceEvidence`. */
    internal data class Evidence(
        @SerializedName("base_path") val basePath: String?,
        @SerializedName("fallback_root") val fallbackRoot: String?,
        val focused: String,
        val focus: String? = null,
        @SerializedName("focus_root") val focusRoot: String? = null,
        val columns: List<List<String>>,
    )

    internal fun evidence(
        basePath: String?,
        layout: MeasuredMainLayout.Measured,
        focusedRoot: (String?, String) -> String = { base, focused ->
            TerminalUtil.resolveProjectPath(base, focused).first
        },
    ): Evidence {
        return Evidence(
            basePath = basePath,
            fallbackRoot = focusedRoot(basePath, layout.focused),
            focused = layout.focused,
            focus = layout.focusedEditor,
            focusRoot = layout.focusedEditor?.let { focusedRoot(basePath, it) },
            columns = layout.columns,
        )
    }

    /** Derive the forwarded surface. Never on the EDT: the forwarder's worker calls this. */
    fun build(
        basePath: String?,
        layout: MeasuredMainLayout.Measured,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): ForwardedRemoteSurface {
        val evidence = evidence(basePath, layout)
        val native =
            if (SwingUtilities.isEventDispatchThread()) {
                LOG.warn("[rd-layout] remote_client_surface asked on the EDT; using the compatibility floor")
                null
            } else {
                derivation.derive("remote_client_surface", GSON.toJson(evidence))?.let(::parseNative)
            }
        return native ?: frozen(evidence)
    }

    /** Parse the native `remote_client_surface` output; `null` takes the frozen floor. */
    internal fun parseNative(json: String): ForwardedRemoteSurface? =
        try {
            val root = com.google.gson.JsonParser.parseString(json).asJsonObject
            val surface = root.get("surface")?.takeIf { it.isJsonObject }?.asJsonObject
            val projectRoot = root.get("project_root")?.takeIf { it.isJsonPrimitive }?.asString
            if (surface == null || projectRoot.isNullOrBlank()) {
                LOG.warn("[rd-layout] native remote client surface incomplete: root=$projectRoot")
                null
            } else {
                ForwardedRemoteSurface(projectRoot, surface.toString())
            }
        } catch (e: Exception) {
            LOG.warn("[rd-layout] native remote client surface unreadable: ${e.message}")
            null
        }

    /**
     * GH #326 compatibility floor: the pre-#326 forward rules (the classic surface's
     * [SyncLayoutAction] root choice and path normalization, then the
     * [EditorTabSyncListener.SurfaceReport] payload), frozen. Used only when no native generation
     * answers. Never extend it; change the native derivation instead.
     */
    internal fun frozen(
        evidence: Evidence,
        nearestRoot: (String) -> String? = TerminalUtil::nearestAgentDocProjectRoot,
    ): ForwardedRemoteSurface {
        val visible = evidence.columns.flatten().distinct()
        val focusedRoot = evidence.fallbackRoot?.takeIf { it.isNotBlank() } ?: evidence.basePath ?: ""
        val projectRoot = SyncLayoutAction.chooseSyncProjectRoot(evidence.basePath, focusedRoot, visible, nearestRoot)
        val editorLayout = SyncLayoutAction.absolutizeEditorLayout(
            projectRoot,
            SyncLayoutAction.normalizeEditorLayout(
                evidence.basePath,
                projectRoot,
                EditorLayout(evidence.columns.map(::LayoutColumn), columnOrder = "editor"),
            ),
        )
        val surface = EditorTabSyncListener.SurfaceReport.buildSurface(
            focusedFile = evidence.focused,
            visibleMdFiles = visible,
            openMdFiles = visible,
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
    /**
     * Establish pending measured-layout authority before the first snapshot, but only for an
     * authenticated remote session. A modular frontend also runs in a monolithic IDE; that local
     * lease is detached-view transport, not permission to retire the classic layout projection.
     */
    fun open(clientId: String, generation: Long): Boolean

    fun forward(clientId: String, generation: Long, snapshot: FrontendSurfaceSnapshot)

    fun retire(clientId: String, generation: Long)

    object Disabled : RemoteClientLayoutForwarder {
        override fun open(clientId: String, generation: Long): Boolean = false

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

    override fun open(clientId: String, generation: Long): Boolean {
        val surfaceId = remoteClientSurfaceId(clientId, generation)
        if (!isRemoteClient(clientId)) {
            execute { noteUnavailable(surfaceId, "local_client") }
            return false
        }
        liveSurfaces += surfaceId
        RemoteClientLayoutAuthority.activate(project, surfaceId)
        return true
    }

    override fun forward(clientId: String, generation: Long, snapshot: FrontendSurfaceSnapshot) {
        val surfaceId = remoteClientSurfaceId(clientId, generation)
        execute { deliver(clientId, surfaceId, snapshot) }
    }

    override fun retire(clientId: String, generation: Long) {
        val surfaceId = remoteClientSurfaceId(clientId, generation)
        liveSurfaces -= surfaceId
        execute { retireSurface(surfaceId, reason = "frontend_connection_retired") }
    }

    fun dispose() {
        val retiring = liveSurfaces.toList()
        liveSurfaces.clear()
        // Pending leases may not have forwarded a first snapshot yet, but they
        // already own frontend authority and must release it on disposal.
        execute { retiring.forEach { retireSurface(it, reason = "backend_disposed") } }
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
        BackendRemoteSessionGate.ownsLayout(
            clientId,
            marketplaceRemoteSessions(project),
        )

    private fun isSessionDocument(path: String): Boolean {
        val file = LocalFileSystem.getInstance().findFileByPath(path) ?: return false
        return AgentDocSessionFiles.isSessionDocument(file)
    }

    companion object {
        private val LOG = Logger.getInstance(ControllerRemoteClientLayoutForwarder::class.java)
    }
}

private fun marketplaceRemoteSessions(project: Project): List<Pair<String, String>> = runCatching {
    val managerClass = Class.forName("com.intellij.openapi.client.ClientSessionsManager")
    val kindClass = Class.forName("com.intellij.openapi.client.ClientKind")
    val all = kindClass.getField("ALL").get(null)
    val companion = managerClass.getField("Companion").get(null)
    val sessions = companion.javaClass.methods.firstOrNull {
        it.name == "getProjectSessions" && it.parameterCount == 2
    }?.invoke(companion, project, all) as? Iterable<*> ?: return emptyList()
    sessions.mapNotNull { session ->
        session ?: return@mapNotNull null
        val clientId = session.javaClass.methods.first { it.name == "getClientId" && it.parameterCount == 0 }
            .invoke(session)
        val id = clientId.javaClass.methods.firstOrNull {
            it.name == "getValue" && it.parameterCount == 0
        }?.invoke(clientId)?.toString() ?: clientId.toString()
        val type = session.javaClass.methods.first { it.name == "getType" && it.parameterCount == 0 }
            .invoke(session).toString()
        id to type
    }
}.getOrDefault(emptyList())
