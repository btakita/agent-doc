package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import java.io.File
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
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
            deferredAttempt: Int = 0,
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
                var syncGuard: AgentDocLib? = null
                val lib = AgentDocLib.get()
                // `#recyclerestart` Q2 (supersede, not warn-and-skip): bump the sync
                // generation up front. If another sync currently holds the guard, this
                // newer request is deferred — but the in-flight holder re-runs with THIS
                // latest layout on completion (see the finally block) instead of dropping
                // it, so a rapid second `Sync Tmux Layout` is never silently lost.
                val myGen = lib?.agent_doc_sync_bump_generation() ?: 0L
                try {
                    if (lib != null) {
                        if (!lib.agent_doc_sync_try_lock()) {
                            val retryDelayMs = deferredSyncRetryDelayMs(deferredAttempt)
                            LOG.info(
                                "[sync] deferred sync (gen=$myGen attempt=$deferredAttempt): another sync is in flight; retryDelayMs=$retryDelayMs"
                            )
                            if (notify && deferredAttempt == 0) {
                                TerminalUtil.notifyWarning(project, SYNC_ALREADY_RUNNING_WARNING)
                            }
                            if (retryDelayMs != null) {
                                Thread {
                                    Thread.sleep(retryDelayMs)
                                    syncLayout(
                                        project,
                                        notify = false,
                                        noAutostart = noAutostart,
                                        deferredAttempt = deferredAttempt + 1,
                                    )
                                }.apply {
                                    isDaemon = true
                                    start()
                                }
                            } else if (notify) {
                                TerminalUtil.notifyWarning(
                                    project,
                                    "Sync deferred: another tmux layout sync is still running after ${
                                        SYNC_DEFERRED_MAX_RETRIES * SYNC_DEFERRED_RETRY_MS / 1000
                                    }s.",
                                )
                            }
                            return@Thread
                        }
                        syncGuard = lib
                    }
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
                    val result = runCommandWithTimeout(cmd, projectRoot)
                    val output = result.output
                    val exitCode = result.exitCode
                    LOG.info("[sync] exit=$exitCode cmd=${cmd.joinToString(" ")}")
                    if (output.isNotEmpty()) {
                        // Log first 500 chars of output for debugging
                        LOG.info("[sync] output: ${output.take(500)}")
                    }
                    if (notify && exitCode != 0) {
                        val reason = if (result.timedOut) {
                            "Sync timed out after ${SYNC_PROCESS_TIMEOUT_MS / 1000}s; the local sync guard was released so retry can run binary recovery."
                        } else {
                            "Sync failed (exit $exitCode):"
                        }
                        TerminalUtil.notifyError(project, "$reason\n$output")
                    } else if (notify) {
                        val warning = preservedLayoutWarning(output)
                        if (warning != null) {
                            TerminalUtil.notifyWarning(project, warning)
                        }
                    }
                } catch (ex: Exception) {
                    if (notify) TerminalUtil.notifyError(project, "Failed to sync layout: ${ex.message}")
                } finally {
                    val heldGuard = syncGuard != null
                    syncGuard?.agent_doc_sync_unlock()
                    // `#recyclerestart` Q2: if a newer sync arrived while we held the guard
                    // (it bumped the generation but was deferred), re-run ONCE so the LATEST
                    // editor layout is applied instead of dropped. Only the guard holder
                    // re-runs (a deferred request never does), and the generation converges
                    // when no newer sync is pending, so this cannot loop indefinitely.
                    if (lib != null &&
                        shouldRerunAfterSupersede(heldGuard, lib.agent_doc_sync_check_generation(myGen))
                    ) {
                        // `#tmuxsynccrash`: the supersede re-run only needs to apply the
                        // newer editor layout's focus/placement — the destructive doctor
                        // repair already ran in the superseded pass. Force the re-run
                        // passive (`--no-autostart`, no doctor repair) so a rapid manual
                        // sync cannot loop the destructive window-op sequence that crashes
                        // the tmux server. The binary also rate-limits this defensively.
                        LOG.info("[sync] re-running after supersede (passive, a newer sync arrived during this one)")
                        syncLayout(project, notify = false, noAutostart = true)
                    }
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
     * Detect the editor layout as a list of columns, each containing stacked files.
     * Returns null if detection fails or there's only one editor window.
     */
    fun detectEditorLayout(project: com.intellij.openapi.project.Project): EditorLayout? {
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val windows = managerEx.windows
            if (windows.size < 2) {
                LOG.info("[layout-detect] single editor window (count=${windows.size}); no split layout to mirror")
                return null
            }

            val splitters = managerEx.splitters
            val splittersComponent = splitters as? java.awt.Component
            if (splittersComponent == null) {
                LOG.info("[layout-detect] ${windows.size} editor windows but splitters component unavailable; cannot resolve columns")
                return null
            }

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
            LOG.info(
                "[layout-detect] ${windows.size} editor window(s): " +
                    snapshots.joinToString(", ") { "x=${it.x} y=${it.y} file=${it.file ?: "<none>"}" }
            )
            if (snapshots.none { it.file != null }) {
                LOG.info("[layout-detect] no .md file selected in any editor window; no layout to mirror")
                return null
            }

            val columns = buildColumnsFromSnapshots(snapshots)
            LOG.info(
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
                LOG.info("[layout-detect] fewer than 2 columns after grouping; treating as single-column layout")
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
