package com.github.btakita.agentdoc

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Reconciles tmux focus/layout with editor tab switches.
 *
 * Single-document tab-selection changes use `agent-doc focus`; split-layout
 * tab selections stay on non-destructive `agent-doc sync --no-autostart` so
 * a selected pane can be rescued back out of stash into the agent-doc window.
 *
 * Guards against rapid-fire events:
 * - 100ms debounce so only the final burst state is acted upon
 * - Concurrency guard: a newer request replays immediately after the running command finishes
 *
 * Registered in plugin.xml as a projectListener on FileEditorManagerListener.
 */
class EditorTabSyncListener : FileEditorManagerListener {
    @Volatile
    private var lastVisibleSignature: String? = null

    @Volatile
    private var lastFocusedFile: String? = null

    private val latestSnapshot = java.util.concurrent.atomic.AtomicReference<AutomaticStateSnapshot?>(null)

    companion object {
        private const val DEBOUNCE_MS = 100L
        private const val DEFERRED_RETRY_BASE_MS = 750L
        private const val DEFERRED_RETRY_MAX_MS = 5_000L
        private const val SYNC_GUARD_RETRY_MS = 150L
        private const val MAX_DEFERRED_RETRIES = 8
        private val fallbackGeneration = AtomicLong(0)
        private val fallbackRunning = AtomicBoolean(false)
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
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
    )

    internal data class AutomaticStateSnapshot(
        val activeFile: String,
        val focusedRelativePath: String,
        val focusedProjectRoot: String,
        val syncProjectRoot: String,
        val visibleMdFiles: List<String>,
        val editorLayout: EditorLayout?,
        val visibleSignature: String,
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
        ): AutomaticCommandPlan? {
            if (visibleMdFiles.isEmpty()) return null

            if (visibleSignature == previousVisibleSignature && focusedFile == previousFocusedFile) {
                return null
            }

            return AutomaticCommandPlan(AutomaticCommandKind.Sync, visibleSignature)
        }

        fun shouldReplayAfterRun(startedGeneration: Long, latestGeneration: Long): Boolean =
            latestGeneration > startedGeneration

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

    private fun log(msg: String) {
        LOG.info("[layout-sync] $msg")
    }

    private fun nextGeneration(lib: AgentDocLib?): Long =
        lib?.agent_doc_sync_bump_generation() ?: fallbackGeneration.incrementAndGet()

    private fun isCurrentGeneration(lib: AgentDocLib?, generation: Long): Boolean =
        lib?.agent_doc_sync_check_generation(generation) ?: (fallbackGeneration.get() == generation)

    private fun requestAutomaticSync(
        project: com.intellij.openapi.project.Project,
        snapshot: AutomaticStateSnapshot,
        delayMs: Long = DEBOUNCE_MS,
        requestedGeneration: Long? = null,
    ) {
        latestSnapshot.set(snapshot)
        val lib = AgentDocLib.get()
        val generation = requestedGeneration ?: nextGeneration(lib)
        Thread {
            try {
                if (delayMs > 0) {
                    Thread.sleep(delayMs)
                }
                if (!isCurrentGeneration(lib, generation)) {
                    log("debounce: superseded gen=$generation")
                    return@Thread
                }
                drainAutomaticSync(project, generation)
            } catch (e: Exception) {
                log("error: ${e.message}")
            }
        }.apply {
            isDaemon = true
            start()
        }
    }

    private fun captureSnapshot(
        project: com.intellij.openapi.project.Project,
        preferredFile: com.intellij.openapi.vfs.VirtualFile? = null,
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
        )
    }

    private fun buildExecutionPlan(snapshot: AutomaticStateSnapshot): AutomaticExecutionPlan? {
        val plan = AutomaticCommandPlanner.plan(
            visibleMdFiles = snapshot.visibleMdFiles,
            visibleSignature = snapshot.visibleSignature,
            focusedFile = snapshot.activeFile,
            previousVisibleSignature = lastVisibleSignature,
            previousFocusedFile = lastFocusedFile,
        ) ?: return null

        val (projectRoot, cmd) = when (plan.kind) {
            AutomaticCommandKind.Focus -> {
                val agentDoc = TerminalUtil.resolveAgentDoc(snapshot.focusedProjectRoot)
                snapshot.focusedProjectRoot to SyncLayoutAction.buildFocusCommand(
                    agentDoc = agentDoc,
                    focusedFile = snapshot.focusedRelativePath,
                )
            }
            AutomaticCommandKind.Sync -> {
                val agentDoc = TerminalUtil.resolveAgentDoc(snapshot.syncProjectRoot)
                snapshot.syncProjectRoot to SyncLayoutAction.buildSyncCommand(
                    agentDoc = agentDoc,
                    visibleMdFiles = snapshot.visibleMdFiles,
                    editorLayout = snapshot.editorLayout,
                    focusedFile = snapshot.activeFile,
                    noAutostart = true,
                )
            }
        }

        return AutomaticExecutionPlan(
            plan = plan,
            projectRoot = projectRoot,
            command = cmd,
            activeFile = snapshot.activeFile,
        )
    }

    private fun resetDeferredRetryState() {
        deferredRetryKey = null
        deferredRetryCount = 0
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
        val lib = AgentDocLib.get()
        val locked = lib?.agent_doc_sync_try_lock()
            ?: fallbackRunning.compareAndSet(false, true)
        if (!locked) {
            log("guard: layout already running, queued latest request")
            latestSnapshot.get()?.let {
                requestAutomaticSync(project, it, SYNC_GUARD_RETRY_MS, requestedGeneration)
            }
            return
        }

        var startedGeneration = requestedGeneration
        try {
            val snapshot = latestSnapshot.get()
            val execution = snapshot?.let { buildExecutionPlan(it) }
            if (execution == null) {
                log("dedup: selection state already synchronized")
            } else {
                val cmd = execution.command
                log("exec: ${cmd.joinToString(" ")}")
                TerminalUtil.showHint(project, TerminalUtil.formatLayoutSummary(cmd))
                val process = ProcessBuilder(cmd)
                    .directory(java.io.File(execution.projectRoot))
                    .redirectErrorStream(true)
                    .start()
                val output = process.inputStream.bufferedReader().readText()
                val exitCode = process.waitFor()
                log("result: exit=$exitCode output=${output.trim()}")
                val result = AutomaticCommandPlanner.analyzeCommandResult(
                    kind = execution.plan.kind,
                    exitCode = exitCode,
                    output = output,
                )
                if (result.applied) {
                    lastVisibleSignature = execution.plan.visibleSignature
                    lastFocusedFile = execution.activeFile
                    resetDeferredRetryState()
                } else if (result.shouldRetry) {
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
                } else if (exitCode != 0) {
                    resetDeferredRetryState()
                }
            }
        } finally {
            lib?.agent_doc_sync_unlock() ?: fallbackRunning.set(false)
            if (!isCurrentGeneration(lib, startedGeneration)) {
                log("queue: replaying latest automatic sync request")
                latestSnapshot.get()?.let { requestAutomaticSync(project, it, 0) }
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

        val snapshot = captureSnapshot(event.manager.project, file) ?: return
        requestAutomaticSync(event.manager.project, snapshot)
    }
}
