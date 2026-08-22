package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.vfs.VirtualFile
import java.io.File
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import javax.swing.SwingUtilities

/**
 * Classifies editor files from their live document text, so layout projection never mistakes an
 * ordinary Markdown plan/README for an agent-doc session document. Open editor files already have
 * a document; using its chars avoids a disk read and observes unsaved frontmatter changes.
 */
internal object AgentDocSessionFiles {
    fun isSessionDocument(file: VirtualFile): Boolean {
        if (!file.name.endsWith(".md")) return false
        val document = FileDocumentManager.getInstance().getDocument(file) ?: return false
        return isAgentDocDocumentTextUtil(document.charsSequence)
    }
}

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
        private val GSON = Gson()
        private const val PRESERVED_LAYOUT_MARKER =
            "[sync] sync preserved the current tmux layout because"
        private const val SAFE_PASSIVE_PRESERVED_LAYOUT_MARKER =
            "[sync] safe passive sync preserved the current tmux layout because"

        internal const val PRESERVED_LAYOUT_DEFERRED_WARNING =
            "Sync deferred: another visible agent-doc pane is mid-closeout, so the current tmux layout was preserved. Try again after that closeout finishes."
        internal const val SYNC_ALREADY_RUNNING_WARNING =
            "Sync deferred: another tmux layout sync is already running; this sync will retry shortly."
        internal const val SYNC_PROCESS_TIMEOUT_MS = 30_000L
        internal const val SYNC_DEFERRED_RETRY_MS = 500L
        internal const val SYNC_DEFERRED_MAX_RETRIES = 80

        private val PROTECTED_PANES_PATTERN =
            Regex("""visible protected pane\(s\) (.+?) cannot be detached safely""")

        internal data class SyncProcessResult(
            val exitCode: Int,
            val output: String,
            val timedOut: Boolean,
        )

        internal fun runCommandWithTimeout(
            cmd: List<String>,
            projectRoot: String,
            timeoutMs: Long = SYNC_PROCESS_TIMEOUT_MS,
        ): SyncProcessResult {
            val process = ProcessBuilder(cmd)
                .directory(File(projectRoot))
                .redirectErrorStream(true)
                .start()
            val outputFuture = CompletableFuture.supplyAsync {
                process.inputStream.bufferedReader().readText()
            }
            if (!process.waitFor(timeoutMs, TimeUnit.MILLISECONDS)) {
                process.destroy()
                if (!process.waitFor(500, TimeUnit.MILLISECONDS)) {
                    process.destroyForcibly()
                    process.waitFor(500, TimeUnit.MILLISECONDS)
                }
                val output = try {
                    outputFuture.get(1, TimeUnit.SECONDS)
                } catch (_: Exception) {
                    ""
                }
                return SyncProcessResult(
                    exitCode = 124,
                    output = output.trim(),
                    timedOut = true,
                )
            }
            val output = try {
                outputFuture.get(1, TimeUnit.SECONDS)
            } catch (_: Exception) {
                ""
            }
            return SyncProcessResult(
                exitCode = process.exitValue(),
                output = output.trim(),
                timedOut = false,
            )
        }

        internal fun isPreservedLayoutOutput(output: String): Boolean =
            output
                .lineSequence()
                .map { it.trim() }
                .any {
                    it.contains(PRESERVED_LAYOUT_MARKER) ||
                        it.contains(SAFE_PASSIVE_PRESERVED_LAYOUT_MARKER)
                }

        internal fun preservedLayoutDetails(output: String): String? {
            val markerLine = output
                .lineSequence()
                .map { it.trim() }
                .firstOrNull {
                    it.contains(PRESERVED_LAYOUT_MARKER) ||
                        it.contains(SAFE_PASSIVE_PRESERVED_LAYOUT_MARKER)
                }
                ?: return null
            val protectedPaneText = PROTECTED_PANES_PATTERN.find(markerLine)
                ?.groupValues
                ?.getOrNull(1)
                ?: return null
            val protectedPanes = protectedPaneText
                .split(",")
                .mapNotNull { raw ->
                    val parts = raw.trim().split(":", limit = 3)
                    if (parts.size != 3) return@mapNotNull null
                    val (pane, phase, file) = parts
                    "$pane $phase $file"
                }
            return protectedPanes.takeIf { it.isNotEmpty() }?.joinToString("; ")
        }

        internal fun preservedLayoutWarning(output: String): String? =
            if (isPreservedLayoutOutput(output)) {
                preservedLayoutDetails(output)?.let { details ->
                    "$PRESERVED_LAYOUT_DEFERRED_WARNING Blocked pane(s): $details"
                } ?: PRESERVED_LAYOUT_DEFERRED_WARNING
            } else {
                null
            }

        internal fun syncFailureMessage(output: String): String {
            val diagnostic = output.trim().ifEmpty {
                "project controller returned no diagnostic"
            }
            return "Sync failed: ${diagnostic.take(500)}"
        }

        internal fun collectVisibleMarkdownFiles(
            files: Array<out com.intellij.openapi.vfs.VirtualFile>,
            isSessionDocument: (VirtualFile) -> Boolean = { it.name.endsWith(".md") },
        ): List<String> = files
            .filter(isSessionDocument)
            .map { it.path }
            .distinct()

        internal fun chooseSyncProjectRoot(
            basePath: String?,
            fallbackRoot: String,
            visibleMarkdownFiles: List<String>,
        ): String {
            val visibleRoots = visibleMarkdownFiles
                .mapNotNull { TerminalUtil.nearestAgentDocProjectRoot(it) }
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
            exactVisible: Boolean = false,
        ): List<String> {
            val focusArgs = if (focusedFile != null) listOf("--focus", focusedFile) else emptyList()
            val noAutostartArgs = if (noAutostart) listOf("--no-autostart") else emptyList()
            val exactVisibleArgs = if (exactVisible) listOf("--exact-visible") else emptyList()
            return if (editorLayout != null && editorLayout.columns.size > 1) {
                val colArgs = editorLayout.columns
                    .flatMap { col ->
                        listOf("--col", col.files.joinToString(","))
                    }
                listOf(agentDoc, "sync") + colArgs + focusArgs + exactVisibleArgs + noAutostartArgs
            } else {
                val colArg = visibleMdFiles.joinToString(",")
                listOf(agentDoc, "sync", "--col", colArg) + focusArgs + exactVisibleArgs + noAutostartArgs
            }
        }

        internal fun buildSyncColumns(
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
        ): List<String> =
            if (editorLayout != null && editorLayout.columns.size > 1) {
                editorLayout.columns.map { column -> column.files.joinToString(",") }
            } else {
                listOf(visibleMdFiles.joinToString(","))
            }

        /**
         * `#recyclerestart` Q2 — decide whether a just-completed sync should re-run to
         * apply a layout that superseded it mid-flight. Re-run only when WE held the guard
         * (`heldGuard`) and a newer sync bumped the generation while we ran
         * (`!generationStillCurrent`). A deferred request (guard not held) never re-runs —
         * the in-flight holder owns the re-run — and a still-current generation means no
         * newer sync is pending, so the re-run chain converges and cannot loop forever.
         */
        internal fun shouldRerunAfterSupersede(
            heldGuard: Boolean,
            generationStillCurrent: Boolean,
        ): Boolean = heldGuard && !generationStillCurrent

        internal fun deferredSyncRetryDelayMs(attempt: Int): Long? =
            if (attempt < SYNC_DEFERRED_MAX_RETRIES) SYNC_DEFERRED_RETRY_MS else null

        internal fun syncCallerKind(
            noAutostart: Boolean,
            requestedCallerKind: String?,
        ): String = requestedCallerKind
            ?.takeIf { it.isNotBlank() }
            ?: if (noAutostart) "automatic" else "manual"

        /**
         * Syncs tmux layout to match the IDE editor split. Can be called from
         * any action (e.g. ClaimAction calls this after claiming).
         * Runs on a background thread — safe to call from EDT.
         */
        fun syncLayout(
            project: com.intellij.openapi.project.Project,
            notify: Boolean = true,
            noAutostart: Boolean = false,
            callerKind: String? = null,
        ) {
            if (!SwingUtilities.isEventDispatchThread()) {
                ApplicationManager.getApplication().invokeLater {
                    syncLayout(project, notify, noAutostart, callerKind)
                }
                return
            }
            val manager = FileEditorManager.getInstance(project)
            val focusedVFile = manager.selectedTextEditor?.virtualFile
                ?.takeIf(AgentDocSessionFiles::isSessionDocument)
                ?: manager.selectedFiles.firstOrNull(AgentDocSessionFiles::isSessionDocument)
                ?: return
            val focusedFile = focusedVFile.path
            val visibleMdFiles = collectVisibleMarkdownFiles(
                manager.selectedFiles,
                AgentDocSessionFiles::isSessionDocument,
            )
            if (visibleMdFiles.isEmpty()) {
                if (notify) TerminalUtil.showHint(project, "No .md files open")
                return
            }
            val basePath = project.basePath
            val (focusedProjectRoot, _) = TerminalUtil.resolveProject(project, focusedVFile)
            val projectRoot = chooseSyncProjectRoot(
                basePath,
                focusedProjectRoot,
                visibleMdFiles,
            )
            // IDEA component state is captured on the EDT exactly once. The
            // controller/socket work below owns the background portion.
            val editorLayout = absolutizeEditorLayout(
                projectRoot,
                normalizeEditorLayout(
                    basePath,
                    projectRoot,
                    LayoutDetector.detectEditorLayout(
                        project,
                        manager.openFiles
                            .filter(AgentDocSessionFiles::isSessionDocument)
                            .map { it.path }
                            .toSet(),
                    ),
                ),
            )

            Thread {
                try {
                    val columns = buildSyncColumns(
                        visibleMdFiles,
                        editorLayout,
                    )
                    val receipt = CpRouteClient.submitSyncTmuxLayout(
                        projectRoot = projectRoot,
                        columnsJson = GSON.toJson(columns),
                        window = null,
                        focus = focusedFile,
                        noAutostart = noAutostart,
                        exactVisible = true,
                        callerKind = syncCallerKind(noAutostart, callerKind),
                    )
                    if (receipt.exitCode != 0) {
                        LOG.warn("[sync] Project Controller async submit failed projectRoot=$projectRoot focus=$focusedFile columns=$columns output=${receipt.output}")
                        if (notify) {
                            TerminalUtil.notifyError(
                                project,
                                syncFailureMessage(receipt.output),
                            )
                        }
                    } else {
                        LOG.info("[sync] Project Controller async submit accepted: ${receipt.output.take(500)}")
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
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(LayoutDetector::class.java)
    private const val COLUMN_X_TOLERANCE_PX = 8

    internal data class LayoutWindowSnapshot(
        val x: Int,
        val y: Int,
        val file: String?,
    )

    /**
     * `#stickymdpane`: which agent-doc document an editor window stands for.
     *
     * A window's selected tab is the answer whenever it is one. When the
     * operator switches that window to a source file, the window has not gone
     * away — it is still on screen, still part of a two-column layout — so
     * reporting "no document here" collapsed the mirrored tmux layout to a
     * single pane and threw away the session pane the operator was working
     * next to. Getting it back required navigating to the document again.
     *
     * A window that has shown a document therefore keeps standing for the last
     * one it showed, taken from that window's own tabs in most-recently-used
     * order. A window that never held a document still contributes nothing,
     * so closing a split (or opening a fresh source-only split) still
     * collapses the layout as before.
     */
    internal fun stickyMarkdownForWindow(
        selectedPath: String?,
        windowMarkdownTabsMruLast: List<String>,
    ): String? =
        selectedPath?.takeIf(windowMarkdownTabsMruLast::contains)
            ?: windowMarkdownTabsMruLast.lastOrNull()

    /**
     * That window's `.md` tabs, ordered so the most recently used is last.
     *
     * `EditorHistoryManager` is the IDE's own selection history, so this needs
     * no plugin-side per-window bookkeeping to survive restarts or splits.
     * Tabs the history has never seen keep their tab order, ahead of any tab
     * it has, because "never selected" is older than "selected once".
     */
    internal fun markdownTabsMruLast(
        project: com.intellij.openapi.project.Project,
        windowTabPaths: List<String>,
    ): List<String> {
        val history = try {
            com.intellij.openapi.fileEditor.impl.EditorHistoryManager
                .getInstance(project)
                .fileList
                .map { it.path }
        } catch (e: Exception) {
            LOG.debug("[layout-detect] editor history unavailable: ${e.message}")
            emptyList()
        }
        return windowTabPaths
            .filter { it.endsWith(".md") }
            .sortedBy { history.indexOf(it) }
    }

    /**
     * Detect the editor layout as a list of columns, each containing stacked files.
     * Returns null if detection fails or there's only one editor window.
     */
    fun detectEditorLayout(
        project: com.intellij.openapi.project.Project,
        sessionDocumentPaths: Set<String>? = null,
    ): EditorLayout? {
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val windows = managerEx.windows
            if (windows.size < 2) {
                LOG.debug("[layout-detect] single editor window (count=${windows.size}); no split layout to mirror")
                return null
            }

            val splitters = managerEx.splitters
            val splittersComponent = splitters as? java.awt.Component
            if (splittersComponent == null) {
                LOG.debug("[layout-detect] ${windows.size} editor windows but splitters component unavailable; cannot resolve columns")
                return null
            }

            val classifiedSessionPaths = sessionDocumentPaths ?: windows
                .flatMap { it.fileList.toList() }
                .filter(AgentDocSessionFiles::isSessionDocument)
                .map { it.path }
                .toSet()
            val snapshots = windows.map { window ->
                // `#stickymdpane`: a window showing a source file still stands
                // for the last document it showed, so the mirrored column
                // survives a detour into source.
                val stickyPath = stickyMarkdownForWindow(
                    selectedPath = window.selectedFile?.path,
                    windowMarkdownTabsMruLast = markdownTabsMruLast(
                        project,
                        window.fileList
                            .map { it.path }
                            .filter(classifiedSessionPaths::contains),
                    ),
                )
                val file = stickyPath
                    ?.let { path -> window.fileList.firstOrNull { it.path == path } }
                    ?.let { TerminalUtil.relativePath(project, it) }
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
            LOG.debug(
                "[layout-detect] ${windows.size} editor window(s): " +
                    snapshots.joinToString(", ") { "x=${it.x} y=${it.y} file=${it.file ?: "<none>"}" }
            )
            if (snapshots.none { it.file != null }) {
                LOG.debug("[layout-detect] no .md file selected in any editor window; no layout to mirror")
                return null
            }

            val columns = buildColumnsFromSnapshots(snapshots)
            LOG.debug(
                "[layout-detect] grouped into ${columns.size} column(s): " +
                    columns.joinToString(" | ") { col ->
                        "[" + col.files.joinToString(", ").ifEmpty { "<empty>" } + "]"
                    }
            )

            // Return layout if at least 2 columns exist (even if some are empty).
            // Empty columns tell sync to leave that tmux pane position alone.
            return if (columns.size >= 2) {
                EditorLayout(columns)
            } else {
                LOG.debug("[layout-detect] fewer than 2 columns after grouping; treating as single-column layout")
                null
            }
        } catch (e: Exception) {
            LOG.warn("[layout-detect] editor layout detection failed: ${e.message}", e)
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
