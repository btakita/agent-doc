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
 * When the user switches editor tabs:
 * - Single visible .md file → `agent-doc focus <file>`
 * - Multiple visible .md files in splits → `agent-doc sync --col <files...>`
 *
 * Guards against rapid-fire events:
 * - 500ms debounce so only the final state is acted upon
 * - Concurrency guard: skips if a layout command is already running
 * - Dedup: skips if the column structure hasn't changed since the last execution
 *
 * Registered in plugin.xml as a projectListener on FileEditorManagerListener.
 */
class EditorTabSyncListener : FileEditorManagerListener {

    companion object {
        private const val DEBOUNCE_MS = 500L
        private val generation = AtomicLong(0)
        private val running = AtomicBoolean(false)
        @Volatile private var lastColumnStructure: List<List<String>> = emptyList()
        @Volatile private var lastActiveFile: String = ""
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)

        /** Clear the dedup cache so the next automatic sync runs unconditionally. */
        fun clearLastFileSet() {
            lastColumnStructure = emptyList()
            lastActiveFile = ""
        }
    }

    private fun log(msg: String) {
        LOG.debug(msg)
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

        // Detect 2D layout structure for dedup comparison
        val editorLayout = LayoutDetector.detectEditorLayout(project)
        val currentColumns: List<List<String>> = if (editorLayout != null && editorLayout.columns.size > 1) {
            editorLayout.columns.map { it.files.sorted() }
        } else {
            listOf(visibleMdFiles.sorted())
        }

        // Debounce: bump generation, wait, then check if we're still current.
        val myGen = generation.incrementAndGet()

        Thread {
            try {
                Thread.sleep(DEBOUNCE_MS)
                if (generation.get() != myGen) {
                    log("debounce: superseded gen=$myGen current=${generation.get()}")
                    return@Thread
                }

                val columnStructureChanged = currentColumns != lastColumnStructure
                val activeFileChanged = activeFile != lastActiveFile

                // Dedup: skip if neither column structure nor active file changed.
                if (!columnStructureChanged && !activeFileChanged) {
                    log("dedup: unchanged columns=$currentColumns active=$activeFile")
                    return@Thread
                }

                // Concurrency guard: skip if another layout command is running.
                if (!running.compareAndSet(false, true)) {
                    log("guard: layout already running, skipping")
                    return@Thread
                }

                try {
                    lastColumnStructure = currentColumns
                    lastActiveFile = activeFile

                    val agentDoc = TerminalUtil.resolveAgentDoc()
                    val windowId = TerminalUtil.projectWindowId(project)
                    val windowArgs = if (windowId != null) listOf("--window", windowId) else emptyList()
                    // Always use sync --col (never focus) so that unwanted panes
                    // are broken out and the entire window layout is managed.
                    val cmd = if (editorLayout != null && editorLayout.columns.size > 1) {
                        // 2D layout
                        val colArgs = editorLayout.columns.flatMap { col ->
                            listOf("--col", col.files.joinToString(","))
                        }
                        listOf(agentDoc, "sync") + colArgs + listOf("--focus", activeFile) + windowArgs
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
                    running.set(false)
                }
            } catch (e: Exception) {
                running.set(false)
                log("error: ${e.message}")
            }
        }.start()
    }
}
