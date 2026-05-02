package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import javax.swing.SwingUtilities

/**
 * Manually re-syncs the tmux pane layout to match the current IDE editor split.
 *
 * Triggered by Ctrl+Shift+Alt+L or via the Alt+Enter popup menu.
 * Runs immediately (no debounce) and clears the dedup cache so
 * automatic sync picks up subsequent changes.
 */
class SyncLayoutAction : AnAction() {

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(SyncLayoutAction::class.java)

        internal fun buildSyncCommand(
            agentDoc: String,
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
            focusedFile: String?,
            windowId: String?,
            noAutostart: Boolean,
        ): List<String> {
            val windowArgs = listOf("--window", windowId ?: "agent-doc")
            val focusArgs = if (focusedFile != null) listOf("--focus", focusedFile) else emptyList()
            val noAutostartArgs = if (noAutostart) listOf("--no-autostart") else emptyList()
            return if (editorLayout != null && editorLayout.columns.size > 1) {
                val colArgs = editorLayout.columns
                    .filter { it.files.isNotEmpty() }
                    .flatMap { col ->
                        listOf("--col", col.files.joinToString(","))
                    }
                listOf(agentDoc, "sync") + colArgs + focusArgs + windowArgs + noAutostartArgs
            } else {
                val colArg = visibleMdFiles.joinToString(",")
                listOf(agentDoc, "sync", "--col", colArg) + focusArgs + windowArgs + noAutostartArgs
            }
        }

        /**
         * Syncs tmux layout to match the IDE editor split. Can be called from
         * any action (e.g. ClaimAction calls this after claiming).
         * Runs on a background thread — safe to call from EDT.
         */
        fun syncLayout(
            project: com.intellij.openapi.project.Project,
            notify: Boolean = true,
            noAutostart: Boolean = false,
        ) {
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
            // Determine the focused editor file for --focus
            val focusedFile = manager.selectedTextEditor?.virtualFile?.let {
                if (it.name.endsWith(".md")) TerminalUtil.relativePath(project, it) else null
            }
            Thread {
                try {
                    val agentDoc = TerminalUtil.resolveAgentDoc(basePath)
                    val editorLayout = if (visibleMdFiles.size > 1)
                        LayoutDetector.detectEditorLayout(project) else null
                    val cmd = buildSyncCommand(
                        agentDoc,
                        visibleMdFiles,
                        editorLayout,
                        focusedFile,
                        windowId,
                        noAutostart,
                    )
                    if (notify) {
                        val summary = TerminalUtil.formatLayoutSummary(cmd)
                        TerminalUtil.showHint(project, summary)
                    }
                    val process = ProcessBuilder(cmd)
                        .directory(java.io.File(basePath))
                        .redirectErrorStream(true)
                        .start()
                    val output = process.inputStream.bufferedReader().readText().trim()
                    val exitCode = process.waitFor()
                    LOG.info("[sync] exit=$exitCode cmd=${cmd.joinToString(" ")}")
                    if (output.isNotEmpty()) {
                        // Log first 500 chars of output for debugging
                        LOG.info("[sync] output: ${output.take(500)}")
                    }
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
 * Detects the 2D columnar layout of .md files in the editor by grouping
 * visible editor windows by on-screen position.
 *
 * JetBrains does not guarantee `FileEditorManagerEx.windows` is returned in
 * left-to-right screen order; when the right split is focused it can surface
 * that window first. Grouping by actual component bounds keeps the tmux
 * layout stable regardless of focus.
 */
object LayoutDetector {
    private const val COLUMN_X_TOLERANCE_PX = 8

    internal data class LayoutWindowSnapshot(
        val x: Int,
        val y: Int,
        val file: String?,
    )

    /**
     * Detect the editor layout as a list of columns, each containing stacked files.
     * Returns null if detection fails or there's only one editor window.
     */
    fun detectEditorLayout(project: com.intellij.openapi.project.Project): EditorLayout? {
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val windows = managerEx.windows
            if (windows.size < 2) return null

            val splitters = managerEx.splitters
            val splittersComponent = splitters as? java.awt.Component ?: return null

            val snapshots = windows.map { window ->
                val file = window.selectedFile?.takeIf { it.name.endsWith(".md") }?.let {
                    TerminalUtil.relativePath(project, it)
                }
                val component = window.tabbedPane.component
                val bounds = component.parent?.let { parent ->
                    SwingUtilities.convertRectangle(parent, component.bounds, splittersComponent)
                } ?: component.bounds
                LayoutWindowSnapshot(
                    x = bounds.x,
                    y = bounds.y,
                    file = file,
                )
            }
            if (snapshots.none { it.file != null }) return null

            val columns = buildColumnsFromSnapshots(snapshots)

            // Return layout if at least 2 columns exist (even if some are empty).
            // Empty columns tell sync to leave that tmux pane position alone.
            return if (columns.size >= 2) EditorLayout(columns) else null
        } catch (_: Exception) {
            return null
        }
    }

    internal fun buildColumnsFromSnapshots(
        snapshots: List<LayoutWindowSnapshot>,
        columnTolerancePx: Int = COLUMN_X_TOLERANCE_PX,
    ): List<LayoutColumn> {
        if (snapshots.isEmpty()) return emptyList()

        val sorted = snapshots.sortedWith(compareBy<LayoutWindowSnapshot>({ it.x }, { it.y }))
        val grouped = mutableListOf<MutableList<LayoutWindowSnapshot>>()

        for (snapshot in sorted) {
            val existingColumn = grouped.lastOrNull()?.takeIf { column ->
                kotlin.math.abs(column.first().x - snapshot.x) <= columnTolerancePx
            }
            if (existingColumn != null) {
                existingColumn += snapshot
            } else {
                grouped += mutableListOf(snapshot)
            }
        }

        return grouped.map { column ->
            LayoutColumn(
                column.sortedBy { it.y }.mapNotNull { it.file }
            )
        }
    }
}
