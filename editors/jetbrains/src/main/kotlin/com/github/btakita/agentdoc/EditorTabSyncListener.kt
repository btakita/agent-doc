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
        val basePath = project.basePath ?: return

        // Collect all visible .md files across split panes.
        val manager = FileEditorManager.getInstance(project)
        val allSelected = manager.selectedFiles.toList()
        val visibleMdFiles = allSelected
            .filter { it.name.endsWith(".md") }
            .map { TerminalUtil.relativePath(project, it) }
            .distinct()

        log("selectionChanged: newFile=${file.name} allSelected=[${allSelected.joinToString { it.name }}] mdFiles=$visibleMdFiles")

        if (visibleMdFiles.isEmpty()) return

        val activeFile = TerminalUtil.relativePath(project, file)
        val editorLayout = LayoutDetector.detectEditorLayout(project)

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
                    val agentDoc = TerminalUtil.resolveAgentDoc(basePath)
                    val windowId = TerminalUtil.projectWindowId(project)
                    // Always pass --window: use resolved ID, or fall back to "agent-doc" name
                    val windowArgs = listOf("--window", windowId ?: "agent-doc")
                    // Always use sync --col (never focus) so that unwanted panes
                    // are broken out and the entire window layout is managed.
                    val mdColumns = editorLayout?.columns?.filter { it.files.isNotEmpty() }
                    val cmd = if (mdColumns != null && mdColumns.size > 1) {
                        // Full 2D layout — all columns have .md files
                        val colArgs = mdColumns.flatMap { col ->
                            listOf("--col", col.files.joinToString(","))
                        }
                        listOf(agentDoc, "sync") + colArgs + listOf("--focus", activeFile) + windowArgs
                    } else if (editorLayout != null && editorLayout.columns.size > 1 && (mdColumns?.size ?: 0) < editorLayout.columns.size) {
                        // Mixed split: some columns have non-.md files.
                        // Don't reorganize tmux layout — just focus the active file's pane.
                        listOf(agentDoc, "focus", activeFile)
                    } else {
                        // Single file or flat layout
                        val colArg = visibleMdFiles.joinToString(",")
                        listOf(agentDoc, "sync", "--col", colArg, "--focus", activeFile) + windowArgs
                    }
                    log("exec: ${cmd.joinToString(" ")}")
                    val summary = TerminalUtil.formatLayoutSummary(cmd)
                    TerminalUtil.showHint(project, summary)
                    val process = ProcessBuilder(cmd)
                        .directory(java.io.File(basePath))
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
