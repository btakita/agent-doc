package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.client.ClientKind
import com.intellij.openapi.fileEditor.ClientFileEditorManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.DumbAware
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
        return ReadAction.compute<Boolean, RuntimeException> {
            val document = FileDocumentManager.getInstance().getDocument(file)
                ?: return@compute false
            isAgentDocDocumentTextUtil(document.charsSequence)
        }
    }
}

/**
 * Manually re-syncs the tmux pane layout to match the current IDE editor split.
 *
 * Triggered by Ctrl+Shift+Alt+L or via the Alt+Enter popup menu.
 * Runs immediately (no debounce) and clears the dedup cache so
 * automatic sync picks up subsequent changes.
 */
class SyncLayoutAction : AnAction(), DumbAware {

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
            val errorOutput: String,
            val timedOut: Boolean,
        )

        internal fun runCommandWithTimeout(
            cmd: List<String>,
            projectRoot: String,
            timeoutMs: Long = SYNC_PROCESS_TIMEOUT_MS,
            captureStderrSeparately: Boolean = false,
        ): SyncProcessResult {
            val process = ProcessBuilder(cmd)
                .directory(File(projectRoot))
                .redirectErrorStream(!captureStderrSeparately)
                .start()
            val outputFuture = CompletableFuture.supplyAsync {
                process.inputStream.bufferedReader().readText()
            }
            val errorOutputFuture = if (captureStderrSeparately) {
                CompletableFuture.supplyAsync {
                    process.errorStream.bufferedReader().readText()
                }
            } else {
                CompletableFuture.completedFuture("")
            }
            fun collectOutput(future: CompletableFuture<String>): String = try {
                future.get(1, TimeUnit.SECONDS).trim()
            } catch (_: Exception) {
                ""
            }
            if (!process.waitFor(timeoutMs, TimeUnit.MILLISECONDS)) {
                process.destroy()
                if (!process.waitFor(500, TimeUnit.MILLISECONDS)) {
                    process.destroyForcibly()
                    process.waitFor(500, TimeUnit.MILLISECONDS)
                }
                return SyncProcessResult(
                    exitCode = 124,
                    output = collectOutput(outputFuture),
                    errorOutput = collectOutput(errorOutputFuture),
                    timedOut = true,
                )
            }
            return SyncProcessResult(
                exitCode = process.exitValue(),
                output = collectOutput(outputFuture),
                errorOutput = collectOutput(errorOutputFuture),
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
                val colArgs = undetectedLayoutColumns(visibleMdFiles)
                    .ifEmpty { listOf("") }
                    .flatMap { column -> listOf("--col", column) }
                listOf(agentDoc, "sync") + colArgs + focusArgs + exactVisibleArgs + noAutostartArgs
            }
        }

        /**
         * Columns for a surface whose split layout was not detected (GH #81).
         *
         * Every visible document is the selected tab of its own editor window, so two of them are
         * two splits, never two tabs of one window. Joining them as `--col a,b` told the
         * controller "one column whose tabs are a and b", and it keeps the FIRST agent document
         * per column -- IntelliJ lists the focused window's selection first, so tmux kept one
         * pane whose occupant swapped on every document switch while the sibling's pane stayed
         * in the stash window. Each visible document therefore gets its own column, in the order
         * observed.
         */
        internal fun undetectedLayoutColumns(visibleMdFiles: List<String>): List<String> =
            visibleMdFiles.filter(String::isNotBlank).distinct()

        /**
         * GH #112: the order source of [buildSyncColumns] / route `--col` lists.
         * Only a detected multi-column layout carries a left-to-right split order.
         * The undetected fallback lists `selectedFiles`, which IntelliJ orders
         * focused-window first, so its order is `unknown` and the controller keeps
         * the order it already retains for those documents. On a Remote Dev
         * backend the detector is `unknown` whenever no single client reports a
         * multi-file split set (GH #97), so pane order there is stable, not mirrored.
         */
        internal fun syncColumnOrder(editorLayout: EditorLayout?): String =
            if (editorLayout != null && editorLayout.columns.size > 1) "editor" else "unknown"

        internal fun buildSyncColumns(
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
        ): List<String> =
            if (editorLayout != null && editorLayout.columns.size > 1) {
                editorLayout.columns.map { column -> column.files.joinToString(",") }
            } else {
                undetectedLayoutColumns(visibleMdFiles).ifEmpty { listOf("") }
            }

        /**
         * GH #154: exact-visible is structural authority, so an unreadable editor layout cannot
         * be replaced by the focused-file fallback. That fallback is useful only for legacy
         * command shapes without an exact-visible authority claim.
         */
        internal fun exactVisibleSyncDecision(
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
        ): ExactVisibleSyncDecision =
            editorLayout
                ?.let { ExactVisibleSyncDecision.Publish(buildSyncColumns(visibleMdFiles, it)) }
                ?: ExactVisibleSyncDecision.RefuseUnknownLayout

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
        terminalPrepared: Boolean = false,
    ) {
            if (!SwingUtilities.isEventDispatchThread()) {
                ApplicationManager.getApplication().invokeLater {
                syncLayout(project, notify, noAutostart, callerKind, terminalPrepared)
                }
                return
            }
            val manager = FileEditorManager.getInstance(project)
            // `selectionChanged` can precede selectedTextEditor advancing. The current editor
            // window is the foreground authority at action time; the remaining candidates keep
            // split-editor and restored-window fallback behavior intact.
            val focusedVFile =
                sequenceOf(
                    FileEditorManagerEx.getInstanceEx(project).currentWindow?.selectedFile,
                    manager.selectedTextEditor?.virtualFile,
                ).plus(manager.selectedFiles.asSequence())
                    .filterNotNull()
                    .distinctBy { it.path }
                    .firstOrNull(AgentDocSessionFiles::isSessionDocument)
                    ?: run {
                        LOG.info("[sync] No focused agent-doc session document")
                        if (notify) TerminalUtil.showHint(project, "No agent-doc session document focused")
                        return
                    }
            val focusedFile = focusedVFile.path
            val visibleMdFiles = collectVisibleMarkdownFiles(
                (listOf(focusedVFile) + manager.selectedFiles).toTypedArray(),
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
        val exactVisibleColumns =
            when (val decision = exactVisibleSyncDecision(visibleMdFiles, editorLayout)) {
                is ExactVisibleSyncDecision.Publish -> decision.columns
                ExactVisibleSyncDecision.RefuseUnknownLayout -> {
                    val message =
                        "Editor split could not be read; the retained tmux layout was not changed."
                    LOG.warn("[sync] refusing exact-visible layout because editor columns are unknown")
                    if (notify) TerminalUtil.notifyError(project, message)
                    return
                }
            }

        if (!terminalPrepared && !noAutostart) {
            val relativeFocusedFile = java.io.File(projectRoot).toPath()
                .relativize(java.io.File(focusedFile).toPath())
                .toString()
            IdeTerminalCoordinator.ensureAndAttach(
                project = project,
                cwd = projectRoot,
                relativePath = relativeFocusedFile,
                onReady = {
                    syncLayout(
                        project = project,
                        notify = notify,
                        noAutostart = noAutostart,
                        callerKind = callerKind,
                        terminalPrepared = true,
                    )
                },
                onFailure = { message ->
                    if (notify) {
                        TerminalUtil.notifyError(
                            project,
                            "Failed to prepare agent-doc tmux session: $message",
                        )
                    }
                },
            )
            return
        }

        Thread {
                try {
                    if (!NativeReloadCoordinator.awaitReady()) {
                        if (notify) {
                            TerminalUtil.notifyError(
                                project,
                                "Sync deferred because the native-generation handoff did not " +
                                    "finish within ${NativeReloadCoordinator.USER_ACTION_AWAIT_MS / 1_000} seconds.",
                            )
                        }
                        return@Thread
                    }
                    val receipt = CpRouteClient.submitSyncTmuxLayout(
                        projectRoot = projectRoot,
                        columnsJson = GSON.toJson(exactVisibleColumns),
                        window = null,
                        focus = focusedFile,
                        noAutostart = noAutostart,
                        exactVisible = true,
                        callerKind = syncCallerKind(noAutostart, callerKind),
                        columnOrder = syncColumnOrder(editorLayout),
                    )
                    if (receipt.exitCode != 0) {
                        LOG.warn("[sync] Project Controller async submit failed projectRoot=$projectRoot focus=$focusedFile columns=$exactVisibleColumns output=${receipt.output}")
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
        // The controller submission already owns manual autostart. Submit immediately instead of
        // making the user action depend on a terminal-tool-window attachment callback.
        syncLayout(project, terminalPrepared = true)
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

internal sealed interface ExactVisibleSyncDecision {
    data class Publish(val columns: List<String>) : ExactVisibleSyncDecision
    data object RefuseUnknownLayout : ExactVisibleSyncDecision
}

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
     * Returns null only when detection fails. A proven single local editor window
     * returns one column so consumers can distinguish it from an unknown layout.
     */
    fun detectEditorLayout(
        project: com.intellij.openapi.project.Project,
        sessionDocumentPaths: Set<String>? = null,
    ): EditorLayout? {
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val windows = managerEx.windows
            val remoteClients = remoteClientSessionEditors(project, sessionDocumentPaths)
            if (shouldUseRemoteClientLayout(windows.size, remoteClients.size)) {
                // GH #97: backend-local FileEditorManager.selectedFiles is only the
                // focused file in Remote Dev. JetBrains keeps the real per-frontend
                // selections in client-scoped managers; this is the same service set
                // FileEditorManagerImpl uses for its own "with remotes" operations.
                // GH #134: those selections are focused-only too, so the visible
                // split set comes from the Remote Dev editor tracker, and the shared
                // native fold retains a known split across single-selection
                // observations instead of answering `unknown` forever.
                return detectEditorLayout(snapshotRemoteLayout(project, sessionDocumentPaths))
            }
            if (windows.size < 2) {
                val selectedFile = windows.singleOrNull()?.selectedFile
                val selectedSessionFile = selectedFile?.takeIf { file ->
                    val path = file.path
                    sessionDocumentPaths?.contains(path)
                        ?: AgentDocSessionFiles.isSessionDocument(file)
                }
                val selectedSessionPath = selectedSessionFile?.let { TerminalUtil.relativePath(project, it) }
                val layout = knownSingleWindowLayout(selectedSessionPath)
                val columns = layout?.columns.orEmpty()
                LOG.debug("[layout-detect] single editor window (count=${windows.size}); columns=${columns.size}")
                logObservedLayout(
                    windows.size,
                    windows.map { LayoutWindowSnapshot(0, 0, selectedSessionPath) },
                    columns,
                )
                return layout
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
                // GH #81: a window whose bounds cannot be read (a headless or remote
                // backend component) means geometry is unavailable for the whole layout,
                // never that the layout does not exist.
                val bounds = try {
                    val component = window.tabbedPane.component
                    component.parent?.let { parent ->
                        SwingUtilities.convertRectangle(parent, component.bounds, splittersComponent)
                    } ?: component.bounds
                } catch (e: Exception) {
                    LOG.debug("[layout-detect] editor window bounds unavailable: ${e.message}")
                    null
                }
                Triple(bounds?.x, bounds?.y, file)
            }.let { measured ->
                // Any unmeasured window collapses every window onto one origin, which
                // `buildColumnsFromSnapshots` reads as "keep each window its own column".
                val geometryKnown = measured.all { it.first != null && it.second != null }
                measured.map { (x, y, file) ->
                    LayoutWindowSnapshot(
                        x = if (geometryKnown) x!! else 0,
                        y = if (geometryKnown) y!! else 0,
                        file = file,
                    )
                }
            }
            LOG.debug(
                "[layout-detect] ${windows.size} editor window(s): " +
                    snapshots.joinToString(", ") { "x=${it.x} y=${it.y} file=${it.file ?: "<none>"}" }
            )
            if (snapshots.none { it.file != null }) {
                LOG.debug("[layout-detect] no .md file selected in any editor window; no layout to mirror")
                return null
            }
            if (sharesOrigin(snapshots)) {
                // GH #77: surfaced at info so a live run shows whether this IDE lays its
                // splitters out at all (a Remote Dev backend does not).
                LOG.info(
                    "[layout-detect] ${windows.size} editor windows share an origin; " +
                        "geometry unavailable, keeping each window as its own column"
                )
            }

            val columns = buildColumnsFromSnapshots(snapshots)
            LOG.debug(
                "[layout-detect] grouped into ${columns.size} column(s): " +
                    columns.joinToString(" | ") { col ->
                        "[" + col.files.joinToString(", ").ifEmpty { "<empty>" } + "]"
                    }
            )

            logObservedLayout(windows.size, snapshots, columns)

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

    @Volatile private var lastObservedLayout: String? = null

    /** GH #88: observations since the plugin loaded, carried in the INFO line. */
    private val layoutObservationCount = java.util.concurrent.atomic.AtomicLong(0)

    /** Re-log an unchanged observation every this many runs, so "stuck" reads differently from "never ran". */
    internal const val OBSERVED_LAYOUT_HEARTBEAT = 50L

    /**
     * GH #88: one snapshot per selected session document on a backend with no editor
     * windows. Every snapshot shares the origin, so each becomes its own column.
     */
    internal fun headlessSelectionSnapshots(selectedSessionFiles: List<String>): List<LayoutWindowSnapshot> =
        selectedSessionFiles.distinct().map { LayoutWindowSnapshot(x = 0, y = 0, file = it) }

    /** A local one-window observation is known structure, unlike headless remote ambiguity. */
    internal fun knownSingleWindowLayout(selectedSessionPath: String?): EditorLayout? =
        selectedSessionPath?.let { EditorLayout(listOf(LayoutColumn(listOf(it)))) }

    /**
     * A Remote Dev frontend owns one [ClientFileEditorManager]. Its selected-file
     * set is split evidence only when that ONE client reports at least two files.
     * Never combine one focused file from separate clients into a fabricated split.
     */
    internal fun uniqueRemoteSplitSelection(clientSelections: List<List<String>>): List<String>? {
        val candidates = clientSelections
            .map { it.distinct() }
            .filter { it.size >= 2 }
            .distinct()
        return candidates.singleOrNull()
    }

    /** GH #134: what one Remote Dev client reports, restricted to session documents. */
    internal data class RemoteClientSessionEditors(
        val visible: List<String>,
        val selected: List<String>,
        val open: List<String>,
    )

    /** GH #134: the columns to publish for a backend with no editor windows, and why. */
    internal data class RemoteLayoutResolution(
        val columns: List<LayoutColumn>,
        val source: String,
        val reason: String?,
    )

    /**
     * GH #157: all IntelliJ-owned state needed for the Remote Dev fold, captured while the caller
     * is on the EDT. The native fold consumes only these immutable strings and lists, so it can run
     * later on the observation worker without touching client editor services off the EDT.
     */
    internal data class RemoteLayoutSnapshot(
        val projectRoot: String,
        val clients: List<RemoteClientSessionEditors>,
        val focusedSessionFiles: List<String>,
    )

    internal fun snapshotRemoteLayout(
        project: com.intellij.openapi.project.Project,
        sessionDocumentPaths: Set<String>? = null,
    ): RemoteLayoutSnapshot {
        val clients = remoteClientSessionEditors(project, sessionDocumentPaths)
        val focusedSessionFiles = FileEditorManager.getInstance(project).selectedFiles
            .filter { file ->
                sessionDocumentPaths?.contains(file.path)
                    ?: AgentDocSessionFiles.isSessionDocument(file)
            }
            .map { TerminalUtil.relativePath(project, it) }
        return RemoteLayoutSnapshot(
            projectRoot = project.basePath ?: project.locationHash,
            clients = clients,
            focusedSessionFiles = focusedSessionFiles,
        )
    }

    /** Remote client-session evidence outranks incidental backend-local editor windows (GH #154). */
    internal fun shouldUseRemoteClientLayout(
        backendWindowCount: Int,
        remoteClientSessionCount: Int,
    ): Boolean = when {
        remoteClientSessionCount > 0 -> true
        backendWindowCount == 0 -> false
        else -> false
    }

    private fun remoteClientSessionEditors(
        project: com.intellij.openapi.project.Project,
        sessionDocumentPaths: Set<String>?,
    ): List<RemoteClientSessionEditors> {
        val remoteManagers = project.getServices(ClientFileEditorManager::class.java, ClientKind.REMOTE)
        if (remoteManagers.isEmpty()) return emptyList()
        val isSession = { file: VirtualFile ->
            sessionDocumentPaths?.contains(file.path) ?: AgentDocSessionFiles.isSessionDocument(file)
        }
        // On a Remote Dev backend the editor tracker is the split-aware
        // RdServerEditorTracker. Its active editors are the frontend text editors
        // whose visibility the client reported, but Editor/Preview can transiently
        // retain a hidden tab here (GH #175); the native fold treats selected as
        // focus evidence and prevents that edge from widening established width.
        val activeEditors = EditorOpenFileSurface.activeEditors(project)
        return remoteManagers
            .mapIndexed { index, manager ->
                try {
                    val visible = manager.getAllEditors()
                        .filter { fileEditor ->
                            val editor = (fileEditor as? com.intellij.openapi.fileEditor.TextEditor)?.editor
                            editor != null && editor in activeEditors
                        }
                        .mapNotNull { it.file }
                        .filter(isSession)
                        .map { TerminalUtil.relativePath(project, it) }
                        .distinct()
                    val selected = manager.getSelectedFiles()
                        .filter(isSession)
                        .map { TerminalUtil.relativePath(project, it) }
                    val open = manager.getAllFiles()
                        .filter(isSession)
                        .map { TerminalUtil.relativePath(project, it) }
                    RemoteClientSessionEditors(visible = visible, selected = selected, open = open)
                } catch (e: Exception) {
                    LOG.warn("[layout-detect] remote client $index selection unavailable", e)
                    // Preserve the session's presence: an unreadable remote client still means an
                    // incidental backend-local window is not authoritative for frontend layout.
                    RemoteClientSessionEditors(
                        visible = emptyList(),
                        selected = emptyList(),
                        open = emptyList(),
                    )
                }
            }
    }

    /**
     * Finish zero-window detection from an EDT snapshot. Direct callers may still enter on the
     * EDT, so only the immutable native fold is marshalled to a worker. The selection projection
     * calls this overload from its generation-owned delivery worker and pays no extra dispatch.
     */
    internal fun detectEditorLayout(
        snapshot: RemoteLayoutSnapshot,
        nativeFold: (String, String) -> String? = NativeAdminControls::resolveRemoteLayout,
    ): EditorLayout? {
        val resolve = {
            resolveRemoteLayout(snapshot, nativeFold)
        }
        val resolution =
            if (SwingUtilities.isEventDispatchThread()) {
                java.util.concurrent.CompletableFuture.supplyAsync(resolve).join()
            } else {
                resolve()
            }
        if (resolution == null || resolution.columns.isEmpty()) {
            logUnknownRemoteLayout(
                snapshot.focusedSessionFiles,
                snapshot.clients.map { it.selected },
                visibleSelections = snapshot.clients.map { it.visible },
                reason = resolution?.reason ?: "no_unique_client_split_set",
            )
            return null
        }
        val windows = resolution.columns.flatMap { column ->
            column.files.map { LayoutWindowSnapshot(x = 0, y = 0, file = it) }
        }
        logLayoutObservation(
            observedRemoteLayoutLine(
                windows,
                resolution.columns,
                resolution.source + (resolution.reason?.let { " reason=$it" } ?: ""),
                snapshot.clients,
            ),
        )
        return EditorLayout(resolution.columns)
    }

    /** GH #175: successful Remote Dev observations retain the raw evidence needed to audit width. */
    internal fun observedRemoteLayoutLine(
        snapshots: List<LayoutWindowSnapshot>,
        columns: List<LayoutColumn>,
        source: String,
        clients: List<RemoteClientSessionEditors>,
    ): String =
        observedLayoutLine(
            0,
            snapshots,
            columns,
        ) + " source=$source " + remoteClientEvidenceLine(clients)

    internal fun remoteClientEvidenceLine(clients: List<RemoteClientSessionEditors>): String {
        fun render(files: List<String>) = files.joinToString(",").ifEmpty { "<none>" }
        return "remote_clients=${clients.size} clients=[" + clients.mapIndexed { index, client ->
            "$index:{visible=[${render(client.visible)}] selected=[${render(client.selected)}] " +
                "open=[${render(client.open)}]}"
        }.joinToString(" ") + "]"
    }

    private fun resolveRemoteLayout(
        snapshot: RemoteLayoutSnapshot,
        nativeFold: (String, String) -> String?,
    ): RemoteLayoutResolution? {
        // GH #134: resolve through the shared native fold (`#ffi-first`), which keeps the previous
        // split per project. Without native support, retain the memoryless compatibility fallback.
        val evidenceJson = remoteLayoutEvidenceJson(snapshot.clients, snapshot.focusedSessionFiles)
        nativeFold(snapshot.projectRoot, evidenceJson)
            ?.let(::parseRemoteLayoutResolution)
            ?.let { return it }
        // Older native libraries are memoryless, but must still keep focus
        // evidence out of the visible split set (GH #175).
        val fallback = uniqueRemoteSplitSelection(snapshot.clients.map { it.visible })
            ?: uniqueRemoteSplitSelection(snapshot.clients.map { it.selected })
            ?: return null
        return RemoteLayoutResolution(
            columns = buildColumnsFromSnapshots(headlessSelectionSnapshots(fallback)),
            source = "remote_client_split_without_native",
            reason = null,
        )
    }

    internal fun remoteLayoutEvidenceJson(
        clients: List<RemoteClientSessionEditors>,
        focusedSessionFiles: List<String>,
    ): String = Gson().toJson(
        mapOf(
            "clients" to clients.map {
                mapOf("visible" to it.visible, "selected" to it.selected, "open" to it.open)
            },
            "focused" to focusedSessionFiles,
        ),
    )

    internal fun parseRemoteLayoutResolution(json: String): RemoteLayoutResolution? = try {
        val root = com.google.gson.JsonParser.parseString(json).asJsonObject
        val columns = root.getAsJsonArray("columns")
            ?.mapNotNull { column ->
                column.asJsonObject.getAsJsonArray("files")
                    ?.map { it.asString }
                    ?.filter { it.isNotBlank() }
                    ?.takeIf { it.isNotEmpty() }
                    ?.let(::LayoutColumn)
            }
            .orEmpty()
        RemoteLayoutResolution(
            columns = columns,
            source = root.get("source")?.takeIf { !it.isJsonNull }?.asString ?: "unknown",
            reason = root.get("reason")?.takeIf { !it.isJsonNull }?.asString,
        )
    } catch (e: Exception) {
        LOG.warn("[layout-detect] remote layout resolution unreadable: ${e.message}")
        null
    }

    /** Log a changed observation, and an unchanged one on every [OBSERVED_LAYOUT_HEARTBEAT]th run. */
    internal fun shouldLogObservedLayout(changed: Boolean, observation: Long): Boolean =
        changed || observation % OBSERVED_LAYOUT_HEARTBEAT == 0L

    /**
     * GH #81 discriminator: what this IDE reported, before the controller sees it.
     *
     * A single-document observation has two possible causes that the controller log cannot
     * tell apart -- the IDE exposing one editor window, or a lossy join downstream. This line
     * names the window count, each window's origin and document, and the columns built from
     * them. It is logged at info only when it changes, since detection runs on every surface
     * observation.
     */
    private fun logObservedLayout(
        windowCount: Int,
        snapshots: List<LayoutWindowSnapshot>,
        columns: List<LayoutColumn>,
        source: String = "windows",
    ) {
        val line = observedLayoutLine(windowCount, snapshots, columns) + " source=$source"
        logLayoutObservation(line)
    }

    private fun logUnknownRemoteLayout(
        focusedSessionFiles: List<String>,
        remoteSelections: List<List<String>>,
        visibleSelections: List<List<String>> = emptyList(),
        reason: String = "no_unique_client_split_set",
        windowCount: Int = 0,
    ) {
        logLayoutObservation(
            unknownRemoteLayoutLine(
                focusedSessionFiles,
                remoteSelections,
                visibleSelections,
                reason,
                windowCount,
            ),
        )
    }

    internal fun unknownRemoteLayoutLine(
        focusedSessionFiles: List<String>,
        remoteSelections: List<List<String>>,
        visibleSelections: List<List<String>> = emptyList(),
        reason: String = "no_unique_client_split_set",
        windowCount: Int = 0,
    ): String {
        fun render(perClient: List<List<String>>) = perClient.mapIndexed { index, files ->
            "$index:[${files.joinToString(",").ifEmpty { "<none>" }}]"
        }.joinToString(" ")
        val visible = if (visibleSelections.isEmpty()) "" else "visible=[${render(visibleSelections)}] "
        return "[layout-detect] unknown windows=$windowCount source=remote_client_selected_files " +
            "focused=[${focusedSessionFiles.joinToString(",").ifEmpty { "<none>" }}] " +
            "remote_clients=${remoteSelections.size} selections=[${render(remoteSelections)}] " +
            visible +
            "reason=$reason"
    }

    private fun logLayoutObservation(line: String) {
        val observation = layoutObservationCount.incrementAndGet()
        val changed = line != lastObservedLayout
        lastObservedLayout = line
        if (shouldLogObservedLayout(changed, observation)) {
            LOG.info("$line obs=$observation")
        }
    }

    internal fun observedLayoutLine(
        windowCount: Int,
        snapshots: List<LayoutWindowSnapshot>,
        columns: List<LayoutColumn>,
    ): String =
        "[layout-detect] observed windows=$windowCount " +
            "snapshots=[" + snapshots.joinToString(", ") {
                "(${it.x},${it.y}) ${it.file ?: "<none>"}"
            } + "] columns=" + columns.size + " [" +
            columns.joinToString(" | ") { it.files.joinToString(",").ifEmpty { "<empty>" } } + "]"

    private fun sharesOrigin(snapshots: List<LayoutWindowSnapshot>): Boolean =
        snapshots.groupBy { it.x to it.y }.any { (_, atOrigin) -> atOrigin.size > 1 }

    internal fun buildColumnsFromSnapshots(
        snapshots: List<LayoutWindowSnapshot>,
        columnTolerancePx: Int = COLUMN_X_TOLERANCE_PX,
    ): List<LayoutColumn> {
        if (snapshots.isEmpty()) return emptyList()

        // GH #77: two editor windows can never share an on-screen origin in a laid-out
        // split, so a shared origin means the IDE never laid the splitters out — a
        // JetBrains Remote Dev backend reports every split at (0,0). Grouping that by x
        // folded both splits into ONE column, the controller kept only its first agent
        // doc, and the tmux layout converged to a single pane that was swapped on every
        // document switch. Geometry is unavailable, so keep the windows apart, in window
        // order, rather than inventing a vertical stack the operator never made.
        if (sharesOrigin(snapshots)) {
            return snapshots.map { LayoutColumn(listOfNotNull(it.file)) }
        }

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
