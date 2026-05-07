package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import java.io.File
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
        private const val PRESERVED_LAYOUT_MARKER =
            "[sync] sync preserved the current tmux layout because"
        private const val SAFE_PASSIVE_PRESERVED_LAYOUT_MARKER =
            "[sync] safe passive sync preserved the current tmux layout because"

        internal fun preservedLayoutWarning(output: String): String? =
            output
                .lineSequence()
                .map { it.trim() }
                .firstOrNull {
                    it.contains(PRESERVED_LAYOUT_MARKER) ||
                        it.contains(SAFE_PASSIVE_PRESERVED_LAYOUT_MARKER)
                }

        internal fun collectVisibleMarkdownFiles(
            files: Array<out com.intellij.openapi.vfs.VirtualFile>,
        ): List<String> = files
            .filter { it.name.endsWith(".md") }
            .map { it.path }
            .distinct()

        internal fun chooseSyncProjectRoot(
            basePath: String?,
            fallbackRoot: String,
            visibleMarkdownFiles: List<String>,
        ): String {
            val visibleRoots = visibleMarkdownFiles
                .mapNotNull { NativePatching.resolveProjectPath(it)?.first }
                .distinct()
            if (visibleRoots.size <= 1) {
                if (
                    visibleRoots.isEmpty() &&
                    basePath != null &&
                    visibleMarkdownFiles.any { file ->
                        file != fallbackRoot && !file.startsWith("$fallbackRoot/")
                    }
                ) {
                    return basePath
                }
                return visibleRoots.firstOrNull() ?: fallbackRoot
            }

            if (basePath != null) {
                return basePath
            }

            return fallbackRoot
        }

        internal fun normalizeEditorLayout(
            basePath: String?,
            projectRoot: String,
            editorLayout: EditorLayout?,
        ): EditorLayout? {
            val layout = editorLayout ?: return null
            if (basePath == null || basePath == projectRoot) {
                return layout
            }

            val rootPrefix = projectRoot.removePrefix("$basePath/").trim('/')
            if (rootPrefix.isEmpty()) {
                return layout
            }

            val normalizedColumns = layout.columns.map { column ->
                val files = column.files.mapNotNull { file ->
                    when {
                        file.isBlank() -> null
                        File(file).isAbsolute -> file
                        file.startsWith("$rootPrefix/") -> file.removePrefix("$rootPrefix/")
                        else -> File(basePath, file).path
                    }
                }
                LayoutColumn(files)
            }
            return if (normalizedColumns.any { it.files.isNotEmpty() }) {
                EditorLayout(normalizedColumns)
            } else {
                null
            }
        }

        internal fun absolutizeEditorLayout(
            projectRoot: String,
            editorLayout: EditorLayout?,
        ): EditorLayout? {
            val layout = editorLayout ?: return null
            val absoluteColumns = layout.columns.map { column ->
                val files = column.files.mapNotNull { file ->
                    when {
                        file.isBlank() -> null
                        File(file).isAbsolute -> file
                        else -> File(projectRoot, file).path
                    }
                }
                LayoutColumn(files)
            }
            return if (absoluteColumns.any { it.files.isNotEmpty() }) {
                EditorLayout(absoluteColumns)
            } else {
                null
            }
        }

        internal fun buildSyncCommand(
            agentDoc: String,
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
            focusedFile: String?,
            noAutostart: Boolean,
        ): List<String> {
            val focusArgs = if (focusedFile != null) listOf("--focus", focusedFile) else emptyList()
            val noAutostartArgs = if (noAutostart) listOf("--no-autostart") else emptyList()
            return if (editorLayout != null && editorLayout.columns.size > 1) {
                val colArgs = editorLayout.columns
                    .flatMap { col ->
                        listOf("--col", col.files.joinToString(","))
                    }
                listOf(agentDoc, "sync") + colArgs + focusArgs + noAutostartArgs
            } else {
                val colArg = visibleMdFiles.joinToString(",")
                listOf(agentDoc, "sync", "--col", colArg) + focusArgs + noAutostartArgs
            }
        }

        internal fun buildFocusCommand(
            agentDoc: String,
            focusedFile: String,
        ): List<String> = listOf(agentDoc, "focus", focusedFile)

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
            val manager = FileEditorManager.getInstance(project)
            val focusedVFile = manager.selectedTextEditor?.virtualFile
                ?.takeIf { it.name.endsWith(".md") }
                ?: manager.selectedFiles.firstOrNull { it.name.endsWith(".md") }
                ?: return
            val focusedFile = focusedVFile.path
            val visibleMdFiles = collectVisibleMarkdownFiles(manager.selectedFiles)
            val (focusedProjectRoot, _) = TerminalUtil.resolveProject(project, focusedVFile)
            val projectRoot = chooseSyncProjectRoot(
                project.basePath,
                focusedProjectRoot,
                visibleMdFiles,
            )

            if (visibleMdFiles.isEmpty()) {
                if (notify) TerminalUtil.showHint(project, "No .md files open")
                return
            }

            Thread {
                try {
                    val agentDoc = TerminalUtil.resolveAgentDoc(projectRoot)
                    val editorLayout =
                        absolutizeEditorLayout(
                            projectRoot,
                            normalizeEditorLayout(
                                project.basePath,
                                projectRoot,
                                LayoutDetector.detectEditorLayout(project),
                            ),
                        )
                    val cmd = buildSyncCommand(
                        agentDoc,
                        visibleMdFiles,
                        editorLayout,
                        focusedFile,
                        noAutostart,
                    )
                    if (notify) {
                        TerminalUtil.showHint(project, TerminalUtil.formatLayoutSummary(cmd))
                    }
                    val process = ProcessBuilder(cmd)
                        .directory(java.io.File(projectRoot))
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
                    } else if (notify) {
                        val warning = preservedLayoutWarning(output)
                        if (warning != null) {
                            TerminalUtil.notifyWarning(project, warning)
                        }
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
