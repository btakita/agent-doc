package com.github.btakita.agentdoc

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.vfs.VirtualFile
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Reconciles tmux focus/layout with editor tab switches.
 *
 * Single-document tab-selection changes use Project Controller `focus_document_pane`;
 * split-layout tab selections submit non-destructive `sync_tmux_layout` work to the
 * Project Controller so a selected pane can be rescued back out of stash into the
 * agent-doc window.
 *
 * Guards against rapid-fire events:
 * - 100ms debounce so only the final burst state is acted upon
 * - Concurrency guard: a newer request replays immediately after the running command finishes
 * - Real markdown selection events force one guarded reconciliation pass so
 *   missing actors/supervisors can be cold-started even after exact-state dedup
 *
 * Registered in plugin.xml as a projectListener on FileEditorManagerListener.
 */
class EditorTabSyncListener : FileEditorManagerListener {
    @Volatile
    private var lastVisibleSignature: String? = null

    @Volatile
    private var lastFocusedFile: String? = null

    // #panefocussplit: the last file path we drove a focus reconcile for from an
    // editor focus-gained event. Focus events fire repeatedly for the same
    // editor, so this dedups consecutive focus-gained events on one file while
    // still reacting to every switch between different split editors.
    @Volatile
    private var lastFocusRequestedFile: String? = null

    private val latestSnapshot = java.util.concurrent.atomic.AtomicReference<AutomaticStateSnapshot?>(null)
    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "agent-doc-editor-tab-sync").apply { isDaemon = true }
    }

    companion object {
        private const val DEBOUNCE_MS = 100L
        private const val DEFERRED_RETRY_BASE_MS = 750L
        private const val DEFERRED_RETRY_MAX_MS = 5_000L
        private const val SYNC_GUARD_RETRY_MS = 1_000L
        private const val SYNC_TIMEOUT_REPLAY_DELAY_MS = 5_000L
        private const val AUTOMATIC_SYNC_TIMEOUT_MS = 5_000L
        private const val SYNC_TIMEOUT_BACKOFF_BASE_MS = 30_000L
        private const val SYNC_TIMEOUT_BACKOFF_MAX_MS = 300_000L
        private const val FOCUS_TIMEOUT_MS = 2_000L
        private const val MAX_DEFERRED_RETRIES = 8
        private val fallbackGeneration = AtomicLong(0)
        private val fallbackRunning = AtomicBoolean(false)
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
        private val GSON = com.google.gson.Gson()

        internal fun formatProjectControllerSyncLabel(columns: List<String>, focus: String): String =
            "project-controller:sync_tmux_layout " +
                columns.joinToString(" ") { "--col $it" } +
                " --focus $focus --exact-visible --no-autostart"

        internal fun formatCpcSyncHint(columns: List<String>, focus: String): String =
            "Sync: ${columns.joinToString(" ") { "--col $it" }} [focus: $focus]"
    }

    internal enum class AutomaticCommandKind {
        Focus,
        Sync,
    }

    internal data class AutomaticCommandPlan(
        val kind: AutomaticCommandKind,
        val visibleSignature: String,
    )

    internal data class AutomaticExecutionPlan(
        val plan: AutomaticCommandPlan,
        val projectRoot: String,
        val command: List<String>,
        val activeFile: String,
        val columns: List<String> = emptyList(),
    )

    internal data class AutomaticStateSnapshot(
        val activeFile: String,
        val focusedRelativePath: String,
        val focusedProjectRoot: String,
        val syncProjectRoot: String,
        val visibleMdFiles: List<String>,
        val editorLayout: EditorLayout?,
        val visibleSignature: String,
        val forceReconcile: Boolean = false,
    )

    internal data class AutomaticCommandResult(
        val applied: Boolean,
        val shouldRetry: Boolean,
    )

    internal object AutomaticCommandPlanner {
        private const val SAFE_PASSIVE_LAYOUT_RESELECTED_FOCUS_MARKER =
            "[sync] safe_passive_layout_preserved_reselected_focus"
        private const val SAFE_PASSIVE_LOCK_CONTENTION_RETRY_MARKER =
            "[sync] safe_passive_sync_lock_contention_retry"

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

        fun visibleSignature(
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout? = null,
        ): String {
            if (editorLayout != null) {
                return editorLayout.columns.joinToString("\u0000") { column ->
                    column.files
                        .filter { it.isNotEmpty() }
                        .distinct()
                        .joinToString("\u0001")
                }
            }
            return visibleMdFiles.distinct().sorted().joinToString("\u0000")
        }

        fun plan(
            visibleMdFiles: List<String>,
            visibleSignature: String,
            focusedFile: String,
            previousVisibleSignature: String?,
            previousFocusedFile: String?,
            forceReconcile: Boolean = false,
            layoutSynced: Boolean? = true,
        ): AutomaticCommandPlan? {
            if (visibleMdFiles.isEmpty()) return null

            if (
                !forceReconcile &&
                layoutSynced != false &&
                visibleSignature == previousVisibleSignature &&
                focusedFile == previousFocusedFile
            ) {
                return null
            }

            // #tmuxsyncstate: an editor focus event can arrive while the editor
            // split model is unchanged but the tmux panes have drifted/swapped.
            // The controller's lazily-backed sync state report is the authority
            // for that mismatch; focus-only would select the right pane but leave
            // it in the wrong column.
            if (!forceReconcile && layoutSynced == false) {
                return AutomaticCommandPlan(AutomaticCommandKind.Sync, visibleSignature)
            }

            // #panefocussteal: a pure focus change (same visible layout, the
            // operator just switched between open doc tabs) is the ONLY case that
            // should move tmux focus — route it through Project Controller
            // `focus_document_pane`.
            // `agent-doc sync` is layout-only and never
            // moves the operator's active pane (it neutralizes any internal
            // selection), so emitting Sync here would leave the doc-to-doc switch
            // dead. A changed visible layout still goes through Sync.
            if (!forceReconcile && visibleSignature == previousVisibleSignature) {
                return AutomaticCommandPlan(AutomaticCommandKind.Focus, visibleSignature)
            }

            return AutomaticCommandPlan(AutomaticCommandKind.Sync, visibleSignature)
        }

        /**
         * #panefocussplit: decide whether an editor focus-gained event should
         * trigger a tmux focus reconcile. Focus events fire repeatedly for the
         * same editor, so only act on markdown files whose path differs from the
         * last focus reconcile already requested. A switch to a different split
         * editor always changes the path and therefore always reconciles.
         */
        fun shouldReconcileFocusedFile(
            focusedFilePath: String,
            isMarkdown: Boolean,
            lastFocusRequestedFile: String?,
        ): Boolean = isMarkdown && focusedFilePath != lastFocusRequestedFile

        fun shouldReplayAfterRun(startedGeneration: Long, latestGeneration: Long): Boolean =
            latestGeneration > startedGeneration

        fun replayDelayAfterRun(
            startedGeneration: Long,
            latestGeneration: Long,
            commandTimedOut: Boolean,
        ): Long? {
            if (!shouldReplayAfterRun(startedGeneration, latestGeneration)) return null
            return if (commandTimedOut) SYNC_TIMEOUT_REPLAY_DELAY_MS else 0L
        }

        fun shouldScheduleDeferredRetry(startedGeneration: Long, latestGeneration: Long): Boolean =
            latestGeneration <= startedGeneration

        fun analyzeCommandResult(
            kind: AutomaticCommandKind,
            exitCode: Int,
            output: String,
        ): AutomaticCommandResult {
            if (exitCode != 0) {
                return AutomaticCommandResult(applied = false, shouldRetry = false)
            }
            if (
                kind == AutomaticCommandKind.Sync &&
                SyncLayoutAction.isPreservedLayoutOutput(output)
            ) {
                if (output.contains(SAFE_PASSIVE_LAYOUT_RESELECTED_FOCUS_MARKER)) {
                    return AutomaticCommandResult(applied = true, shouldRetry = false)
                }
                return AutomaticCommandResult(applied = false, shouldRetry = true)
            }
            if (
                kind == AutomaticCommandKind.Sync &&
                output.contains(SAFE_PASSIVE_LOCK_CONTENTION_RETRY_MARKER)
            ) {
                return AutomaticCommandResult(applied = false, shouldRetry = true)
            }
            return AutomaticCommandResult(applied = true, shouldRetry = false)
        }
    }

    @Volatile
    private var deferredRetryKey: String? = null

    @Volatile
    private var deferredRetryCount: Int = 0

    @Volatile
    private var syncTimeoutBackoffKey: String? = null

    @Volatile
    private var syncTimeoutBackoffCount: Int = 0

    @Volatile
    private var syncTimeoutRetryAfterMs: Long = 0L

    private fun log(msg: String) {
        LOG.debug("[layout-sync] $msg")
    }

    private fun warn(msg: String) {
        LOG.warn("[layout-sync] $msg")
    }

    private fun nextGeneration(): Long =
        fallbackGeneration.incrementAndGet()

    private fun currentGeneration(): Long =
        fallbackGeneration.get()

    private fun isCurrentGeneration(generation: Long): Boolean =
        currentGeneration() == generation

    private fun requestAutomaticSync(
        project: com.intellij.openapi.project.Project,
        snapshot: AutomaticStateSnapshot,
        delayMs: Long = DEBOUNCE_MS,
        requestedGeneration: Long? = null,
        immediateFocus: Boolean = false,
    ) {
        latestSnapshot.set(snapshot)
        val generation = requestedGeneration ?: nextGeneration()
        if (immediateFocus) {
            focusExistingPaneImmediately(snapshot, generation)
        }
        executor.schedule(sync@{
            try {
                if (!isCurrentGeneration(generation)) {
                    log("debounce: superseded gen=$generation")
                    return@sync
                }
                drainAutomaticSync(project, generation)
            } catch (e: Exception) {
                log("error: ${e.message}")
            }
        }, delayMs.coerceAtLeast(0L), TimeUnit.MILLISECONDS)
    }

    private fun focusExistingPaneImmediately(
        snapshot: AutomaticStateSnapshot,
        generation: Long,
    ) {
        executor.execute focus@{
            try {
                if (!isCurrentGeneration(generation)) {
                    log("focus: superseded gen=$generation")
                    return@focus
                }
                val result = CpcRouteClient.submitFocusDocumentPane(
                    projectRoot = snapshot.focusedProjectRoot,
                    documentPath = snapshot.activeFile,
                )
                if (result.exitCode == 0) {
                    log("focus: Project Controller submit accepted file=${snapshot.focusedRelativePath}")
                } else {
                    log("focus: skipped ${snapshot.focusedRelativePath}: ${result.output}")
                }
            } catch (e: Exception) {
                log("focus: skipped ${snapshot.focusedRelativePath}: ${e.message}")
            }
        }
    }

    private fun captureSnapshot(
        project: com.intellij.openapi.project.Project,
        preferredFile: com.intellij.openapi.vfs.VirtualFile? = null,
        forceReconcile: Boolean = false,
    ): AutomaticStateSnapshot? {
        val manager = FileEditorManager.getInstance(project)
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
        if (visibleMdFiles.isEmpty()) return null
        val preferredMarkdownFile = preferredFile?.takeIf { it.name.endsWith(".md") }
        val selectedEditorFile = manager.selectedTextEditor?.virtualFile
            ?.takeIf { it.name.endsWith(".md") }
        val activeFilePath = AutomaticCommandPlanner.resolveActiveFilePath(
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
        val activeFile = file.path

        val detectedEditorLayout = LayoutDetector.detectEditorLayout(project)
        val syncProjectRoot = SyncLayoutAction.chooseSyncProjectRoot(
            project.basePath,
            focusedProjectRoot,
            visibleMdFiles,
        )
        val absoluteEditorLayout = SyncLayoutAction.absolutizeEditorLayout(
            syncProjectRoot,
            SyncLayoutAction.normalizeEditorLayout(
                project.basePath,
                syncProjectRoot,
                detectedEditorLayout,
            ),
        )
        return AutomaticStateSnapshot(
            activeFile = activeFile,
            focusedRelativePath = focusedRelativePath,
            focusedProjectRoot = focusedProjectRoot,
            syncProjectRoot = syncProjectRoot,
            visibleMdFiles = visibleMdFiles,
            editorLayout = absoluteEditorLayout,
            visibleSignature = AutomaticCommandPlanner.visibleSignature(
                visibleMdFiles,
                absoluteEditorLayout,
            ),
            forceReconcile = forceReconcile,
        )
    }

    private fun queryLayoutSyncState(
        snapshot: AutomaticStateSnapshot,
        syncColumns: List<String>,
    ): Boolean? {
        if (syncColumns.size < 2) return true
        val report = CpcRouteClient.tmuxLayoutSyncState(
            projectRoot = snapshot.syncProjectRoot,
            columnsJson = GSON.toJson(syncColumns),
            focus = snapshot.activeFile,
        )
        if (report == null) {
            log("sync state: unavailable for ${snapshot.focusedRelativePath}")
            return false
        }
        log(
            "sync state: synced=${report.synced} reason=${report.reason} file=${snapshot.focusedRelativePath}"
        )
        return report.synced
    }

    private fun buildExecutionPlan(snapshot: AutomaticStateSnapshot): AutomaticExecutionPlan? {
        val syncColumns = SyncLayoutAction.buildSyncColumns(
            visibleMdFiles = snapshot.visibleMdFiles,
            editorLayout = snapshot.editorLayout,
        )
        val layoutSynced = queryLayoutSyncState(snapshot, syncColumns)
        val plan = AutomaticCommandPlanner.plan(
            visibleMdFiles = snapshot.visibleMdFiles,
            visibleSignature = snapshot.visibleSignature,
            focusedFile = snapshot.activeFile,
            previousVisibleSignature = lastVisibleSignature,
            previousFocusedFile = lastFocusedFile,
            forceReconcile = snapshot.forceReconcile,
            layoutSynced = layoutSynced,
        ) ?: return null

        val (projectRoot, cmd) = when (plan.kind) {
            AutomaticCommandKind.Focus -> {
                snapshot.focusedProjectRoot to listOf(
                    "project-controller:focus_document_pane",
                    snapshot.activeFile,
                )
            }
            AutomaticCommandKind.Sync -> {
                snapshot.syncProjectRoot to listOf("project-controller:sync_tmux_layout")
            }
        }

        return AutomaticExecutionPlan(
            plan = plan,
            projectRoot = projectRoot,
            command = cmd,
            activeFile = snapshot.activeFile,
            columns = syncColumns,
        )
    }

    private fun resetDeferredRetryState() {
        deferredRetryKey = null
        deferredRetryCount = 0
    }

    private fun syncBackoffKey(snapshot: AutomaticStateSnapshot): String =
        "${snapshot.visibleSignature}\u0000${snapshot.activeFile}"

    private fun syncTimeoutBackoffRemainingMs(snapshot: AutomaticStateSnapshot): Long {
        if (syncTimeoutBackoffKey != syncBackoffKey(snapshot)) return 0L
        val remaining = syncTimeoutRetryAfterMs - System.currentTimeMillis()
        return remaining.coerceAtLeast(0L)
    }

    private fun recordSyncTimeout(snapshot: AutomaticStateSnapshot): Long {
        val retryKey = syncBackoffKey(snapshot)
        if (syncTimeoutBackoffKey == retryKey) {
            syncTimeoutBackoffCount += 1
        } else {
            syncTimeoutBackoffKey = retryKey
            syncTimeoutBackoffCount = 1
        }
        val step = (syncTimeoutBackoffCount - 1).coerceAtLeast(0).coerceAtMost(4)
        val delayMs = (SYNC_TIMEOUT_BACKOFF_BASE_MS * (1L shl step))
            .coerceAtMost(SYNC_TIMEOUT_BACKOFF_MAX_MS)
        syncTimeoutRetryAfterMs = System.currentTimeMillis() + delayMs
        return delayMs
    }

    private fun resetSyncTimeoutBackoff() {
        syncTimeoutBackoffKey = null
        syncTimeoutBackoffCount = 0
        syncTimeoutRetryAfterMs = 0L
    }

    private fun summarizeOutput(output: String, maxChars: Int = 1_200): String {
        val trimmed = output.trim()
        if (trimmed.length <= maxChars) return trimmed
        val important = trimmed
            .lineSequence()
            .map { it.trim() }
            .filter {
                it.contains("timeout", ignoreCase = true) ||
                    it.contains("latency budget exceeded") ||
                    it.contains("preserved the current tmux layout") ||
                    it.contains("safe_passive") ||
                    it.contains("error", ignoreCase = true)
            }
            .distinct()
            .joinToString("\n")
            .take(maxChars)
        val summary = important.ifBlank { trimmed.take(maxChars) }
        return "$summary\n[truncated ${trimmed.length} chars]"
    }

    private fun nextDeferredRetryDelayMs(retryCount: Int): Long {
        val step = (retryCount - 1).coerceAtLeast(0)
        val delay = DEFERRED_RETRY_BASE_MS * (1L shl step.coerceAtMost(3))
        return delay.coerceAtMost(DEFERRED_RETRY_MAX_MS)
    }

    private fun registerDeferredRetry(snapshot: AutomaticStateSnapshot): Long? {
        val retryKey = "${snapshot.visibleSignature}\u0000${snapshot.activeFile}"
        if (deferredRetryKey == retryKey) {
            deferredRetryCount += 1
        } else {
            deferredRetryKey = retryKey
            deferredRetryCount = 1
        }
        if (deferredRetryCount > MAX_DEFERRED_RETRIES) {
            return null
        }
        return nextDeferredRetryDelayMs(deferredRetryCount)
    }

    private fun drainAutomaticSync(
        project: com.intellij.openapi.project.Project,
        requestedGeneration: Long,
    ) {
        val locked = fallbackRunning.compareAndSet(false, true)
        if (!locked) {
            log("guard: layout already running, queued latest request")
            latestSnapshot.get()?.let {
                requestAutomaticSync(project, it, SYNC_GUARD_RETRY_MS, requestedGeneration)
            }
            return
        }

        val startedGeneration = requestedGeneration
        var commandTimedOut = false
        try {
            val snapshot = latestSnapshot.get()
            val execution = snapshot?.let { buildExecutionPlan(it) }
            if (execution == null) {
                log("dedup: selection state already synchronized")
            } else {
                if (execution.plan.kind == AutomaticCommandKind.Sync) {
                    val backoffMs = syncTimeoutBackoffRemainingMs(snapshot)
                    if (backoffMs > 0L) {
                        log("backoff: automatic sync delayed ${backoffMs}ms after prior timeout for ${execution.activeFile}")
                        requestAutomaticSync(project, snapshot, backoffMs, requestedGeneration)
                        return
                    }
                }
                val cmd = execution.command
                when (execution.plan.kind) {
                    AutomaticCommandKind.Focus -> log("submit: ${cmd.joinToString(" ")}")
                    AutomaticCommandKind.Sync -> {
                        log("submit: ${formatProjectControllerSyncLabel(execution.columns, execution.activeFile)}")
                        TerminalUtil.showHint(
                            project,
                            formatCpcSyncHint(execution.columns, execution.activeFile),
                        )
                    }
                }
                val timeoutMs = when (execution.plan.kind) {
                    AutomaticCommandKind.Focus -> FOCUS_TIMEOUT_MS
                    AutomaticCommandKind.Sync -> AUTOMATIC_SYNC_TIMEOUT_MS
                }
                val processResult = if (execution.plan.kind == AutomaticCommandKind.Focus) {
                    val result = CpcRouteClient.submitFocusDocumentPane(
                        projectRoot = execution.projectRoot,
                        documentPath = execution.activeFile,
                    )
                    SyncLayoutAction.Companion.SyncProcessResult(
                        exitCode = result.exitCode,
                        output = result.output,
                        timedOut = false,
                    )
                } else {
                    val result = CpcRouteClient.submitSyncTmuxLayout(
                        projectRoot = execution.projectRoot,
                        columnsJson = GSON.toJson(execution.columns),
                        window = null,
                        focus = execution.activeFile,
                        noAutostart = true,
                        exactVisible = true,
                        callerKind = "automatic",
                    )
                    SyncLayoutAction.Companion.SyncProcessResult(
                        exitCode = result.exitCode,
                        output = result.output,
                        timedOut = false,
                    )
                }
                val output = processResult.output
                val exitCode = processResult.exitCode
                commandTimedOut = processResult.timedOut
                log(
                    "result: kind=${execution.plan.kind.name.lowercase()} exit=$exitCode timedOut=${processResult.timedOut} " +
                        "timeoutMs=$timeoutMs output=${summarizeOutput(output)}"
                )
                if (processResult.timedOut) {
                    if (execution.plan.kind == AutomaticCommandKind.Sync) {
                        val delayMs = recordSyncTimeout(snapshot)
                        warn(
                            "timeout: automatic sync exceeded ${timeoutMs}ms; backing off ${delayMs}ms before retry"
                        )
                        if (isCurrentGeneration(startedGeneration)) {
                            requestAutomaticSync(project, snapshot, delayMs, startedGeneration)
                        }
                    } else {
                        warn("timeout: focus command exceeded ${timeoutMs}ms; skipped automatic focus")
                    }
                }
                val result = AutomaticCommandPlanner.analyzeCommandResult(
                    kind = execution.plan.kind,
                    exitCode = exitCode,
                    output = output,
                )
                if (result.applied) {
                    lastVisibleSignature = execution.plan.visibleSignature
                    lastFocusedFile = execution.activeFile
                    resetDeferredRetryState()
                    resetSyncTimeoutBackoff()
                } else if (result.shouldRetry) {
                    if (isCurrentGeneration(startedGeneration)) {
                        val delayMs = registerDeferredRetry(snapshot)
                        if (delayMs != null) {
                            log(
                                "deferred: passive sync preserved layout for ${execution.activeFile}; retry $deferredRetryCount/$MAX_DEFERRED_RETRIES in ${delayMs}ms"
                            )
                            requestAutomaticSync(project, snapshot, delayMs)
                        } else {
                            log(
                                "deferred: passive sync preserved layout for ${execution.activeFile}; retry budget exhausted after $MAX_DEFERRED_RETRIES attempts"
                            )
                        }
                    } else {
                        log(
                            "deferred: skipped superseded passive sync retry for ${execution.activeFile}; latest request will replay"
                        )
                    }
                } else if (exitCode != 0 && !processResult.timedOut) {
                    resetDeferredRetryState()
                    resetSyncTimeoutBackoff()
                }
            }
        } finally {
            fallbackRunning.set(false)
            val replayDelayMs = AutomaticCommandPlanner.replayDelayAfterRun(
                startedGeneration,
                currentGeneration(),
                commandTimedOut,
            )
            if (replayDelayMs != null) {
                val replayKind = if (replayDelayMs > 0) "delayed" else "immediate"
                log("queue: replaying latest automatic sync request ($replayKind, delayMs=$replayDelayMs)")
                latestSnapshot.get()?.let { requestAutomaticSync(project, it, replayDelayMs) }
            }
        }
    }

    override fun selectionChanged(event: FileEditorManagerEvent) {
        val file = event.newFile ?: return
        if (!file.name.endsWith(".md")) return

        val manager = FileEditorManager.getInstance(event.manager.project)
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
        log("selectionChanged: newFile=${file.name} mdFiles=$visibleMdFiles")
        if (visibleMdFiles.isEmpty()) return

        val snapshot = captureSnapshot(event.manager.project, file, forceReconcile = true) ?: return
        requestAutomaticSync(event.manager.project, snapshot, immediateFocus = true)
    }

    /**
     * The operator moved editor focus to [file] — e.g. clicked into the other
     * split editor window showing an already-open agent-doc document.
     *
     * #panefocussplit: [FileEditorManagerListener.selectionChanged] does NOT
     * fire for focus movement between two existing split editors (only for tab /
     * visible-file-set changes), so without this entry point split navigation
     * never moves the tmux active pane. Reuses the same debounced, generation-
     * guarded reconcile as [selectionChanged]; [EditorFocusSyncListener] wires
     * the per-editor focus events that call this.
     */
    fun onEditorFocusGained(project: com.intellij.openapi.project.Project, file: VirtualFile) {
        if (!AutomaticCommandPlanner.shouldReconcileFocusedFile(
                focusedFilePath = file.path,
                isMarkdown = file.name.endsWith(".md"),
                lastFocusRequestedFile = lastFocusRequestedFile,
            )
        ) {
            return
        }
        val manager = FileEditorManager.getInstance(project)
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
        if (visibleMdFiles.isEmpty()) return
        lastFocusRequestedFile = file.path
        log("focusGained: file=${file.name} mdFiles=$visibleMdFiles")
        val snapshot = captureSnapshot(project, file, forceReconcile = false) ?: return
        requestAutomaticSync(project, snapshot, immediateFocus = true)
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
