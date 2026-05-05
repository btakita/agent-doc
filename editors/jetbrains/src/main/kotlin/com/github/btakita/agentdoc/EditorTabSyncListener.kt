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

    companion object {
        private const val DEBOUNCE_MS = 100L
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

    internal object AutomaticCommandPlanner {
        fun visibleSignature(visibleMdFiles: List<String>): String =
            visibleMdFiles.distinct().sorted().joinToString("\u0000")

        fun plan(
            visibleMdFiles: List<String>,
            focusedFile: String,
            previousVisibleSignature: String?,
            previousFocusedFile: String?,
        ): AutomaticCommandPlan? {
            if (visibleMdFiles.isEmpty()) return null

            val visibleSignature = visibleSignature(visibleMdFiles)
            if (visibleSignature == previousVisibleSignature && focusedFile == previousFocusedFile) {
                return null
            }

            val kind = if (
                visibleSignature != previousVisibleSignature ||
                visibleMdFiles.size > 1
            ) {
                AutomaticCommandKind.Sync
            } else {
                AutomaticCommandKind.Focus
            }
            return AutomaticCommandPlan(kind, visibleSignature)
        }

        fun shouldReplayAfterRun(startedGeneration: Long, latestGeneration: Long): Boolean =
            latestGeneration > startedGeneration
    }

    private fun log(msg: String) {
        LOG.info("[layout-sync] $msg")
    }

    private fun nextGeneration(lib: AgentDocLib?): Long =
        lib?.agent_doc_sync_bump_generation() ?: fallbackGeneration.incrementAndGet()

    private fun isCurrentGeneration(lib: AgentDocLib?, generation: Long): Boolean =
        lib?.agent_doc_sync_check_generation(generation) ?: (fallbackGeneration.get() == generation)

    private fun requestAutomaticSync(
        project: com.intellij.openapi.project.Project,
        delayMs: Long = DEBOUNCE_MS,
    ) {
        val lib = AgentDocLib.get()
        val requestedGeneration = nextGeneration(lib)
        Thread {
            try {
                if (delayMs > 0) {
                    Thread.sleep(delayMs)
                }
                if (!isCurrentGeneration(lib, requestedGeneration)) {
                    log("debounce: superseded gen=$requestedGeneration")
                    return@Thread
                }
                drainAutomaticSync(project, requestedGeneration)
            } catch (e: Exception) {
                log("error: ${e.message}")
            }
        }.apply {
            isDaemon = true
            start()
        }
    }

    private fun buildExecutionPlan(project: com.intellij.openapi.project.Project): AutomaticExecutionPlan? {
        val manager = FileEditorManager.getInstance(project)
        val file = manager.selectedTextEditor?.virtualFile
            ?.takeIf { it.name.endsWith(".md") }
            ?: manager.selectedFiles.firstOrNull { it.name.endsWith(".md") }
            ?: return null

        val (focusedProjectRoot, focusedRelativePath) = TerminalUtil.resolveProject(project, file)
        val activeFile = file.path
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
        if (visibleMdFiles.isEmpty()) return null

        val plan = AutomaticCommandPlanner.plan(
            visibleMdFiles = visibleMdFiles,
            focusedFile = activeFile,
            previousVisibleSignature = lastVisibleSignature,
            previousFocusedFile = lastFocusedFile,
        ) ?: return null

        val detectedEditorLayout = LayoutDetector.detectEditorLayout(project)
        val (projectRoot, cmd) = when (plan.kind) {
            AutomaticCommandKind.Focus -> {
                val agentDoc = TerminalUtil.resolveAgentDoc(focusedProjectRoot)
                focusedProjectRoot to SyncLayoutAction.buildFocusCommand(
                    agentDoc = agentDoc,
                    focusedFile = focusedRelativePath,
                )
            }
            AutomaticCommandKind.Sync -> {
                val syncProjectRoot = SyncLayoutAction.chooseSyncProjectRoot(
                    project.basePath,
                    focusedProjectRoot,
                    visibleMdFiles,
                )
                val agentDoc = TerminalUtil.resolveAgentDoc(syncProjectRoot)
                val absoluteEditorLayout = SyncLayoutAction.absolutizeEditorLayout(
                    syncProjectRoot,
                    SyncLayoutAction.normalizeEditorLayout(
                        project.basePath,
                        syncProjectRoot,
                        detectedEditorLayout,
                    ),
                )
                syncProjectRoot to SyncLayoutAction.buildSyncCommand(
                    agentDoc = agentDoc,
                    visibleMdFiles = visibleMdFiles,
                    editorLayout = absoluteEditorLayout,
                    focusedFile = activeFile,
                    noAutostart = true,
                )
            }
        }

        return AutomaticExecutionPlan(
            plan = plan,
            projectRoot = projectRoot,
            command = cmd,
            activeFile = activeFile,
        )
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
            return
        }

        var startedGeneration = requestedGeneration
        try {
            val execution = buildExecutionPlan(project)
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
                if (exitCode == 0) {
                    lastVisibleSignature = execution.plan.visibleSignature
                    lastFocusedFile = execution.activeFile
                }
            }
        } finally {
            lib?.agent_doc_sync_unlock() ?: fallbackRunning.set(false)
            if (!isCurrentGeneration(lib, startedGeneration)) {
                log("queue: replaying latest automatic sync request")
                requestAutomaticSync(project, 0)
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

        requestAutomaticSync(event.manager.project)
    }
}
