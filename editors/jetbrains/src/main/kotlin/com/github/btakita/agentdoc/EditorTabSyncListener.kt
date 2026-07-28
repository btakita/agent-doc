package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/**
 * Reports what this editor looks like; the tmux consequence is derived elsewhere
 * (`#jbsurfaceswap` / `#jbpluginlazilyeffects`).
 *
 * Every tab selection and split-focus change produces **one observation** —
 * focused document, visible markdown set, column layout — handed to
 * `agent_doc_editor_surface_observe_json`. The reactive graph behind that entry
 * point folds the observation against what tmux was last reconciled against,
 * derives focus-vs-sync, and runs the Project Controller command as an `Effect`.
 *
 * The plugin therefore holds no plan, no previous-signature field, and no retry
 * ladder: an observation identical to the last one is idle and costs nothing, so
 * repeat events need no dedup here. What remains is event-storm handling that is
 * genuinely the editor's: a 100ms debounce plus a generation guard so a burst
 * reports only its final state, and an off-EDT executor so the derived command
 * never blocks the UI thread.
 *
 * Focus is latency-sensitive and takes a separate zero-debounce lane. The
 * selected document is sent through the controller's project-scoped latest-wins
 * focus command before layout detection/reconciliation begins. The debounced
 * surface observation still owns safe passive layout sync; the fast lane only
 * makes an already-visible target pane react immediately.
 *
 * Registered from [PluginLifecycleListener] via [install] so it survives
 * hot-reload.
 */
class EditorTabSyncListener : FileEditorManagerListener {
    private val latestSurface = AtomicReference<PendingSurface?>(null)

    /**
     * Every project root this instance has observed. A root's graph holds the
     * reconciled-layout history, so it is released on project close through
     * `agent_doc_editor_surface_forget`.
     */
    private val observedRoots: MutableSet<String> = ConcurrentHashMap.newKeySet()

    /**
     * Debounce generation. Per-instance, so one project's tab churn cannot
     * supersede another project's pending observation.
     */
    private val generation = AtomicLong(0)
    private val focusGeneration = AtomicLong(0)

    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-editor-tab-sync").apply { isDaemon = true }
    }
    private val focusExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "agent-doc-editor-focus-sync").apply { isDaemon = true }
    }

    companion object {
        private const val DEBOUNCE_MS = 100L
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
        private val GSON = com.google.gson.Gson()
        private val instances = ConcurrentHashMap<Project, EditorTabSyncListener>()

        fun install(project: Project): EditorTabSyncListener =
            instances.computeIfAbsent(project) { EditorTabSyncListener() }

        /** Release the project's surface graphs and stop its debounce executor. */
        fun disposeProject(project: Project) {
            instances.remove(project)?.shutdown()
        }

        internal fun formatCpSyncHint(columns: List<String>, focus: String): String =
            "Sync: ${columns.joinToString(" ") { "--col $it" }} [focus: $focus]"

        /**
         * The user-visible hint for an observation receipt, or `null` when the
         * derived intent was idle or a pure focus move (which needs no hint —
         * the operator just moved between documents they can already see).
         */
        internal fun syncHintFromReceipt(receiptJson: String?): String? {
            val intent = intentObject(receiptJson) ?: return null
            if (intent.get("kind")?.asString != "sync") return null
            val document = intent.get("document")?.asString ?: return null
            val columns = intent.getAsJsonArray("columns")
                ?.map { column ->
                    column.asJsonObject.getAsJsonArray("files")
                        .joinToString(",") { it.asString }
                }
                .orEmpty()
            return formatCpSyncHint(columns, document)
        }

        /** The `kind` of the intent a receipt reports, for diagnostics. */
        internal fun intentKindFromReceipt(receiptJson: String?): String? =
            intentObject(receiptJson)?.get("kind")?.asString

        private fun intentObject(receiptJson: String?): com.google.gson.JsonObject? {
            if (receiptJson.isNullOrBlank()) return null
            return try {
                JsonParser.parseString(receiptJson)
                    .asJsonObject
                    .getAsJsonObject("intent")
            } catch (e: Exception) {
                LOG.warn("[layout-sync] unparseable surface receipt: ${e.message}")
                null
            }
        }
    }

    /** One column of the reported split layout. Wire shape of Rust `SurfaceColumn`. */
    internal data class SurfaceColumnPayload(val files: List<String>)

    /**
     * What the editor looks like right now. Wire shape of Rust `EditorSurface`.
     *
     * Every field is something the editor saw. Notably absent is any notion of
     * whether tmux agrees: that is derived by comparing this observation against
     * the controller's own, so the plugin never reports a fact it would have to
     * ask the controller for.
     */
    internal data class EditorSurfacePayload(
        val focused: String,
        val visible: List<String>,
        val columns: List<SurfaceColumnPayload>,
        @SerializedName("force_reconcile") val forceReconcile: Boolean,
    )

    private data class PendingSurface(
        val project: Project,
        val projectRoot: String,
        val relativePath: String,
        val surfaceJson: String,
    )

    internal object SurfaceReport {
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
         * Build the observation. An undetected layout reports **no** columns
         * rather than a synthesized single column, so the graph can tell "the
         * editor has one column" apart from "the editor could not see its
         * layout" and skip the drift comparison in the latter case.
         */
        fun buildSurface(
            focusedFile: String,
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
            forceReconcile: Boolean,
        ): EditorSurfacePayload = EditorSurfacePayload(
            focused = focusedFile,
            visible = visibleMdFiles.distinct(),
            columns = editorLayout
                ?.columns
                ?.map { column -> SurfaceColumnPayload(column.files.filter { it.isNotBlank() }.distinct()) }
                ?.filter { it.files.isNotEmpty() }
                .orEmpty(),
            forceReconcile = forceReconcile,
        )
    }

    private fun log(msg: String) {
        LOG.debug("[layout-sync] $msg")
    }

    private fun requestObservation(pending: PendingSurface, delayMs: Long = DEBOUNCE_MS) {
        latestSurface.set(pending)
        val requested = generation.incrementAndGet()
        executor.schedule(observe@{
            try {
                if (generation.get() != requested) {
                    log("debounce: superseded gen=$requested")
                    return@observe
                }
                reportLatestSurface()
            } catch (e: Exception) {
                LOG.warn("[layout-sync] observation failed: ${e.message}")
            }
        }, delayMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)
    }

    private fun reportLatestSurface() {
        val pending = latestSurface.get() ?: return
        observedRoots.add(pending.projectRoot)
        val receipt = NativeAdminControls.editorSurfaceObserve(
            projectRoot = pending.projectRoot,
            surfaceJson = pending.surfaceJson,
        )
        if (receipt == null) {
            LOG.warn("[layout-sync] surface observation unavailable for ${pending.relativePath}")
            return
        }
        log("observe: file=${pending.relativePath} intent=${intentKindFromReceipt(receipt)} receipt=$receipt")
        syncHintFromReceipt(receipt)?.let { hint ->
            TerminalUtil.showHint(pending.project, hint)
        }
    }

    private fun requestImmediateFocus(project: Project, file: VirtualFile) {
        val documentPath = file.path
        val requested = focusGeneration.incrementAndGet()
        TmuxPaneFocusSync.recordEditorFocusIntent(project, documentPath)
        focusExecutor.execute focus@{
            if (focusGeneration.get() != requested) {
                log("focus: superseded gen=$requested")
                return@focus
            }
            // Project-root discovery crosses native/path services. Keep it off
            // the IntelliJ event thread along with the controller round trip.
            val (projectRoot, _) = TerminalUtil.resolveProject(project, file)
            val receipt = NativeAdminControls.focusDocumentPane(
                projectRoot = projectRoot,
                documentPath = documentPath,
            )
            log("focus: file=$documentPath receipt=$receipt")
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
        val preferredMarkdownFile = preferredFile?.takeIf { it.name.endsWith(".md") }
        val selectedEditorFile = manager.selectedTextEditor?.virtualFile
            ?.takeIf { it.name.endsWith(".md") }
        val activeFilePath = SurfaceReport.resolveActiveFilePath(
            preferredActiveFile = preferredMarkdownFile?.path,
            selectedEditorFile = selectedEditorFile?.path,
            visibleMdFiles = visibleMdFiles,
        ) ?: return null
        val file = sequenceOf(
            preferredMarkdownFile,
            selectedEditorFile,
            manager.selectedFiles.firstOrNull { it.name.endsWith(".md") },
        ).filterNotNull().firstOrNull { it.path == activeFilePath } ?: return null

        val (focusedProjectRoot, focusedRelativePath) = TerminalUtil.resolveProject(project, file)
        // One root keys the surface graph, and it has to be the one that spans
        // the whole visible layout — a surface is the layout, not one document.
        val surfaceProjectRoot = SyncLayoutAction.chooseSyncProjectRoot(
            project.basePath,
            focusedProjectRoot,
            visibleMdFiles,
        )
        val absoluteEditorLayout = SyncLayoutAction.absolutizeEditorLayout(
            surfaceProjectRoot,
            SyncLayoutAction.normalizeEditorLayout(
                project.basePath,
                surfaceProjectRoot,
                LayoutDetector.detectEditorLayout(project),
            ),
        )
        return PendingSurface(
            project = project,
            projectRoot = surfaceProjectRoot,
            relativePath = focusedRelativePath,
            surfaceJson = GSON.toJson(
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
        for (root in observedRoots) {
            NativeAdminControls.editorSurfaceForget(root)
        }
        observedRoots.clear()
        latestSurface.set(null)
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

        // A real selection event is the operator asking for this document, so it
        // skips the unchanged-observation shortcut: a missing actor/supervisor
        // can still be cold-started when nothing about the surface changed.
        val pending = captureSurface(project, file, forceReconcile = true) ?: return
        requestObservation(pending)
    }

    /**
     * The operator moved editor focus to [file] — e.g. clicked into the other
     * split editor window showing an already-open agent-doc document.
     *
     * #panefocussplit: [FileEditorManagerListener.selectionChanged] does NOT
     * fire for focus movement between two existing split editors (only for tab /
     * visible-file-set changes), so without this entry point split navigation
     * never moves the tmux active pane. [EditorFocusSyncListener] wires the
     * per-editor focus events that call this.
     *
     * Focus events fire repeatedly for the same editor. That used to need a
     * `lastFocusRequestedFile` field here; now a repeat produces the identical
     * observation, which the graph reports as idle and acts on not at all.
     */
    fun onEditorFocusGained(project: Project, file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        requestImmediateFocus(project, file)
        val manager = FileEditorManager.getInstance(project)
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
        if (visibleMdFiles.isEmpty()) return
        log("focusGained: file=${file.name} mdFiles=$visibleMdFiles")
        val pending = captureSurface(project, file, forceReconcile = false) ?: return
        requestObservation(pending)
    }

    /**
     * Evict the per-document state-projection mirror + generation counters when an
     * editor tab closes (`#jbmirrorevict` / `#nsq2`). Without this the
     * `StateProjectionBridge` maps grow monotonically and a reused path
     * (move/symlink/reopen) surfaces the prior document's stale projection state.
     * Re-subscription lazily re-creates the mirror from a fresh cold snapshot.
     */
    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        StateProjectionBridge.evictForFile(file.path)
    }
}
