package com.github.btakita.agentdoc

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Syncs tmux pane layout with editor tab switches.
 *
 * The plugin is a thin layout reporter — all sync intelligence
 * (dedup, fast-path, eviction) lives in `agent-doc sync`.
 *
 * Guards against rapid-fire events:
 * - 500ms debounce so only the final state is acted upon
 * - Concurrency guard: skips if a layout command is already running
 *
 * Registered in plugin.xml as a projectListener on FileEditorManagerListener.
 */
class EditorTabSyncListener : FileEditorManagerListener {

    companion object {
        private const val DEBOUNCE_MS = 500L
        private val fallbackGeneration = AtomicLong(0)
        private val fallbackRunning = AtomicBoolean(false)
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
    }

    private fun log(msg: String) {
        LOG.info("[layout-sync] $msg")
    }

    override fun selectionChanged(event: FileEditorManagerEvent) {
        val file = event.newFile ?: return
        if (!file.name.endsWith(".md")) return

        val project = event.manager.project
        val (projectRoot, _) = TerminalUtil.resolveProject(project, file)
        val activeFile = file.path

        // Collect all visible .md files across split panes.
        val manager = FileEditorManager.getInstance(project)
        val allSelected = manager.selectedFiles.toList()
        val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)

        log("selectionChanged: newFile=${file.name} allSelected=[${allSelected.joinToString { it.name }}] mdFiles=$visibleMdFiles")

        if (visibleMdFiles.isEmpty()) return

        val editorLayout = SyncLayoutAction.normalizeEditorLayout(
            project.basePath,
            projectRoot,
            LayoutDetector.detectEditorLayout(project),
        )

        // Debounce: bump generation via FFI (shared across editors), fallback to local counter.
        val lib = AgentDocLib.get()
        val myGen = lib?.agent_doc_sync_bump_generation() ?: fallbackGeneration.incrementAndGet()

        Thread {
            try {
                Thread.sleep(DEBOUNCE_MS)
                val isCurrent = lib?.agent_doc_sync_check_generation(myGen)
                    ?: (fallbackGeneration.get() == myGen)
                if (!isCurrent) {
                    log("debounce: superseded gen=$myGen")
                    return@Thread
                }

                // Concurrency guard via FFI (shared across editors), fallback to local.
                val locked = lib?.agent_doc_sync_try_lock()
                    ?: fallbackRunning.compareAndSet(false, true)
                if (!locked) {
                    log("guard: layout already running, skipping")
                    return@Thread
                }

                try {
                    val agentDoc = TerminalUtil.resolveAgentDoc(projectRoot)
                    val absoluteEditorLayout = SyncLayoutAction.absolutizeEditorLayout(
                        projectRoot,
                        editorLayout,
                    )
                    val cmd = SyncLayoutAction.buildSyncCommand(
                        agentDoc = agentDoc,
                        visibleMdFiles = visibleMdFiles,
                        editorLayout = absoluteEditorLayout,
                        focusedFile = activeFile,
                        noAutostart = false,
                    )
                    log("exec: ${cmd.joinToString(" ")}")
                    val summary = TerminalUtil.formatLayoutSummary(cmd)
                    TerminalUtil.showHint(project, summary)
                    val process = ProcessBuilder(cmd)
                        .directory(java.io.File(projectRoot))
                        .redirectErrorStream(true)
                        .start()
                    val output = process.inputStream.bufferedReader().readText()
                    val exitCode = process.waitFor()
                    log("result: exit=$exitCode output=${output.trim()}")
                } finally {
                    lib?.agent_doc_sync_unlock() ?: fallbackRunning.set(false)
                }
            } catch (e: Exception) {
                lib?.agent_doc_sync_unlock() ?: fallbackRunning.set(false)
                log("error: ${e.message}")
            }
        }.start()
    }
}
