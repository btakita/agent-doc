package com.github.btakita.agentdoc

import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.WindowManager
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/**
 * Reports what this editor looks like; the tmux consequence is derived elsewhere (`#jbsurfaceswap`
 * / `#jbpluginlazilyeffects`).
 *
 * Every tab selection publishes selected-document intent. A later editor-layout event projects that
 * intent with the current visible markdown set and column layout, then hands the complete
 * observation to `agent_doc_editor_surface_observe_json`. The reactive graph behind that entry
 * point folds the observation against what tmux was last reconciled against, derives focus-vs-sync,
 * and runs the Project Controller command as an `Effect`.
 *
 * The plugin therefore holds no plan, no previous-signature field, and no retry ladder: an
 * observation identical to the last one is idle and costs nothing, so repeat events need no dedup
 * here. What remains is event-storm handling that is genuinely the editor's: a 40ms debounce plus a
 * generation guard so a burst reports only its final state, and an off-EDT executor so the derived
 * command never blocks the UI thread.
 *
 * Focus is latency-sensitive and takes a separate micro-coalesced lane. IDEA emits both selection
 * and component-focus events for one click, so a 12ms generation window collapses those duplicates
 * before they cross the socket. The selected document is then sent through the controller's
 * project-scoped latest-wins focus command before layout reconciliation. The debounced surface
 * observation still owns safe passive layout sync; the fast lane only makes an already-visible
 * target pane react immediately.
 *
 * Registered from [PluginLifecycleListener] via [install] so it survives hot-reload.
 */
class EditorTabSyncListener : FileEditorManagerListener {
    private val latestSurfaceObservation = AtomicReference<PendingSurfaceObservation?>(null)

    /**
     * Every project root this instance has observed. A root's graph holds the reconciled-layout
     * history, so it is released on project close through `agent_doc_editor_surface_forget`.
     */
    private val observedRoots: MutableSet<String> = ConcurrentHashMap.newKeySet()

    /**
     * Debounce generation. Per-instance, so one project's tab churn cannot supersede another
     * project's pending observation.
     */
    private val generation = AtomicLong(0)
    private val focusGeneration = AtomicLong(0)

    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-editor-tab-sync").apply { isDaemon = true }
    }
    private val focusExecutor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-editor-focus-sync").apply { isDaemon = true }
    }

    companion object {
        internal const val SURFACE_COALESCE_MS = 40L
        private const val FOCUS_COALESCE_MS = 12L
        private val FOCUS_MAX_AGE_NANOS = TimeUnit.MILLISECONDS.toNanos(500)
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
        private val GSON = com.google.gson.Gson()

        internal fun shouldDispatchFocus(
            requestedGeneration: Long,
            currentGeneration: Long,
            projectWindowActive: Boolean,
            ageNanos: Long,
        ): Boolean =
            requestedGeneration == currentGeneration &&
                projectWindowActive &&
                ageNanos in 0..FOCUS_MAX_AGE_NANOS

        private val instances = ConcurrentHashMap<Project, EditorTabSyncListener>()

        fun install(project: Project): EditorTabSyncListener =
            instances.computeIfAbsent(project) { EditorTabSyncListener() }

        /** Release the project's surface graphs and stop its debounce executor. */
        fun disposeProject(project: Project) {
            instances.remove(project)?.shutdown()
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
        val columns: List<SurfaceColumnPayload>,
        @SerializedName("force_reconcile") val forceReconcile: Boolean,
    )

    private data class PendingSurfaceObservation(
        val project: Project,
        val preferredFile: VirtualFile?,
        val forceReconcile: Boolean,
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

        /**
         * Build the observation. An undetected layout reports **no** columns rather than a
         * synthesized single column, so the graph can tell "the editor has one column" apart from
         * "the editor could not see its layout" and skip the drift comparison in the latter case.
         */
        fun buildSurface(
            focusedFile: String,
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
            forceReconcile: Boolean,
        ): EditorSurfacePayload =
            EditorSurfacePayload(
                focused = focusedFile,
                visible = visibleMdFiles.distinct(),
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
        delayMs: Long = SURFACE_COALESCE_MS,
    ) {
        latestSurfaceObservation.set(observation)
        val requested = generation.incrementAndGet()
        executor.schedule(
            observe@{
                try {
                    if (generation.get() != requested) {
                        log("debounce: superseded gen=$requested")
                        return@observe
                    }
                    reportLatestSurface()
                } catch (e: Exception) {
                    LOG.warn("[layout-sync] observation failed: ${e.message}")
                }
            },
            delayMs.coerceAtLeast(0L),
            TimeUnit.MILLISECONDS,
        )
    }

    private fun reportLatestSurface() {
        val observation = latestSurfaceObservation.get() ?: return
        val pending =
            captureSurface(
                project = observation.project,
                preferredFile = observation.preferredFile,
                forceReconcile = observation.forceReconcile,
            )
                ?: run {
                    log("observe: awaiting selected document in stable editor surface")
                    return
                }
        observedRoots.add(pending.projectRoot)
        val accepted =
            NativeAdminControls.editorSurfaceEnqueue(
                projectRoot = pending.projectRoot,
                surfaceJson = pending.surfaceJson,
            )
        if (!accepted) {
            LOG.warn("[layout-sync] surface observation unavailable for ${pending.relativePath}")
            return
        }
        latestSurfaceObservation.compareAndSet(observation, null)
        log("observe: queued file=${pending.relativePath}")
    }

    private fun requestImmediateFocus(project: Project, file: VirtualFile) {
        val documentPath = file.path
        val requested = focusGeneration.incrementAndGet()
        val requestedAtNanos = System.nanoTime()
        focusExecutor.schedule(
            focus@{
                if (
                    !shouldDispatchFocus(
                        requestedGeneration = requested,
                        currentGeneration = focusGeneration.get(),
                        projectWindowActive =
                            WindowManager.getInstance().getFrame(project)?.isActive == true,
                        ageNanos = System.nanoTime() - requestedAtNanos,
                    )
                ) {
                    log(
                        "focus: superseded, inactive, or expired before root resolution gen=$requested"
                    )
                    return@focus
                }
                // Project-root discovery performs local filesystem walks. Keep it off the
                // IntelliJ event thread along with the controller round trip.
                val (projectRoot, _) = TerminalUtil.resolveProject(project, file)
                if (
                    !shouldDispatchFocus(
                        requestedGeneration = requested,
                        currentGeneration = focusGeneration.get(),
                        projectWindowActive =
                            WindowManager.getInstance().getFrame(project)?.isActive == true,
                        ageNanos = System.nanoTime() - requestedAtNanos,
                    )
                ) {
                    log(
                        "focus: superseded, inactive, or expired after root resolution gen=$requested"
                    )
                    return@focus
                }
                TmuxPaneFocusSync.recordEditorFocusIntent(project, documentPath)
                val receipt =
                    CpRouteClient.submitFocusDocumentPane(
                        projectRoot = projectRoot,
                        documentPath = documentPath,
                    )
                log("focus: file=$documentPath receipt=$receipt")
            },
            FOCUS_COALESCE_MS,
            TimeUnit.MILLISECONDS,
        )
    }

    private fun captureSurface(
        project: Project,
        preferredFile: VirtualFile? = null,
        forceReconcile: Boolean = false,
    ): PendingSurface? {
        val manager = FileEditorManager.getInstance(project)
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
        if (visibleMdFiles.isEmpty()) return null
        val preferredMarkdownFile = preferredFile?.takeIf { it.name.endsWith(".md") }
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
                        editorLayout = absoluteEditorLayout,
                        forceReconcile = forceReconcile,
                    )
                ),
        )
    }

    private fun shutdown() {
        executor.shutdownNow()
        focusExecutor.shutdownNow()
        val roots = observedRoots.toList()
        observedRoots.clear()
        latestSurfaceObservation.set(null)
        Thread(
                {
                    for (root in roots) {
                        NativeAdminControls.editorSurfaceForget(root)
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
        requestImmediateFocus(project, file)
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
            )
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
        requestImmediateFocus(project, file)
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
                it.project === project && it.preferredFile != null
            }
        requestObservation(
            pendingSelection
                ?: PendingSurfaceObservation(
                    project = project,
                    preferredFile = null,
                    forceReconcile = false,
                ),
            delayMs = if (pendingSelection == null) 0L else SURFACE_COALESCE_MS,
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
