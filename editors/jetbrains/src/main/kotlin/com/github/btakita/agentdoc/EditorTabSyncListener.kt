package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.ui.Splitter
import java.awt.Component
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Syncs tmux pane layout with editor tab switches.
 *
 * When the user switches editor tabs:
 * - Single visible .md file → `agent-doc focus <file>`
 * - Multiple visible .md files in splits → `agent-doc layout <files...> --split h|v`
 *
 * Guards against rapid-fire events:
 * - 500ms debounce so only the final state is acted upon
 * - Concurrency guard: skips if a layout command is already running
 * - Dedup: skips if the file set hasn't changed since the last execution
 *
 * Registered in plugin.xml as a projectListener on FileEditorManagerListener.
 */
class EditorTabSyncListener : FileEditorManagerListener {

    companion object {
        private const val DEBOUNCE_MS = 500L
        private val generation = AtomicLong(0)
        private val running = AtomicBoolean(false)
        @Volatile private var lastFileSet: List<String> = emptyList()
        @Volatile private var lastActiveFile: String = ""
        private val LOG_FILE = java.io.File("/tmp/agent-doc-plugin.log")

        /** Clear the dedup cache so the next automatic sync runs unconditionally. */
        fun clearLastFileSet() {
            lastFileSet = emptyList()
            lastActiveFile = ""
        }
    }

    private fun log(msg: String) {
        try {
            LOG_FILE.appendText("${java.time.Instant.now()} $msg\n")
        } catch (_: Exception) {}
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

        // Detect split orientation from the Swing component tree.
        val split = detectSplitOrientation(project)
        val activeFile = TerminalUtil.relativePath(project, file)

        // Debounce: bump generation, wait, then check if we're still current.
        val myGen = generation.incrementAndGet()

        Thread {
            try {
                Thread.sleep(DEBOUNCE_MS)
                if (generation.get() != myGen) {
                    log("debounce: superseded gen=$myGen current=${generation.get()}")
                    return@Thread
                }

                val sorted = visibleMdFiles.sorted()
                val fileSetChanged = sorted != lastFileSet
                val activeFileChanged = activeFile != lastActiveFile

                // Dedup: skip if neither file set nor active file changed.
                if (!fileSetChanged && !activeFileChanged) {
                    log("dedup: unchanged set=$sorted active=$activeFile")
                    return@Thread
                }

                // Concurrency guard: skip if another layout command is running.
                if (!running.compareAndSet(false, true)) {
                    log("guard: layout already running, skipping")
                    return@Thread
                }

                try {
                    lastFileSet = sorted
                    lastActiveFile = activeFile

                    val agentDoc = TerminalUtil.resolveAgentDoc()
                    val cmd = if (fileSetChanged) {
                        // File set changed → full layout
                        if (visibleMdFiles.size == 1) {
                            listOf(agentDoc, "focus", visibleMdFiles[0])
                        } else {
                            val splitFlag = if (split == "v") "v" else "h"
                            listOf(agentDoc, "layout") + visibleMdFiles + listOf("--split", splitFlag)
                        }
                    } else {
                        // Same file set, different active file → just focus
                        listOf(agentDoc, "focus", activeFile)
                    }
                    log("exec: ${cmd.joinToString(" ")}")
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

    /**
     * Walk the Swing component tree from EditorsSplitters to detect split orientation.
     * Returns "h" for side-by-side (horizontal), "v" for stacked (vertical).
     * Defaults to "h" if no split is detected.
     */
    private fun detectSplitOrientation(project: com.intellij.openapi.project.Project): String {
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val splitters = managerEx.splitters
            val root = (splitters as? java.awt.Container) ?: return "h"
            return findSplitterOrientation(root) ?: "h"
        } catch (_: Exception) {
            return "h"
        }
    }

    private fun findSplitterOrientation(component: Component): String? {
        if (component is Splitter) {
            // Splitter.isVertical == true means components stacked vertically → "v"
            // Splitter.isVertical == false means components side by side → "h"
            return if (component.isVertical) "v" else "h"
        }
        if (component is java.awt.Container) {
            for (child in component.components) {
                val result = findSplitterOrientation(child)
                if (result != null) return result
            }
        }
        return null
    }
}
