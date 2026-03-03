package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.ui.Splitter
import java.awt.Component

/**
 * Manually re-syncs the tmux pane layout to match the current IDE editor split.
 *
 * Triggered by Ctrl+Shift+Alt+L or via the Alt+Enter popup menu.
 * Runs immediately (no debounce) and clears the dedup cache so
 * automatic sync picks up subsequent changes.
 */
class SyncLayoutAction : AnAction() {

    companion object {
        /**
         * Syncs tmux layout to match the IDE editor split. Can be called from
         * any action (e.g. ClaimAction calls this after claiming).
         * Runs on a background thread — safe to call from EDT.
         */
        fun syncLayout(project: com.intellij.openapi.project.Project, notify: Boolean = true) {
            val basePath = project.basePath ?: return

            val manager = FileEditorManager.getInstance(project)
            val visibleMdFiles = manager.selectedFiles
                .filter { it.name.endsWith(".md") }
                .map { TerminalUtil.relativePath(project, it) }
                .distinct()

            if (visibleMdFiles.isEmpty()) {
                if (notify) TerminalUtil.showHint(project, "No .md files open")
                return
            }

            val windowId = TerminalUtil.projectWindowId(project)
            EditorTabSyncListener.clearLastFileSet()

            Thread {
                try {
                    val agentDoc = TerminalUtil.resolveAgentDoc()
                    val windowArgs = if (windowId != null) listOf("--window", windowId) else emptyList()
                    // Always use sync --col (never focus) so that unwanted panes
                    // are broken out and the entire window layout is managed.
                    val editorLayout = if (visibleMdFiles.size > 1)
                        LayoutDetector.detectEditorLayout(project) else null
                    val cmd = if (editorLayout != null && editorLayout.columns.size > 1) {
                        // 2D layout: use sync --col format
                        val colArgs = editorLayout.columns.flatMap { col ->
                            listOf("--col", col.files.joinToString(","))
                        }
                        listOf(agentDoc, "sync") + colArgs + windowArgs
                    } else {
                        // Single file or flat layout: sync with single column
                        val colArg = visibleMdFiles.joinToString(",")
                        listOf(agentDoc, "sync", "--col", colArg) + windowArgs
                    }
                    if (notify) {
                        val summary = TerminalUtil.formatLayoutSummary(cmd)
                        TerminalUtil.notifyInfo(project, summary)
                    }
                    val process = ProcessBuilder(cmd)
                        .directory(java.io.File(basePath))
                        .redirectErrorStream(true)
                        .start()
                    val output = process.inputStream.bufferedReader().readText().trim()
                    val exitCode = process.waitFor()
                    if (notify && exitCode != 0) {
                        TerminalUtil.notifyError(project, "Sync failed (exit $exitCode):\n$output")
                    }
                } catch (ex: Exception) {
                    if (notify) TerminalUtil.notifyError(project, "Failed to sync layout: ${ex.message}")
                }
            }.start()
        }
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        syncLayout(project)
    }

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            file != null && file.extension?.lowercase() == "md"
    }

    override fun getActionUpdateThread(): ActionUpdateThread {
        return ActionUpdateThread.BGT
    }
}

/**
 * Represents a detected 2D editor layout.
 */
data class LayoutColumn(val files: List<String>)
data class EditorLayout(val columns: List<LayoutColumn>)

/**
 * Detects the 2D columnar layout of .md files in the editor by walking
 * the Swing Splitter tree recursively.
 *
 * - Horizontal Splitter (isVertical=false) → separate columns (left + right)
 * - Vertical Splitter (isVertical=true) → merge files into same column (stacked)
 * - Leaf → maps to an EditorWindow by order (splitter tree leaves correspond 1:1
 *   with EditorWindow instances in left-to-right, top-to-bottom order)
 */
object LayoutDetector {

    /**
     * Detect the editor layout as a list of columns, each containing stacked files.
     * Returns null if detection fails or there's only one editor window.
     */
    fun detectEditorLayout(project: com.intellij.openapi.project.Project): EditorLayout? {
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val windows = managerEx.windows
            if (windows.size < 2) return null

            // Collect selected .md files from each window, in window order.
            // Each window represents one leaf in the splitter tree.
            val windowFiles = windows.mapNotNull { window ->
                val file = window.selectedFile
                if (file != null && file.name.endsWith(".md")) {
                    TerminalUtil.relativePath(project, file)
                } else null
            }
            if (windowFiles.size < 2) return null

            // Walk the splitter tree to get the column structure as leaf indices.
            val splitters = managerEx.splitters
            val root = (splitters as? java.awt.Container) ?: return null
            val leafCounter = intArrayOf(0) // mutable counter for leaf assignment
            val indexColumns = walkSplitterTree(root, leafCounter)
            if (indexColumns.isEmpty()) return null

            // Map leaf indices to file paths
            val columns = indexColumns.mapNotNull { indices ->
                val files = indices.mapNotNull { idx ->
                    if (idx < windowFiles.size) windowFiles[idx] else null
                }
                if (files.isNotEmpty()) LayoutColumn(files) else null
            }

            return if (columns.size >= 2) EditorLayout(columns) else null
        } catch (_: Exception) {
            return null
        }
    }

    /**
     * Recursively walk the Swing component tree and return column structure
     * as lists of leaf indices.
     *
     * A horizontal splitter (isVertical=false) means left/right → separate columns.
     * A vertical splitter (isVertical=true) means top/bottom → same column, merged indices.
     * A leaf → assigns the next leaf index from the counter.
     */
    private fun walkSplitterTree(
        component: Component,
        leafCounter: IntArray
    ): List<List<Int>> {
        if (component is Splitter) {
            val first = component.firstComponent
            val second = component.secondComponent
            val leftCols = if (first != null) walkSplitterTree(first, leafCounter) else emptyList()
            val rightCols = if (second != null) walkSplitterTree(second, leafCounter) else emptyList()

            return if (!component.isVertical) {
                // Horizontal split → separate columns
                leftCols + rightCols
            } else {
                // Vertical split → merge all indices into one column
                val merged = (leftCols + rightCols).flatten()
                if (merged.isEmpty()) emptyList() else listOf(merged)
            }
        }

        // Delegate only to children that contain Splitters.
        // Non-splitter children (decorations, overlays, status bars) are skipped
        // to avoid assigning bogus leaf indices that offset the real editor panes.
        if (component is java.awt.Container) {
            val splitterChildren = component.components.filter { containsSplitter(it) }
            if (splitterChildren.size == 1) {
                // Wrapper around a single splitter subtree — delegate directly
                return walkSplitterTree(splitterChildren[0], leafCounter)
            } else if (splitterChildren.size > 1) {
                // Multiple splitter subtrees — process all
                return splitterChildren.flatMap { walkSplitterTree(it, leafCounter) }
            }
        }

        // Leaf: assign current index and increment
        val idx = leafCounter[0]
        leafCounter[0] = idx + 1
        return listOf(listOf(idx))
    }

    private fun containsSplitter(component: Component): Boolean {
        if (component is Splitter) return true
        if (component is java.awt.Container) {
            return component.components.any { containsSplitter(it) }
        }
        return false
    }
}
