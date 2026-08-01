package com.github.btakita.agentdoc

import com.google.gson.annotations.SerializedName
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
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
 * Focus and layout are both projections of the same selected-document Source. There is no direct
 * focus command lane for an older selection to occupy: the controller receives ordered facts,
 * fences retired plugin generations, and derives the consequence.
 *
 * Registered from [PluginLifecycleListener] via [install] so it survives hot-reload.
 */
class EditorTabSyncListener : FileEditorManagerListener {
    private val latestSurfaceObservation = AtomicReference<PendingSurfaceObservation?>(null)

    /**
     * Every project root this instance has observed. Controller projection membership is released
     * on project close through `editor_surface_forget`; controller cursors fence late reload
     * callbacks independently.
     */
    private val observedRoots: MutableSet<String> = ConcurrentHashMap.newKeySet()

    /**
     * Debounce generation. Per-instance, so one project's tab churn cannot supersede another
     * project's pending observation.
     */
    private val generation = AtomicLong(0)

    private val surfaceDeliveryExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "agent-doc-editor-surface-delivery").apply { isDaemon = true }
    }

    companion object {
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
        private val GSON = com.google.gson.Gson()

        private val instances = ConcurrentHashMap<Project, EditorTabSyncListener>()

        fun install(project: Project): EditorTabSyncListener =
            instances.computeIfAbsent(project) { EditorTabSyncListener() }

        /** Release the project's surface graphs and stop its debounce executor. */
        fun disposeProject(project: Project) {
            instances.remove(project)?.shutdown()
        }
    }

    internal enum class ObservationAuthority {
        Layout,
        ComponentFocus,
        DocumentSelection,
    }

    internal object ObservationProjection {
        fun shouldReplace(
            currentAuthority: ObservationAuthority?,
            currentFile: String?,
            incomingAuthority: ObservationAuthority,
            incomingFile: String?,
        ): Boolean {
            if (
                currentAuthority == ObservationAuthority.DocumentSelection &&
                incomingAuthority == ObservationAuthority.ComponentFocus
            ) {
                return currentFile == incomingFile
            }
            return true
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
    )

    private data class PendingSurfaceObservation(
        val project: Project,
        val preferredFile: VirtualFile?,
        val forceReconcile: Boolean,
        val authority: ObservationAuthority,
    )

    private data class PendingSurface(
        val projectRoot: String,
        val relativePath: String,
        val surfaceJson: String,
    )

    internal object SurfaceReport {
        enum class ProjectionReadiness {
            Current,
            AwaitingSelectedDocument,
        }

        fun projectionReadiness(
            preferredActiveFile: String?,
            visibleMdFiles: List<String>,
        ): ProjectionReadiness =
            if (!preferredActiveFile.isNullOrBlank() && preferredActiveFile !in visibleMdFiles) {
                ProjectionReadiness.AwaitingSelectedDocument
            } else {
                ProjectionReadiness.Current
            }

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
            )
    }

    private fun log(msg: String) {
        LOG.debug("[layout-sync] $msg")
    }

    private fun requestObservation(
        observation: PendingSurfaceObservation,
    ) {
        while (true) {
            val current = latestSurfaceObservation.get()
            if (
                !ObservationProjection.shouldReplace(
                    currentAuthority = current?.authority,
                    currentFile = current?.preferredFile?.path,
                    incomingAuthority = observation.authority,
                    incomingFile = observation.preferredFile?.path,
                )
            ) {
                log(
                    "observe: retained ${current?.authority} file=${current?.preferredFile?.name} " +
                        "over ${observation.authority} file=${observation.preferredFile?.name}",
                )
                return
            }
            if (latestSurfaceObservation.compareAndSet(current, observation)) break
        }
        val requested = generation.incrementAndGet()
        projectLatestSurfaceOnEditorThread(requested)
    }

    private fun projectLatestSurfaceOnEditorThread(requestedGeneration: Long) {
        ApplicationManager.getApplication().invokeLater {
            try {
                if (generation.get() != requestedGeneration) {
                    log("editor projection: superseded gen=$requestedGeneration")
                    return@invokeLater
                }
                val observation = latestSurfaceObservation.get() ?: return@invokeLater
                val pending =
                    captureSurface(
                        project = observation.project,
                        preferredFile = observation.preferredFile,
                        forceReconcile = observation.forceReconcile,
                    )
                        ?: run {
                            // The selection event can precede IDEA's visible-editor
                            // projection. Retain the selected-document Source and
                            // wait for the next real selection/layout/focus event;
                            // a timer retry would repeatedly block the EDT.
                            log("observe: retained until editor-surface dependency changes")
                            return@invokeLater
                        }
                surfaceDeliveryExecutor.execute {
                    if (generation.get() != requestedGeneration) {
                        log("socket delivery: superseded gen=$requestedGeneration")
                        return@execute
                    }
                    observedRoots.add(pending.projectRoot)
                    val receipt =
                        CpRouteClient.observeEditorSurface(
                            projectRoot = pending.projectRoot,
                            surfaceJson = pending.surfaceJson,
                        )
                    if (receipt.exitCode != 0) {
                        LOG.warn(
                            "[layout-sync] surface observation unavailable for " +
                                "${pending.relativePath}: ${receipt.output}",
                        )
                        return@execute
                    }
                    latestSurfaceObservation.compareAndSet(observation, null)
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
        forceReconcile: Boolean = false,
    ): PendingSurface? {
        val manager = FileEditorManager.getInstance(project)
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
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
        val managerEx = FileEditorManagerEx.getInstanceEx(project)
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

        val (focusedProjectRoot, focusedRelativePath) = TerminalUtil.resolveProject(project, file)
        // One root keys the surface graph, and it has to be the one that spans
        // the whole visible layout — a surface is the layout, not one document.
        val surfaceProjectRoot =
            SyncLayoutAction.chooseSyncProjectRoot(
                project.basePath,
                focusedProjectRoot,
                visibleMdFiles,
            )
        val absoluteEditorLayout =
            SyncLayoutAction.absolutizeEditorLayout(
                surfaceProjectRoot,
                SyncLayoutAction.normalizeEditorLayout(
                    project.basePath,
                    surfaceProjectRoot,
                    LayoutDetector.detectEditorLayout(project),
                ),
            )
        return PendingSurface(
            projectRoot = surfaceProjectRoot,
            relativePath = focusedRelativePath,
            surfaceJson =
                GSON.toJson(
                    SurfaceReport.buildSurface(
                        focusedFile = file.path,
                        visibleMdFiles = visibleMdFiles,
                        openMdFiles = openMdFiles,
                        editorLayout = absoluteEditorLayout,
                        forceReconcile = forceReconcile,
                    )
                ),
        )
    }

    private fun shutdown() {
        surfaceDeliveryExecutor.shutdownNow()
        val roots = observedRoots.toList()
        observedRoots.clear()
        latestSurfaceObservation.set(null)
        Thread(
                {
                    for (root in roots) {
                        CpRouteClient.forgetEditorSurface(root)
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
        val manager = FileEditorManager.getInstance(project)
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
        log("selectionChanged: newFile=${file.name} mdFiles=$visibleMdFiles")
        if (visibleMdFiles.isEmpty()) return

        // Focus owns targeted session recovery. Re-forcing a full surface
        // reconcile on every tab switch made ordinary focus navigation contend
        // with layout sync and briefly expose stale extra panes.
        requestObservation(
            PendingSurfaceObservation(
                project = project,
                preferredFile = file,
                forceReconcile = false,
                authority = ObservationAuthority.DocumentSelection,
            ),
        )
        TmuxPaneFocusSync.recordEditorFocusIntent(project, file.path)
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
        requestObservation(
            PendingSurfaceObservation(
                project = project,
                preferredFile = file,
                forceReconcile = false,
                authority = ObservationAuthority.ComponentFocus,
            ),
        )
        TmuxPaneFocusSync.recordEditorFocusIntent(project, file.path)
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
