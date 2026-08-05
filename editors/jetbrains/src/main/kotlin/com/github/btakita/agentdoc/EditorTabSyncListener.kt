package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.WindowManager
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/**
 * Reports what this editor looks like; the tmux consequence is derived elsewhere (`#jbsurfaceswap`
 * / `#jbpluginlazilyeffects`).
 *
 * Every tab selection publishes selected-document intent. A later editor-layout event projects that
 * intent with the current visible markdown set and column layout, then hands the complete
 * observation directly to the already-running Project Controller. Socket transport adds an
 * ordered reload generation/cursor without entering the reloadable native library. The
 * controller's process-scoped graph folds it against locally observed tmux state, derives
 * focus-vs-sync, and runs the tmux consequence as an `Effect`.
 *
 * The plugin therefore holds no plan, no previous-signature field, and no retry ladder: an
 * observation identical to the last one is idle and costs nothing, so repeat events need no dedup
 * here. What remains is event-storm handling that is genuinely the editor's: a 40ms debounce plus a
 * generation guard so a burst reports only its final state. Capture is projected by the next EDT
 * event; socket delivery remains off the EDT, so neither side waits on the other.
 *
 * Focus has a separate micro-coalesced state lane because a visible editor surface may span
 * documents owned by different project controllers. The focused file resolves its own controller
 * root and publishes one generation-fenced, focus-only surface Source; that controller's Lazily
 * graph derives pane selection while the spanning surface remains the authority for layout.
 *
 * Registered from [PluginLifecycleListener] via [install] so it survives hot-reload.
 */
class EditorTabSyncListener : FileEditorManagerListener {
    private val latestSurfaceObservation = AtomicReference<PendingSurfaceObservation?>(null)

    /**
     * One editor surface has one active controller root. When a cross-root split changes which
     * root spans the whole surface, the old controller projection must be retired immediately;
     * otherwise both controllers keep reconciling the same tmux window from incompatible retained
     * layouts.
     */
    private val surfaceRoots = SurfaceRootOwnership()

    /**
     * Debounce generation. Per-instance, so one project's tab churn cannot supersede another
     * project's pending observation.
     */
    private val generation = AtomicLong(0)
private val focusProjectionGeneration = AtomicLong(0)
    private val lifecycleLock = Any()

    @Volatile
    private var closed = false

    private val surfaceDeliveryExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "agent-doc-editor-surface-delivery").apply { isDaemon = true }
    }
private val focusProjectionExecutor = Executors.newSingleThreadScheduledExecutor { runnable ->
    Thread(runnable, "agent-doc-editor-focus-projection").apply { isDaemon = true }
    }

    companion object {
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
        private val GSON = com.google.gson.Gson()
        private const val FOCUS_COALESCE_MS = 12L

        private val instances = ConcurrentHashMap<Project, EditorTabSyncListener>()

        fun install(project: Project): EditorTabSyncListener =
            instances.computeIfAbsent(project) { EditorTabSyncListener() }

        /** Release the project's surface graphs and stop its debounce executor. */
        fun disposeProject(project: Project) {
            instances.remove(project)?.shutdown()
        }

    internal fun shouldPublishFocusProjection(
        requestedGeneration: Long,
        currentGeneration: Long,
        projectWindowActive: Boolean,
        ): Boolean =
        requestedGeneration == currentGeneration && projectWindowActive

    /**
     * Exact effect receipt for the retained focus-only surface. Admission is not success: install
     * the reverse-focus handoff lease only after the controller proves `select-pane`.
     */
    internal fun focusProjectionApplied(receiptJson: String): Boolean {
        return try {
            val receipt = JsonParser.parseString(receiptJson).asJsonObject
            val outcome =
                receipt.get("outcome")
                    ?.takeUnless { it.isJsonNull }
                    ?.asString
                    ?.let(JsonParser::parseString)
                    ?.asJsonObject
                    ?: return false
            val data =
                outcome.get("data")
                    ?.takeIf { it.isJsonObject }
                    ?.asJsonObject
                    ?: outcome
            data.get("focused")?.asBoolean == true
        } catch (_: Exception) {
            false
        }
    }
    }

internal enum class ObservationAuthority {
    Layout,
    DocumentSelection,
    FileOpened,
    IdeActivation,
}

    /**
     * The pending slot protects editor-event ordering only until the EDT has captured a
     * self-consistent surface. Controller delivery can take seconds while it proves pane
     * ownership and realizes a layout, so the slot must not retain an already-captured selection
     * while that I/O runs. Release before entering the delivery executor; restore only when the
     * same generation failed and no newer observation has occupied the slot.
     */
    internal object ObservationDeliveryOwnership {
        fun <T : Any> releaseForDelivery(
            slot: AtomicReference<T?>,
            captured: T,
        ): Boolean = slot.compareAndSet(captured, null)

        fun <T : Any> retainAfterFailure(
            slot: AtomicReference<T?>,
            captured: T,
        ): Boolean = slot.compareAndSet(null, captured)
    }

    internal class SurfaceRootOwnership {
        private val activeRoot = AtomicReference<String?>(null)
        private val observedRoots: MutableSet<String> = ConcurrentHashMap.newKeySet()

        fun recordAttempt(root: String) {
            observedRoots.add(root)
        }

        fun recordAttempts(roots: Collection<String>) {
            observedRoots.addAll(roots)
        }

        fun rootsToRetireBeforePublishing(root: String): List<String> =
            observedRoots.filter { it != root }.sorted()

        fun markPublished(root: String): List<String> {
            observedRoots.add(root)
            activeRoot.set(root)
            return observedRoots.filter { it != root }.sorted()
        }

        fun markForgotten(root: String): Boolean =
            activeRoot.get() != root && observedRoots.remove(root)

        fun drain(): List<String> {
            activeRoot.set(null)
            val roots = observedRoots.toList().sorted()
            observedRoots.clear()
            return roots
        }
    }

    /**
     * IDEA may emit selection, structural-layout, and file-open edges before `selectedFiles`
     * and every restored split window agree. Re-read the editor projection on a bounded
     * number of later EDT turns. This is event settling, not a timer/retry ladder: every
     * pass yields the EDT, the generation guard cancels stale work, and only exhausted
     * document selection applies an authoritative old-to-new edge.
     */
internal object SelectionProjectionSettling {
    const val MAX_REPROJECTION_PASSES = 3

    data class SettledProjection(
        val visibleMdFiles: List<String>,
        val editorLayout: EditorLayout?,
    )

    fun shouldReproject(
        authority: ObservationAuthority,
        remainingPasses: Int,
    ): Boolean =
        when (authority) {
            ObservationAuthority.Layout,
            ObservationAuthority.DocumentSelection,
            ObservationAuthority.FileOpened,
            ObservationAuthority.IdeActivation -> remainingPasses > 0
        }

    fun reconcileEventEdge(
        preferredFile: String?,
        previousFile: String?,
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout?,
    ): SettledProjection {
            if (
                preferredFile.isNullOrBlank() ||
                    previousFile.isNullOrBlank()
            ) {
                return SettledProjection(visibleMdFiles, editorLayout)
            }

            fun replacePreviousIfStale(files: List<String>): List<String> =
                if (preferredFile in files || previousFile !in files) {
                    files
                } else {
                    files
                        .map { file -> if (file == previousFile) preferredFile else file }
                        .distinct()
                }

            return SettledProjection(
                visibleMdFiles = replacePreviousIfStale(visibleMdFiles),
                editorLayout =
                    editorLayout?.copy(
                        columns =
                            editorLayout.columns.map { column ->
                                column.copy(files = replacePreviousIfStale(column.files))
                            },
                    ),
            )
    }
}

    /** One column of the reported split layout. Wire shape of Rust `SurfaceColumn`. */
    internal data class SurfaceColumnPayload(val files: List<String>)

    /**
     * What the editor looks like right now. Wire shape of Rust `EditorSurface`.
     *
     * Every field is something the editor saw. Notably absent is any notion of whether tmux agrees:
     * that is derived by comparing this observation against the controller's own, so the plugin
     * never reports a fact it would have to ask the controller for.
     */
    internal data class EditorSurfacePayload(
        val focused: String,
        val visible: List<String>,
        val open: List<String>,
        val columns: List<SurfaceColumnPayload>,
        @SerializedName("force_reconcile") val forceReconcile: Boolean,
        @SerializedName("focus_only") val focusOnly: Boolean,
    )

private data class PendingSurfaceObservation(
    val project: Project,
    val preferredFile: VirtualFile?,
    val previousFile: VirtualFile? = null,
    val forceReconcile: Boolean,
    val authority: ObservationAuthority,
)

private data class PendingSurface(
    val projectRoot: String,
    val relativePath: String,
    val surfaceJson: String,
    val knownControllerRoots: List<String>,
)

/**
 * Immutable editor-only snapshot captured on the EDT. Project-root discovery, filesystem walks,
 * patch-watch registration, JSON construction, and controller delivery all happen later on the
 * serialized background lane.
 */
private data class CapturedSurface(
    val project: Project,
    val projectBasePath: String?,
    val focusedFile: VirtualFile,
    val visibleMdFiles: List<String>,
    val openMdFiles: List<String>,
    val editorLayout: EditorLayout?,
    val forceReconcile: Boolean,
)

    internal object SurfaceReport {
        enum class ProjectionReadiness {
            Current,
            AwaitingSelectedDocument,
        }

        fun projectionReadiness(
            preferredActiveFile: String?,
            visibleMdFiles: List<String>,
            layoutMdFiles: List<String>? = null,
        ): ProjectionReadiness =
            if (
                !preferredActiveFile.isNullOrBlank() &&
                (
                    preferredActiveFile !in visibleMdFiles ||
                        layoutMdFiles?.let { preferredActiveFile !in it } == true
                )
            ) {
                ProjectionReadiness.AwaitingSelectedDocument
            } else {
                ProjectionReadiness.Current
            }

        fun restoredEditorWindowsReady(selectedWindowFiles: List<String?>): Boolean =
            selectedWindowFiles.isNotEmpty() && selectedWindowFiles.all { it != null }

        fun visibleMarkdownFilesFromRestoredWindows(
            selectedWindowFiles: List<String?>,
        ): List<String> =
            selectedWindowFiles
                .filterNotNull()
                .filter { it.endsWith(".md") }
                .distinct()

        fun resolveActiveFilePath(
            preferredActiveFile: String?,
            selectedEditorFile: String?,
            visibleMdFiles: List<String>,
        ): String? {
            if (!preferredActiveFile.isNullOrBlank()) {
                return preferredActiveFile
            }
            if (!selectedEditorFile.isNullOrBlank()) {
                return selectedEditorFile
            }
            return visibleMdFiles.firstOrNull()
        }

        fun tabsByProximity(focusedFile: String, tabs: List<String>): List<String> {
            val focusedIndex = tabs.indexOf(focusedFile).takeIf { it >= 0 } ?: 0
            return tabs
                .withIndex()
                .sortedWith(
                    compareBy<IndexedValue<String>>(
                        { kotlin.math.abs(it.index - focusedIndex) },
                        { it.index },
                    )
                )
                .map { it.value }
                .distinct()
        }

        fun prioritizeOpenDocuments(
            focusedFile: String,
            nearbyTabs: List<String>,
            visibleMdFiles: List<String>,
            openMdFiles: List<String>,
        ): List<String> =
            sequenceOf(
                    sequenceOf(focusedFile),
                    nearbyTabs.asSequence(),
                    visibleMdFiles.asSequence(),
                    openMdFiles.asSequence(),
                )
                .flatten()
                .filter(String::isNotBlank)
                .distinct()
                .toList()

        /**
         * Build the observation. An undetected layout reports **no** columns rather than a
         * synthesized single column, so the graph can tell "the editor has one column" apart from
         * "the editor could not see its layout" and skip the drift comparison in the latter case.
         */
        fun buildSurface(
            focusedFile: String,
            visibleMdFiles: List<String>,
            openMdFiles: List<String> = visibleMdFiles,
            editorLayout: EditorLayout?,
            forceReconcile: Boolean,
        ): EditorSurfacePayload =
            EditorSurfacePayload(
                focused = focusedFile,
                visible = visibleMdFiles.distinct(),
                open =
                    prioritizeOpenDocuments(
                        focusedFile = focusedFile,
                        nearbyTabs = openMdFiles,
                        visibleMdFiles = visibleMdFiles,
                        openMdFiles = emptyList(),
                    ),
                columns =
                    editorLayout
                        ?.columns
                        ?.map { column ->
                            SurfaceColumnPayload(column.files.filter { it.isNotBlank() }.distinct())
                        }
                        ?.filter { it.files.isNotEmpty() }
                        .orEmpty(),
                forceReconcile = forceReconcile,
                focusOnly = false,
            )

        /**
         * Selected-document state published to that document's own controller.
         *
         * This is intentionally a separate retained source from the spanning layout surface:
         * cross-root editor splits need the subproject controller to own pane selection, but the
         * narrow payload must never be mistaken for a one-column layout replacement.
         */
        fun buildFocusProjection(focusedFile: String): EditorSurfacePayload =
            EditorSurfacePayload(
                focused = focusedFile,
                visible = listOf(focusedFile),
                open = listOf(focusedFile),
                columns = emptyList(),
                forceReconcile = true,
                focusOnly = true,
            )
    }

    private fun log(msg: String) {
        LOG.debug("[layout-sync] $msg")
    }

    private fun requestObservation(
        observation: PendingSurfaceObservation,
    ) {
        latestSurfaceObservation.set(observation)
        val requested = generation.incrementAndGet()
        projectLatestSurfaceOnEditorThread(requested)
    }

    private fun projectLatestSurfaceOnEditorThread(requestedGeneration: Long) {
        projectLatestSurfaceOnEditorThread(
            requestedGeneration,
            SelectionProjectionSettling.MAX_REPROJECTION_PASSES,
        )
    }

    private fun projectLatestSurfaceOnEditorThread(
        requestedGeneration: Long,
        remainingSelectionPasses: Int,
    ) {
        ApplicationManager.getApplication().invokeLater {
            try {
                if (generation.get() != requestedGeneration) {
                    log("editor projection: superseded gen=$requestedGeneration")
                    return@invokeLater
                }
                val observation = latestSurfaceObservation.get() ?: return@invokeLater
            val captured =
                captureSurface(
                    project = observation.project,
                    preferredFile = observation.preferredFile,
                    previousFile = observation.previousFile,
                    forceReconcile = observation.forceReconcile,
                    reconcileStaleSelection =
                        observation.authority == ObservationAuthority.DocumentSelection &&
                            remainingSelectionPasses == 0,
                )
                        ?: run {
                            // The selection event can precede IDEA's visible-editor projection.
                            // Re-read on a bounded later EDT turn so a single tab switch is a
                            // complete reactive edge even when IDEA emits no follow-up event.
                            if (
                                SelectionProjectionSettling.shouldReproject(
                                    authority = observation.authority,
                                    remainingPasses = remainingSelectionPasses,
                                )
                            ) {
                                log(
                                    "observe: editor projection settling; remaining=" +
                                        "$remainingSelectionPasses",
                                )
                                projectLatestSurfaceOnEditorThread(
                                    requestedGeneration,
                                    remainingSelectionPasses - 1,
                                )
                            } else {
                                log("observe: retained until editor-surface dependency changes")
                            }
                            return@invokeLater
                        }
                if (
                    !ObservationDeliveryOwnership.releaseForDelivery(
                        latestSurfaceObservation,
                        observation,
                    )
                ) {
                    log("socket delivery: observation superseded before enqueue gen=$requestedGeneration")
                    return@invokeLater
                }
            surfaceDeliveryExecutor.execute {
                if (generation.get() != requestedGeneration) {
                    log("socket delivery: superseded gen=$requestedGeneration")
                    return@execute
                }
                val pending =
                    try {
                        resolveSurface(captured)
                    } catch (e: Exception) {
                        LOG.warn("[layout-sync] background surface resolution failed: ${e.message}")
                        return@execute
                    }
                synchronized(lifecycleLock) {
                        if (closed) return@execute
                        surfaceRoots.recordAttempts(pending.knownControllerRoots)
                    }
                    val rootsToRetire =
                        synchronized(lifecycleLock) {
                            if (closed) {
                                emptyList()
                            } else {
                                surfaceRoots.rootsToRetireBeforePublishing(pending.projectRoot)
                            }
                        }
            for (obsoleteRoot in rootsToRetire) {
                val surfaceForgotten = CpRouteClient.forgetEditorSurface(obsoleteRoot)
                CpRouteClient.forgetEditorFocus(obsoleteRoot)
                if (surfaceForgotten) {
                            surfaceRoots.markForgotten(obsoleteRoot)
                            log(
                                "observe: retired superseded surface root before publish " +
                                    "root=$obsoleteRoot active=${pending.projectRoot}",
                            )
                        } else {
                            log(
                                "observe: pre-publish surface root retirement deferred root=" +
                                    "$obsoleteRoot active=${pending.projectRoot}",
                            )
                        }
                    }
                    val receipt =
                        CpRouteClient.observeEditorSurface(
                            projectRoot = pending.projectRoot,
                            surfaceJson = pending.surfaceJson,
                        )
                    if (receipt.exitCode != 0) {
                        if (generation.get() == requestedGeneration) {
                            ObservationDeliveryOwnership.retainAfterFailure(
                                latestSurfaceObservation,
                                observation,
                            )
                        }
                        LOG.warn(
                            "[layout-sync] surface observation unavailable for " +
                                "${pending.relativePath}: ${receipt.output}",
                        )
                        return@execute
                    }
                    val obsoleteRoots =
                        synchronized(lifecycleLock) {
                            if (closed) {
                                null
                            } else {
                                surfaceRoots.markPublished(pending.projectRoot)
                            }
                        }
            if (obsoleteRoots == null) {
                CpRouteClient.forgetEditorSurface(pending.projectRoot)
                CpRouteClient.forgetEditorFocus(pending.projectRoot)
                surfaceRoots.markForgotten(pending.projectRoot)
                return@execute
            }
            for (obsoleteRoot in obsoleteRoots) {
                val surfaceForgotten = CpRouteClient.forgetEditorSurface(obsoleteRoot)
                CpRouteClient.forgetEditorFocus(obsoleteRoot)
                if (surfaceForgotten) {
                            surfaceRoots.markForgotten(obsoleteRoot)
                            log(
                                "observe: retired superseded surface root=$obsoleteRoot " +
                                    "active=${pending.projectRoot}",
                            )
                        } else {
                            log(
                                "observe: superseded surface root retirement deferred root=" +
                                    "$obsoleteRoot active=${pending.projectRoot}",
                            )
                        }
                    }
                    log("observe: published file=${pending.relativePath}")
                }
            } catch (e: Exception) {
                LOG.warn("[layout-sync] observation failed: ${e.message}")
            }
        }
    }

    private fun captureSurface(
        project: Project,
        preferredFile: VirtualFile? = null,
        previousFile: VirtualFile? = null,
        forceReconcile: Boolean = false,
        reconcileStaleSelection: Boolean = false,
    ): CapturedSurface? {
        val manager = FileEditorManager.getInstance(project)
        val managerEx = FileEditorManagerEx.getInstanceEx(project)
        val selectedWindowFiles = managerEx.windows.map { it.selectedFile }
        if (
            !SurfaceReport.restoredEditorWindowsReady(
                selectedWindowFiles.map { it?.path },
            )
        ) {
            return null
        }
        val rawVisibleMdFiles =
            SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                selectedWindowFiles.map { it?.path },
            )
        val rawEditorLayout =
            project.basePath?.let { basePath ->
                SyncLayoutAction.absolutizeEditorLayout(
                    basePath,
                    LayoutDetector.detectEditorLayout(project),
                )
            } ?: LayoutDetector.detectEditorLayout(project)
        val settledProjection =
            if (reconcileStaleSelection) {
                SelectionProjectionSettling.reconcileEventEdge(
                    preferredFile = preferredFile?.path,
                    previousFile = previousFile?.path,
                    visibleMdFiles = rawVisibleMdFiles,
                    editorLayout = rawEditorLayout,
                )
            } else {
                SelectionProjectionSettling.SettledProjection(
                    visibleMdFiles = rawVisibleMdFiles,
                    editorLayout = rawEditorLayout,
                )
            }
        val visibleMdFiles = settledProjection.visibleMdFiles
        if (visibleMdFiles.isEmpty()) return null
        val openMarkdownFiles = manager.openFiles.filter { it.name.endsWith(".md") }
        val preferredMarkdownFile = preferredFile?.takeIf { candidate ->
            candidate.isValid &&
                candidate.name.endsWith(".md") &&
                openMarkdownFiles.any { it.path == candidate.path }
        }
        if (preferredFile != null && preferredMarkdownFile == null) return null
        if (
            SurfaceReport.projectionReadiness(
                preferredActiveFile = preferredMarkdownFile?.path,
                visibleMdFiles = visibleMdFiles,
                layoutMdFiles =
                    settledProjection.editorLayout
                        ?.columns
                        ?.flatMap(LayoutColumn::files),
            ) != SurfaceReport.ProjectionReadiness.Current
        ) {
            return null
        }
        val selectedEditorFile =
            manager.selectedTextEditor?.virtualFile?.takeIf { it.name.endsWith(".md") }
        val activeFilePath =
            SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = preferredMarkdownFile?.path,
                selectedEditorFile = selectedEditorFile?.path,
                visibleMdFiles = visibleMdFiles,
            ) ?: return null
        val file =
            sequenceOf(
                    preferredMarkdownFile,
                    selectedEditorFile,
                    manager.selectedFiles.firstOrNull { it.name.endsWith(".md") },
                )
                .filterNotNull()
                .firstOrNull { it.path == activeFilePath } ?: return null
        val focusedWindowTabs =
            managerEx.windows
                .firstOrNull { it.selectedFile?.path == activeFilePath }
                ?.fileList
                ?.filter { it.name.endsWith(".md") }
                ?.map { it.path }
                .orEmpty()
        val openMdFiles =
            SurfaceReport.prioritizeOpenDocuments(
                focusedFile = activeFilePath,
                nearbyTabs =
                    SurfaceReport.tabsByProximity(
                        focusedFile = activeFilePath,
                        tabs = focusedWindowTabs,
                    ),
                visibleMdFiles = visibleMdFiles,
                openMdFiles = openMarkdownFiles.map { it.path },
            )

        return CapturedSurface(
            project = project,
            projectBasePath = project.basePath,
            focusedFile = file,
            visibleMdFiles = visibleMdFiles,
            openMdFiles = openMdFiles,
            editorLayout = settledProjection.editorLayout,
            forceReconcile = forceReconcile,
        )
    }

    private fun resolveSurface(captured: CapturedSurface): PendingSurface {
        val (focusedProjectRoot, focusedRelativePath) =
            TerminalUtil.resolveProject(captured.project, captured.focusedFile)
        // One root keys the surface graph, and it has to be the one that spans
        // the whole visible layout — a surface is the layout, not one document.
        val surfaceProjectRoot =
            SyncLayoutAction.chooseSyncProjectRoot(
                captured.projectBasePath,
                focusedProjectRoot,
                captured.visibleMdFiles,
            )
        val knownControllerRoots =
            (
                captured.openMdFiles.mapNotNull(TerminalUtil::nearestAgentDocProjectRoot) +
                    surfaceProjectRoot
            ).distinct()
        val absoluteEditorLayout =
            SyncLayoutAction.absolutizeEditorLayout(
                surfaceProjectRoot,
                SyncLayoutAction.normalizeEditorLayout(
                    captured.projectBasePath,
                    surfaceProjectRoot,
                    captured.editorLayout,
                ),
            )
        return PendingSurface(
            projectRoot = surfaceProjectRoot,
            relativePath = focusedRelativePath,
            surfaceJson =
                GSON.toJson(
                    SurfaceReport.buildSurface(
                        focusedFile = captured.focusedFile.path,
                        visibleMdFiles = captured.visibleMdFiles,
                        openMdFiles = captured.openMdFiles,
                        editorLayout = absoluteEditorLayout,
                        forceReconcile = captured.forceReconcile,
                    ),
                ),
            knownControllerRoots = knownControllerRoots,
        )
    }

    private fun requestFocusProjection(project: Project, file: VirtualFile) {
        val requestedGeneration = focusProjectionGeneration.incrementAndGet()
        synchronized(lifecycleLock) {
            if (closed) return
            try {
                focusProjectionExecutor.schedule(
                    focus@{
                        if (
                            project.isDisposed ||
                                !shouldPublishFocusProjection(
                                    requestedGeneration = requestedGeneration,
                                    currentGeneration = focusProjectionGeneration.get(),
                                    projectWindowActive =
                                        WindowManager.getInstance().getFrame(project)?.isActive ==
                                            true,
                                )
                        ) {
                            log(
                                "focus projection: superseded, inactive, or disposed " +
                                    "gen=$requestedGeneration",
                            )
                            return@focus
                        }

                        // The focused document, not the spanning editor surface, owns the
                        // controller root for this immediate handoff. This is what lets a
                        // superproject document and submodule document share one visible tmux
                        // window without routing both focus intents through one controller.
                        val (projectRoot, _) = TerminalUtil.resolveProject(project, file)
                        if (
                            project.isDisposed ||
                                !shouldPublishFocusProjection(
                                    requestedGeneration = requestedGeneration,
                                    currentGeneration = focusProjectionGeneration.get(),
                                    projectWindowActive =
                                        WindowManager.getInstance().getFrame(project)?.isActive ==
                                            true,
                                )
                        ) {
                            log(
                                "focus projection: superseded, inactive, or disposed after " +
                                    "root resolution gen=$requestedGeneration",
                            )
                            return@focus
                        }

                        val surfaceJson =
                            GSON.toJson(SurfaceReport.buildFocusProjection(file.path))
                        val receipt =
                            CpRouteClient.observeEditorFocus(
                                projectRoot = projectRoot,
                                surfaceJson = surfaceJson,
                            )
                        if (receipt.exitCode != 0) {
                            LOG.warn(
                                "[focus] retained focus projection unavailable for ${file.path}: " +
                                    receipt.output,
                            )
                        } else if (focusProjectionApplied(receipt.output)) {
                            // Reverse tmux→editor mirroring is suppressed only after the
                            // controller proves that this exact retained projection selected
                            // the pane. A missing actor must not install a 90-second stale lease.
                            TmuxPaneFocusSync.recordEditorFocusIntent(project, file.path)
                            log("focus projection: applied file=${file.path}")
                        } else {
                            log(
                                "focus projection: retained without pane selection " +
                                    "file=${file.path} receipt=${receipt.output}",
                            )
                        }
                    },
                    FOCUS_COALESCE_MS,
                    TimeUnit.MILLISECONDS,
                )
            } catch (rejected: RejectedExecutionException) {
                if (!closed) {
                    LOG.warn(
                        "[focus] focus projection rejected while listener is active",
                        rejected,
                    )
                }
            }
        }
    }

    private fun shutdown() {
        val roots =
            synchronized(lifecycleLock) {
                closed = true
                focusProjectionGeneration.incrementAndGet()
                focusProjectionExecutor.shutdownNow()
                surfaceDeliveryExecutor.shutdownNow()
                surfaceRoots.drain()
            }
        latestSurfaceObservation.set(null)
        Thread(
                {
                    for (root in roots) {
                        CpRouteClient.forgetEditorSurface(root)
                        CpRouteClient.forgetEditorFocus(root)
                    }
                },
                "agent-doc-editor-surface-forget",
            )
            .apply {
                isDaemon = true
                start()
            }
    }

    override fun selectionChanged(event: FileEditorManagerEvent) {
        val file = event.newFile ?: return
        if (!file.name.endsWith(".md")) return

        val project = event.manager.project
        requestFocusProjection(project, file)
        log("selectionChanged: newFile=${file.name}; projection queued")

        // Focus owns targeted session recovery. Re-forcing a full surface
        // reconcile on every tab switch made ordinary focus navigation contend
        // with layout sync and briefly expose stale extra panes.
        requestObservation(
        PendingSurfaceObservation(
            project = project,
            preferredFile = file,
            previousFile = event.oldFile,
            forceReconcile = false,
            authority = ObservationAuthority.DocumentSelection,
            ),
        )
    }

    /**
     * Re-publish the settled visible surface when IDEA becomes active. i3 workspace changes do not
     * necessarily emit a selection, file-open, or layout event, even though the terminal tool
     * window may have been rebuilt while the IDE was hidden.
     */
    fun onIdeActivated(project: Project) {
        log("ideActivated: settled surface projection queued")
        requestObservation(
            PendingSurfaceObservation(
                project = project,
                preferredFile = null,
                forceReconcile = false,
                authority = ObservationAuthority.IdeActivation,
            ),
        )
    }

    /**
     * The operator moved editor focus to [file] — e.g. clicked into the other split editor window
     * showing an already-open agent-doc document.
     *
     * #panefocussplit: [FileEditorManagerListener.selectionChanged] does NOT fire for focus
     * movement between two existing split editors (only for tab / visible-file-set changes), so
     * without this entry point split navigation never moves the tmux active pane.
     * [EditorFocusSyncListener] wires the per-editor focus events that call this.
     *
     * Focus events fire repeatedly for the same editor. The micro-coalesced focus lane collapses
     * those repeats. A component-focus event cannot change which tabs or splits are visible, so it
     * deliberately does not enqueue a second surface observation that could race the targeted pane
     * focus.
     */
    fun onEditorFocusGained(project: Project, file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        requestFocusProjection(project, file)
        log("focusGained: file=${file.name}")
    }

    /**
     * A structural split/container change occurred. [LayoutChangeDetector] reports it through the
     * same surface graph as tab and focus events, keeping one layout planner and one debounce
     * owner.
     */
    fun onEditorLayoutChanged(project: Project) {
        val pendingSelection =
            latestSurfaceObservation.get()?.takeIf {
                it.project === project &&
                    it.authority == ObservationAuthority.DocumentSelection &&
                    it.preferredFile != null
            }
        requestObservation(
            pendingSelection
                ?: PendingSurfaceObservation(
                    project = project,
                    preferredFile = null,
                    forceReconcile = false,
                    authority = ObservationAuthority.Layout,
            ),
        )
    }

    /**
     * IDEA can restore editor containers before it finishes restoring their files. The delayed
     * structural seed then observes the right split count with an incomplete selected-file set,
     * and opening the restored files does not necessarily emit either `selectionChanged` or a new
     * container event. Publish the completed surface from the file lifecycle edge so startup does
     * not depend on an explicit Sync Tmux Layout action.
     */
    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        log("fileOpened: file=${file.name}")
        requestObservation(
            PendingSurfaceObservation(
                project = source.project,
                preferredFile = file,
                forceReconcile = false,
                authority = ObservationAuthority.FileOpened,
            ),
        )
    }

    /**
     * Evict the per-document state-projection mirror + generation counters when an editor tab
     * closes (`#jbmirrorevict` / `#nsq2`). Without this the `StateProjectionBridge` maps grow
     * monotonically and a reused path (move/symlink/reopen) surfaces the prior document's stale
     * projection state. Re-subscription lazily re-creates the mirror from a fresh cold snapshot.
     */
    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        StateProjectionBridge.evictForFile(file.path)
    }
}
