package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.shell.ImplAction
import com.google.gson.Gson
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
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
class SyncLayoutAction : ImplAction {

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

        /**
         * GH #326 phase 3: frozen compatibility copy of the native `sync_layout` root choice
         * ([SyncLayoutSnapshot]). Never extend it; change the native derivation instead.
         */
        internal fun chooseSyncProjectRoot(
            basePath: String?,
            fallbackRoot: String,
            visibleMarkdownFiles: List<String>,
            nearestRoot: (String) -> String? = TerminalUtil::nearestAgentDocProjectRoot,
        ): String {
            val visibleRoots = visibleMarkdownFiles
                .mapNotNull(nearestRoot)
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

        /** GH #326 phase 3: frozen copy of the native `sync_layout` normalization. Never extend it. */
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
                EditorLayout(normalizedColumns, layout.columnOrder)
            } else {
                null
            }
        }

        /** GH #326 phase 3: frozen copy of the native `sync_layout` absolutization. Never extend it. */
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
                EditorLayout(absoluteColumns, layout.columnOrder)
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
         *
         * GH #326 phase 3: frozen copy of the native `sync_layout` rule. Never extend it.
         */
        internal fun undetectedLayoutColumns(visibleMdFiles: List<String>): List<String> =
            visibleMdFiles.filter(String::isNotBlank).distinct()

        /**
         * GH #112/#185: the order source of [buildSyncColumns] / route `--col` lists.
         * Only local detected geometry carries an `editor` left-to-right split order.
         * Remote Dev backend collections expose focus/liveness and tab-open order, not split
         * geometry; they remain structurally `unknown`.
         * The undetected fallback lists `selectedFiles`, which IntelliJ orders
         * focused-window first, so its order is `unknown` and the controller keeps
         * the order it already retains for those documents. On a Remote Dev
         * backend the detector is `unknown` whenever no single client reports a
         * multi-file split set (GH #97), so pane order there is stable, not mirrored.
         */
        internal fun syncColumnOrder(editorLayout: EditorLayout?): String =
            if (editorLayout != null && editorLayout.columns.size > 1) editorLayout.columnOrder else "unknown"

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
         *
         * GH #326 phase 3: frozen copy of the native `sync_layout` decision. Never extend it.
         */
        internal fun exactVisibleSyncDecision(
            visibleMdFiles: List<String>,
            editorLayout: EditorLayout?,
            focusedFile: String? = null,
            foreground: Boolean = true,
        ): ExactVisibleSyncDecision =
            editorLayout
                ?.let {
                    val columns = buildSyncColumns(visibleMdFiles, it)
                    if (foreground && focusedFile != null && columns.none { column ->
                            column.split(',').map(String::trim).any { file -> file == focusedFile }
                        }
                    ) {
                        ExactVisibleSyncDecision.RefuseStaleFocusProjection
                    } else {
                        ExactVisibleSyncDecision.Publish(columns)
                    }
                }
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
         *
         * The layout is the propagated editor surface (`surface_layout_reference_v1`): the sync
         * publishes the current surface on the [EditorTabSyncListener] path and names it, and the
         * controller derives the columns, their order label, and the focus from its surface
         * Source. [allowSurfaceReference] `false` is the rollout fallback that sends the editor's
         * `--col` snapshot instead (an older controller, or a surface this listener cannot
         * publish to the sync's controller).
         *
         * GH #326 phase 3: the EDT only reads editor state (the focused and visible session
         * documents, and the command surface or the split layout evidence). The root choice, the
         * layout derivation, and the `--col` decisions are the native `sync_layout` derivation
         * ([SyncLayoutSnapshot]) on the background thread.
         */
    fun syncLayout(
        project: com.intellij.openapi.project.Project,
        notify: Boolean = true,
        noAutostart: Boolean = false,
        callerKind: String? = null,
        terminalPrepared: Boolean = false,
        allowSurfaceReference: Boolean = true,
    ) {
            if (!SwingUtilities.isEventDispatchThread()) {
                ApplicationManager.getApplication().invokeLater {
                syncLayout(project, notify, noAutostart, callerKind, terminalPrepared, allowSurfaceReference)
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
            val visibleMdFiles = collectVisibleMarkdownFiles(
                (listOf(focusedVFile) + manager.selectedFiles).toTypedArray(),
                AgentDocSessionFiles::isSessionDocument,
            )
            if (visibleMdFiles.isEmpty()) {
                if (notify) TerminalUtil.showHint(project, "No .md files open")
                return
            }
            val request = SyncRequest(
                project = project,
                notify = notify,
                noAutostart = noAutostart,
                callerKind = callerKind,
                terminalPrepared = terminalPrepared,
                allowSurfaceReference = allowSurfaceReference,
                basePath = project.basePath,
                focusedFile = focusedVFile.path,
                visibleMdFiles = visibleMdFiles,
            )
            if (allowSurfaceReference) {
                val listener = EditorTabSyncListener.forProject(project)
                val commandSurface = listener?.captureCommandSurface(project, focusedVFile)
                if (listener != null && commandSurface != null) {
                    syncLayoutBySurfaceReference(request, listener, commandSurface)
                    return
                }
                LOG.warn(
                    "[sync] surface reference unavailable reason=" +
                        (if (listener == null) "no_surface_listener" else "surface_capture_deferred") +
                        "; sending the --col snapshot",
                )
            }
            // IDEA component state is captured on the EDT exactly once; the layout it implies is
            // derived on the background thread below.
            val layoutEvidence = LayoutDetector.captureEditorLayoutEvidence(
                project,
                manager.openFiles
                    .filter(AgentDocSessionFiles::isSessionDocument)
                    .map { it.path }
                    .toSet(),
            )
            syncLayoutBySnapshot(request, layoutEvidence)
        }

    /** What one Sync Tmux Layout read from the editor on the EDT. */
    private data class SyncRequest(
        val project: com.intellij.openapi.project.Project,
        val notify: Boolean,
        val noAutostart: Boolean,
        val callerKind: String?,
        val terminalPrepared: Boolean,
        val allowSurfaceReference: Boolean,
        val basePath: String?,
        val focusedFile: String,
        val visibleMdFiles: List<String>,
    ) {
        val failureAction: String
            get() = if (noAutostart) "sync the tmux layout" else "open the tmux pane"

        fun notifyFailure(detail: String) {
            if (!notify) return
            TerminalUtil.showEditorFailureStatus(project, focusedFile, failureAction, detail)
            TerminalUtil.notifyError(project, detail)
        }
    }

    /**
     * GH #326 phase 3: the native `sync_layout` derivation of [request] over [editorLayout]. Off
     * the EDT: resolving the focused document's own root reads the filesystem.
     */
    private fun deriveSyncLayout(request: SyncRequest, editorLayout: EditorLayout?): SyncLayoutSnapshot.Result {
        val (focusedProjectRoot, _) = TerminalUtil.resolveProject(request.project, request.focusedFile)
        return SyncLayoutSnapshot.derive(
            SyncLayoutSnapshot.evidence(
                basePath = request.basePath,
                fallbackRoot = focusedProjectRoot,
                focused = request.focusedFile,
                visible = request.visibleMdFiles,
                editorLayout = editorLayout,
                foreground = !request.noAutostart,
            ),
        )
    }

    /**
     * Prepare the agent-doc tmux session at [projectRoot] when this caller asked for it, then
     * re-read the editor and sync again. `false` when the terminal was already prepared.
     */
    private fun prepareTerminalFirst(request: SyncRequest, projectRoot: String): Boolean {
        if (request.terminalPrepared || request.noAutostart) return false
        val relativeFocusedFile = java.io.File(projectRoot).toPath()
            .relativize(java.io.File(request.focusedFile).toPath())
            .toString()
        IdeTerminalCoordinator.ensureAndAttach(
            project = request.project,
            cwd = projectRoot,
            relativePath = relativeFocusedFile,
            onReady = {
                syncLayout(
                    project = request.project,
                    notify = request.notify,
                    noAutostart = request.noAutostart,
                    callerKind = request.callerKind,
                    terminalPrepared = true,
                    allowSurfaceReference = request.allowSurfaceReference,
                )
            },
            onFailure = { message ->
                request.notifyFailure("Failed to prepare agent-doc tmux session: $message")
            },
        )
        return true
    }

    /** `false` (after telling the user) when the native-generation handoff did not finish. */
    private fun awaitNativeGeneration(request: SyncRequest): Boolean {
        if (NativeReloadCoordinator.awaitReady()) return true
        request.notifyFailure(
            "Sync deferred because the native-generation handoff did not " +
                "finish within ${NativeReloadCoordinator.USER_ACTION_AWAIT_MS / 1_000} seconds.",
        )
        return false
    }

    /**
     * The `--col` snapshot half of [syncLayout]. Background thread: derive the layout from the
     * EDT's [layoutEvidence], ask `sync_layout` for the root, columns and exact-visible decision,
     * refuse an unknown split or a stale focus projection, prepare the terminal, then submit.
     */
    private fun syncLayoutBySnapshot(
        request: SyncRequest,
        layoutEvidence: LayoutDetector.EditorLayoutEvidence?,
    ) {
        Thread {
            try {
                if (request.project.isDisposed) return@Thread
                val snapshot = deriveSyncLayout(
                    request,
                    layoutEvidence?.let { LayoutDetector.detectEditorLayout(it) },
                )
                val projectRoot = snapshot.projectRoot
                val exactVisibleColumns =
                    when (val decision = snapshot.decision) {
                        is ExactVisibleSyncDecision.Publish -> decision.columns
                        ExactVisibleSyncDecision.RefuseUnknownLayout -> {
                            LOG.warn("[sync] refusing exact-visible layout because editor columns are unknown")
                            request.notifyFailure(SurfaceCommandReference.UNKNOWN_LAYOUT_MESSAGE)
                            return@Thread
                        }
                        ExactVisibleSyncDecision.RefuseStaleFocusProjection -> {
                            LOG.warn(
                                "[sync] refusing exact-visible layout because focused file is absent " +
                                    "focus=${request.focusedFile} visible=${request.visibleMdFiles} " +
                                    "layout=${snapshot.editorLayout}",
                            )
                            request.notifyFailure(
                                "Editor focus changed while the split layout was being read; " +
                                    "retry after the selection settles.",
                            )
                            return@Thread
                        }
                    }
                if (prepareTerminalFirst(request, projectRoot)) return@Thread
                if (!awaitNativeGeneration(request)) return@Thread
                val receipt = CpRouteClient.submitSyncTmuxLayout(
                    projectRoot = projectRoot,
                    columnsJson = GSON.toJson(exactVisibleColumns),
                    window = null,
                    focus = request.focusedFile,
                    noAutostart = request.noAutostart,
                    exactVisible = true,
                    callerKind = syncCallerKind(request.noAutostart, request.callerKind),
                    columnOrder = snapshot.columnOrder,
                )
                if (receipt.exitCode != 0) {
                    LOG.warn(
                        "[sync] Project Controller projection failed projectRoot=$projectRoot " +
                            "focus=${request.focusedFile} columns=$exactVisibleColumns output=${receipt.output}",
                    )
                    request.notifyFailure(syncFailureMessage(receipt.output))
                } else {
                    LOG.info("[sync] Project Controller projection completed: ${receipt.output.take(500)}")
                }
            } catch (ex: Exception) {
                request.notifyFailure("Failed to sync layout: ${ex.message}")
            }
        }.start()
    }

    /**
     * The surface-reference half of [syncLayout]: prepare the terminal like the snapshot path,
     * then publish the captured surface and submit `sync_tmux_layout` naming it. The controller
     * owns the exact-visible decisions the snapshot path makes in the plugin (an unknown split or
     * a focus outside the columns is refused there, with the same message). Background thread:
     * the root is the `sync_layout` derivation's.
     */
    private fun syncLayoutBySurfaceReference(
        request: SyncRequest,
        listener: EditorTabSyncListener,
        commandSurface: EditorTabSyncListener.CommandSurfaceCapture,
    ) {
        val project = request.project
        val focusedFile = request.focusedFile
        Thread {
            try {
                if (project.isDisposed) return@Thread
                val projectRoot = deriveSyncLayout(request, editorLayout = null).projectRoot
                if (prepareTerminalFirst(request, projectRoot)) return@Thread
                if (!awaitNativeGeneration(request)) return@Thread
                when (
                    val publication = listener.publishCommandSurface(
                        commandSurface,
                        projectRoot,
                        SurfaceCommandReference.Command.ExactSync,
                    )
                ) {
                    is EditorTabSyncListener.CommandSurfacePublication.Unavailable -> {
                        LOG.warn(
                            "[sync] surface reference unavailable reason=${publication.reason}; " +
                                "sending the --col snapshot",
                        )
                        ApplicationManager.getApplication().invokeLater {
                            if (!project.isDisposed) {
                                // GH #334: keep the first pass's client-scoped focus. Re-entering
                                // syncLayout here runs under the backend ClientId in Remote Dev and
                                // can no longer see the selected ClientFileEditorManager document.
                                val layoutEvidence = LayoutDetector.captureEditorLayoutEvidence(
                                    project,
                                    request.visibleMdFiles.toSet(),
                                )
                                syncLayoutBySnapshot(
                                    request.copy(
                                        terminalPrepared = true,
                                        allowSurfaceReference = false,
                                    ),
                                    layoutEvidence,
                                )
                            }
                        }
                    }
                    is EditorTabSyncListener.CommandSurfacePublication.Published -> {
                        val reference = publication.reference
                        val receipt = CpRouteClient.submitSyncTmuxLayout(
                            projectRoot = projectRoot,
                            columnsJson = "[]",
                            window = null,
                            focus = focusedFile,
                            noAutostart = request.noAutostart,
                            exactVisible = true,
                            callerKind = syncCallerKind(request.noAutostart, request.callerKind),
                            surfaceReference = reference,
                        )
                        if (receipt.exitCode != 0) {
                            LOG.warn(
                                "[sync] Project Controller projection failed projectRoot=$projectRoot " +
                                    "surface=${reference.surfaceId} min_sequence=${reference.minSequence} " +
                                    "output=${receipt.output}",
                            )
                            request.notifyFailure(
                                SurfaceCommandReference.refusalMessage(receipt.output)
                                    ?: syncFailureMessage(receipt.output),
                            )
                        } else {
                            LOG.info(
                                "[sync] Project Controller projection completed by surface reference " +
                                    "surface=${reference.surfaceId} min_sequence=${reference.minSequence}: " +
                                    receipt.output.take(500),
                            )
                        }
                    }
                    is EditorTabSyncListener.CommandSurfacePublication.PreservedLayout -> {
                        LOG.info(
                            "[sync] flat surface retained without structural sync " +
                                "surface=${publication.reference.surfaceId} " +
                                "min_sequence=${publication.reference.minSequence} " +
                                "reason=flat_evidence_preserves_retained_layout",
                        )
                    }
                }
            } catch (ex: Exception) {
                request.notifyFailure("Failed to sync layout: ${ex.message}")
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
}

/**
 * Represents a detected 2D editor layout.
 */
data class LayoutColumn(val files: List<String>)
data class EditorLayout(
    val columns: List<LayoutColumn>,
    val columnOrder: String = "editor",
)

internal sealed interface ExactVisibleSyncDecision {
    data class Publish(val columns: List<String>) : ExactVisibleSyncDecision
    data object RefuseUnknownLayout : ExactVisibleSyncDecision
    data object RefuseStaleFocusProjection : ExactVisibleSyncDecision
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
    ): List<String> = markdownTabsMruLast(windowTabPaths, editorHistoryPaths(project))

    /** Frozen compatibility copy of the native `session_tabs_mru_last` (GH #326). */
    internal fun markdownTabsMruLast(
        windowTabPaths: List<String>,
        history: List<String>,
    ): List<String> =
        windowTabPaths
            .filter { it.endsWith(".md") }
            .sortedBy { history.indexOf(it) }

    /** The IDE's own selection history, least recently used first. */
    private fun editorHistoryPaths(project: com.intellij.openapi.project.Project): List<String> =
        try {
            com.intellij.openapi.fileEditor.impl.EditorHistoryManager
                .getInstance(project)
                .fileList
                .map { it.path }
        } catch (e: Exception) {
            LOG.debug("[layout-detect] editor history unavailable: ${e.message}")
            emptyList()
        }

    /**
     * `#clearnoticeverify`: the editor windows of the frame the operator is looking
     * at. Editor windows of other frames (a detached editor window) are a separate
     * screen surface: their bounds cannot be measured against this frame's
     * splitters, and they are not columns of this split. Falls back to every window
     * when no window belongs to [activeOwner] (an unresolved owner never hides the
     * layout).
     */
    internal fun <W, O> windowsInActiveFrame(windows: List<W>, ownerOf: (W) -> O?, activeOwner: O?): List<W> {
        if (activeOwner == null) return windows
        val inFrame = windows.filter { ownerOf(it) === activeOwner }
        return inFrame.ifEmpty { windows }
    }

    /**
     * Detect the editor layout as a list of columns, each containing stacked files.
     * Returns null only when detection fails. A proven single local editor window
     * returns one column so consumers can distinguish it from an unknown layout.
     */
    fun detectEditorLayout(
        project: com.intellij.openapi.project.Project,
        sessionDocumentPaths: Set<String>? = null,
        surfaceWindows: List<com.intellij.openapi.fileEditor.impl.EditorWindow>? = null,
    ): EditorLayout? =
        try {
            captureEditorLayoutEvidence(project, sessionDocumentPaths, surfaceWindows)
                ?.let { detectEditorLayout(it) }
        } catch (e: Exception) {
            LOG.warn("[layout-detect] editor layout detection failed: ${e.message}", e)
            null
        }

    /**
     * GH #326 phase 3: the editor layout evidence one EDT turn reads. Its derivation
     * ([detectEditorLayout]) runs later, off the EDT, on whichever thread holds the evidence.
     */
    internal sealed interface EditorLayoutEvidence {
        /** Local editor windows of the mirrored frame (`window_layout`). */
        data class Windows(val evidence: WindowLayoutEvidence) : EditorLayoutEvidence

        /** Remote Dev client sessions (native non-structural classification, GH #134/#157/#343). */
        data class Remote(val snapshot: RemoteLayoutSnapshot) : EditorLayoutEvidence
    }

    /** The layout [evidence] implies. Off the EDT this pays no dispatch and no budget. */
    internal fun detectEditorLayout(
        evidence: EditorLayoutEvidence,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): EditorLayout? =
        when (evidence) {
            is EditorLayoutEvidence.Windows -> detectEditorLayout(evidence.evidence, derivation)
            is EditorLayoutEvidence.Remote -> detectEditorLayout(evidence.snapshot)
        }

    /**
     * Capture what the editor layout IS, on the caller's (EDT) turn. No layout rule runs here;
     * `null` when the frame's windows cannot be measured or the editor state is unreadable.
     */
    internal fun captureEditorLayoutEvidence(
        project: com.intellij.openapi.project.Project,
        sessionDocumentPaths: Set<String>? = null,
        surfaceWindows: List<com.intellij.openapi.fileEditor.impl.EditorWindow>? = null,
    ): EditorLayoutEvidence? {
        try {
            val managerEx = FileEditorManagerEx.getInstanceEx(project)
            val windows = surfaceWindows ?: run {
                // `#clearnoticeverify` (2026-10-11): `managerEx.windows` holds the
                // editor windows of EVERY frame, including a detached editor frame.
                // Measured against the first window's splitters, a detached frame's
                // editor got foreign coordinates (-56,-117), sorted first, and was
                // published as an extra tmux column: a 2-split main frame became 3
                // panes. Mirror only the frame the operator is looking at.
                val all = managerEx.windows.toList()
                val activeOwner = managerEx.currentWindow?.owner ?: managerEx.splitters
                val inFrame = windowsInActiveFrame(all, { it.owner }, activeOwner)
                if (inFrame.size < all.size) {
                    LOG.debug(
                        "[layout-detect] ignoring ${all.size - inFrame.size} editor window(s) in other frames",
                    )
                }
                inFrame
            }
            val remoteClients =
                if (surfaceWindows == null) {
                    remoteClientSessionEditors(project, sessionDocumentPaths)
                } else {
                    emptyList()
                }
            if (
                surfaceWindows == null &&
                    shouldUseRemoteClientLayout(windows.size, remoteClients.size)
            ) {
                // GH #97: backend-local FileEditorManager.selectedFiles is only the
                // focused file in Remote Dev. JetBrains keeps the real per-frontend
                // selections in client-scoped managers; this is the same service set
                // FileEditorManagerImpl uses for its own "with remotes" operations.
                // GH #134: those selections are focused-only too, so the visible
                // split set comes from the Remote Dev editor tracker, and the shared
                // native fold retains a known split across single-selection
                // observations instead of answering `unknown` forever.
                return EditorLayoutEvidence.Remote(snapshotRemoteLayout(project, sessionDocumentPaths))
            }
            // GH #326: capture what each window IS on this thread; the hot-reloaded native library
            // derives the layout from it (`window_layout`), off the EDT.
            return captureWindowLayoutEvidence(project, windows, sessionDocumentPaths)
                ?.let(EditorLayoutEvidence::Windows)
        } catch (e: Exception) {
            LOG.warn("[layout-detect] editor layout capture failed: ${e.message}", e)
            return null
        }
    }

    /** GH #326: one local editor window as observed. Wire shape of Rust `EditorWindowEvidence`. */
    internal data class EditorWindowEvidence(
        val x: Int?,
        val y: Int?,
        val selected: String?,
        @SerializedName("session_tabs") val sessionTabs: List<String>,
    )

    /** GH #326: the local windows of the mirrored frame. Wire shape of Rust `WindowLayoutEvidence`. */
    internal data class WindowLayoutEvidence(
        @SerializedName("base_path") val basePath: String?,
        val windows: List<EditorWindowEvidence>,
        val history: List<String>,
    )

    /** The layout the `window_layout` derivation answered, and what to log about it. */
    internal data class WindowLayoutResolution(
        val layout: EditorLayout?,
        val observedLine: String?,
        val diagnostic: String?,
        val reason: String?,
    )

    /** How long a direct EDT caller waits for the off-EDT native derivation before the fallback. */
    internal const val EDT_DERIVATION_BUDGET_MS = 250L

    private val GSON = Gson()

    /**
     * GH #326: what each editor window IS, read on the caller's (EDT) turn: its origin in the
     * frame's splitter coordinates (null when unreadable, GH #81), its selected tab, and its
     * session-document tabs. No layout rule runs here; `null` only when two or more windows exist
     * but their frame has no splitter component to measure against.
     */
    internal fun captureWindowLayoutEvidence(
        project: com.intellij.openapi.project.Project,
        windows: List<com.intellij.openapi.fileEditor.impl.EditorWindow>,
        sessionDocumentPaths: Set<String>?,
    ): WindowLayoutEvidence? =
        captureEditorWindows(project, windows, sessionDocumentPaths)
            .takeUnless(CapturedEditorWindows::geometryUnavailable)
            ?.evidence

    /**
     * GH #326: what the windows of a frame are, plus whether their frame could be measured. When it
     * could not, every window is still reported (selected tab and session tabs, no origin), because
     * the surface projection reads the selections even when no local layout can be derived.
     */
    internal data class CapturedEditorWindows(
        val evidence: WindowLayoutEvidence,
        val geometryUnavailable: Boolean,
    )

    internal fun captureEditorWindows(
        project: com.intellij.openapi.project.Project,
        windows: List<com.intellij.openapi.fileEditor.impl.EditorWindow>,
        sessionDocumentPaths: Set<String>?,
    ): CapturedEditorWindows {
        var geometryUnavailable = false
        val splittersComponent =
            if (windows.size >= 2) {
                val splitters = windows.firstOrNull()?.owner
                    ?: FileEditorManagerEx.getInstanceEx(project).splitters
                (splitters as? java.awt.Component) ?: run {
                    LOG.debug(
                        "[layout-detect] ${windows.size} editor windows but splitters component " +
                            "unavailable; cannot resolve columns",
                    )
                    geometryUnavailable = true
                    null
                }
            } else {
                null
            }
        val sessionPaths = sessionDocumentPaths ?: windows
            .flatMap { it.fileList.toList() }
            .filter(AgentDocSessionFiles::isSessionDocument)
            .map { it.path }
            .toSet()
        val evidence = windows.map { window ->
            val bounds = splittersComponent?.let { target ->
                try {
                    val component = window.tabbedPane.component
                    component.parent?.let { parent ->
                        SwingUtilities.convertRectangle(parent, component.bounds, target)
                    } ?: component.bounds
                } catch (e: Exception) {
                    LOG.debug("[layout-detect] editor window bounds unavailable: ${e.message}")
                    null
                }
            }
            EditorWindowEvidence(
                x = bounds?.x,
                y = bounds?.y,
                selected = window.selectedFile?.path,
                sessionTabs = window.fileList.map { it.path }.filter(sessionPaths::contains),
            )
        }
        val tabs = evidence.flatMap(EditorWindowEvidence::sessionTabs).toSet()
        return CapturedEditorWindows(
            evidence =
                WindowLayoutEvidence(
                    basePath = project.basePath,
                    windows = evidence,
                    history = editorHistoryPaths(project).filter(tabs::contains),
                ),
            geometryUnavailable = geometryUnavailable,
        )
    }

    /**
     * GH #326: the layout [evidence] implies, derived by the hot-reloaded native library. A direct
     * EDT caller marshals only the immutable evidence to a worker and waits at most
     * [EDT_DERIVATION_BUDGET_MS]; the surface delivery worker calls this off the EDT and pays no
     * dispatch.
     */
    internal fun detectEditorLayout(
        evidence: WindowLayoutEvidence,
        derivation: EditorDerivation = EditorDerivation.Native,
    ): EditorLayout? {
        val resolution = resolveWindowLayout(evidence, derivation)
        resolution.diagnostic?.let { LOG.info(it) }
        resolution.observedLine?.let(::logLayoutObservation)
        if (resolution.layout == null) {
            LOG.debug("[layout-detect] no layout reason=${resolution.reason ?: "<none>"}")
        }
        return resolution.layout
    }

    internal fun resolveWindowLayout(
        evidence: WindowLayoutEvidence,
        derivation: EditorDerivation,
    ): WindowLayoutResolution {
        val inputJson = GSON.toJson(evidence)
        val native = { derivation.derive("window_layout", inputJson) }
        val answer =
            if (SwingUtilities.isEventDispatchThread()) {
                try {
                    CompletableFuture.supplyAsync(native)
                        .get(EDT_DERIVATION_BUDGET_MS, TimeUnit.MILLISECONDS)
                } catch (e: java.util.concurrent.TimeoutException) {
                    LOG.warn(
                        "[layout-detect] native window_layout exceeded ${EDT_DERIVATION_BUDGET_MS}ms " +
                            "on an EDT caller; using the compatibility fallback",
                    )
                    null
                } catch (e: Exception) {
                    LOG.warn("[layout-detect] native window_layout failed: ${e.message}")
                    null
                }
            } else {
                native()
            }
        return answer?.let(::parseWindowLayoutResolution) ?: frozenWindowLayout(evidence)
    }

    /** Parse the native `window_layout` output. Empty columns inside a layout are kept. */
    internal fun parseWindowLayoutResolution(json: String): WindowLayoutResolution? = try {
        val root = com.google.gson.JsonParser.parseString(json).asJsonObject
        fun string(name: String): String? =
            root.get(name)?.takeIf { !it.isJsonNull }?.asString
        val columns = root.getAsJsonArray("columns")
            ?.map { column ->
                LayoutColumn(
                    column.asJsonObject.getAsJsonArray("files")
                        ?.map { it.asString }
                        ?.filter(String::isNotBlank)
                        .orEmpty(),
                )
            }
            .orEmpty()
        WindowLayoutResolution(
            layout = columns.takeIf { it.isNotEmpty() }?.let {
                EditorLayout(it, string("column_order") ?: "unknown")
            },
            observedLine = string("observed_line"),
            diagnostic = string("diagnostic"),
            reason = string("reason"),
        )
    } catch (e: Exception) {
        LOG.warn("[layout-detect] native window layout unreadable: ${e.message}")
        null
    }

    /**
     * GH #326 compatibility floor: the GH #320 `window_layout` rules, frozen. Used only when no
     * native generation answers. Never extend it; change the native derivation instead.
     */
    internal fun frozenWindowLayout(evidence: WindowLayoutEvidence): WindowLayoutResolution {
        val windows = evidence.windows
        fun relative(path: String): String = projectRelativePath(evidence.basePath, path)
        if (windows.size < 2) {
            val selected = windows.firstOrNull()?.let { window ->
                window.selected?.takeIf(window.sessionTabs::contains)?.let(::relative)
            }
            val snapshots = windows.map { LayoutWindowSnapshot(0, 0, selected) }
            val layout = knownSingleWindowLayout(selected)
            return WindowLayoutResolution(
                layout = layout,
                observedLine =
                    observedLayoutLine(windows.size, snapshots, layout?.columns.orEmpty()) +
                        " source=windows",
                diagnostic = null,
                reason = if (layout == null) "no_selected_session_document" else null,
            )
        }
        val geometryKnown = windows.all { it.x != null && it.y != null }
        val snapshots = windows.map { window ->
            LayoutWindowSnapshot(
                x = if (geometryKnown) window.x!! else 0,
                y = if (geometryKnown) window.y!! else 0,
                file = stickyMarkdownForWindow(
                    selectedPath = window.selected,
                    windowMarkdownTabsMruLast = markdownTabsMruLast(window.sessionTabs, evidence.history),
                )?.let(::relative),
            )
        }
        if (snapshots.none { it.file != null }) {
            return WindowLayoutResolution(null, null, null, "no_session_document_in_any_window")
        }
        val diagnostic =
            if (sharesOrigin(snapshots)) {
                "[layout-detect] ${windows.size} editor windows share an origin; " +
                    "geometry unavailable, keeping each window as its own column"
            } else {
                null
            }
        val columns = buildColumnsFromSnapshots(snapshots)
        val line = observedLayoutLine(windows.size, snapshots, columns) + " source=windows"
        return if (columns.size >= 2) {
            WindowLayoutResolution(EditorLayout(columns, measuredColumnOrder(snapshots)), line, diagnostic, null)
        } else {
            WindowLayoutResolution(null, line, diagnostic, "fewer_than_two_columns")
        }
    }

    /** [TerminalUtil.relativePath]'s rule over a plain path. */
    internal fun projectRelativePath(basePath: String?, path: String): String =
        if (basePath != null && path.startsWith(basePath)) path.removePrefix("$basePath/") else path

    @Volatile private var lastObservedLayout: String? = null

    /** GH #88: observations since the plugin loaded, carried in the INFO line. */
    private val layoutObservationCount = java.util.concurrent.atomic.AtomicLong(0)

    /** Re-log an unchanged observation every this many runs, so "stuck" reads differently from "never ran". */
    internal const val OBSERVED_LAYOUT_HEARTBEAT = 50L

    /** A local one-window observation is known structure, unlike headless remote ambiguity. */
    internal fun knownSingleWindowLayout(selectedSessionPath: String?): EditorLayout? =
        selectedSessionPath?.let { EditorLayout(listOf(LayoutColumn(listOf(it)))) }

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
        val columnOrder: String,
        val reason: String?,
    )

    /**
     * GH #157: all IntelliJ-owned state needed for Remote Dev backend observation, captured while
     * the caller is on the EDT. Native classification consumes only these immutable strings and lists, so it can run
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

    /**
     * GH #334: Remote Dev client files do not necessarily appear in the backend-local
     * FileEditorManager. The client managers have already classified these paths as session
     * documents, so carry their absolute paths into the automatic surface projection directly.
     */
    internal fun remoteSessionDocumentPaths(snapshot: RemoteLayoutSnapshot): List<String> =
        snapshot.clients
            .asSequence()
            .flatMap { client ->
                sequenceOf(client.open.asSequence(), client.visible.asSequence(), client.selected.asSequence())
                    .flatten()
            }
            .filter(String::isNotBlank)
            .map { path ->
                val file = File(path)
                (if (file.isAbsolute) file else File(snapshot.projectRoot, path))
                    .toPath()
                    .normalize()
                    .toString()
            }
            .distinct()
            .toList()

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
        val remoteManagers = MarketplacePlatformBridge.remoteFileEditorManagers(project)
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
                    val visible = MarketplacePlatformBridge.allEditors(manager)
                        .filter { fileEditor ->
                            val editor = (fileEditor as? com.intellij.openapi.fileEditor.TextEditor)?.editor
                            editor != null && editor in activeEditors
                        }
                        .mapNotNull { it.file }
                        .filter(isSession)
                        .map { TerminalUtil.relativePath(project, it) }
                        .distinct()
                    val selected = MarketplacePlatformBridge.selectedFiles(manager)
                        .filter(isSession)
                        .map { TerminalUtil.relativePath(project, it) }
                    val open = MarketplacePlatformBridge.allFiles(manager)
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
     * EDT, so only the immutable native observation is marshalled to a worker. The selection projection
     * calls this overload from its generation-owned delivery worker and pays no extra dispatch.
     */
    internal fun detectEditorLayout(
        snapshot: RemoteLayoutSnapshot,
        nativeResolver: (String, String) -> String? = { projectRoot, evidenceJson ->
            nativeRemoteLayout(projectRoot, evidenceJson)
        },
    ): EditorLayout? {
        val resolve = {
            resolveRemoteLayout(snapshot, nativeResolver)
        }
        val resolution =
            if (SwingUtilities.isEventDispatchThread()) {
                java.util.concurrent.CompletableFuture.supplyAsync(resolve).join()
            } else {
                resolve()
            }
        if (resolution.columns.isEmpty()) {
            logUnknownRemoteLayout(
                snapshot.focusedSessionFiles,
                snapshot.clients.map { it.selected },
                visibleSelections = snapshot.clients.map { it.visible },
                reason = resolution.reason ?: "no_unique_client_split_set",
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
        return EditorLayout(resolution.columns, resolution.columnOrder)
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
        nativeResolver: (String, String) -> String?,
    ): RemoteLayoutResolution {
        // GH #134/#343: resolve through the shared native compatibility boundary (`#ffi-first`).
        // Flat backend evidence is non-structural and production resolution returns no columns.
        // The modular exact frontend vector uses a separate forwarding lane. With no native answer
        // the layout is unknown (structurally inert, GH #105).
        val evidenceJson = remoteLayoutEvidenceJson(snapshot.clients, snapshot.focusedSessionFiles)
        return nativeResolver(snapshot.projectRoot, evidenceJson)?.let(::parseRemoteLayoutResolution)
            ?: NO_NATIVE_REMOTE_LAYOUT
    }

    /** `reason` of the unknown Remote Dev layout when no native generation answers. */
    internal const val NO_NATIVE_REMOTE_LAYOUT_REASON = "native_remote_layout_unavailable"

    private val NO_NATIVE_REMOTE_LAYOUT =
        RemoteLayoutResolution(
            columns = emptyList(),
            source = "unknown",
            columnOrder = "unknown",
            reason = NO_NATIVE_REMOTE_LAYOUT_REASON,
        )

    /**
     * GH #326 phase 4: the Remote Dev compatibility observation, asked through `remote_layout` in the
     * hot-reloaded native library. A library that predates the derivation is asked through the
     * pre-#326 `agent_doc_editor_surface_resolve_remote_layout_json` symbol. Both paths classify flat
     * evidence as non-structural. `null` when neither answers. Never call it on the EDT.
     */
    internal fun nativeRemoteLayout(
        projectRoot: String,
        evidenceJson: String,
        derivation: EditorDerivation = EditorDerivation.Native,
        legacyResolver: (String, String) -> String? = NativeAdminControls::resolveRemoteLayout,
    ): String? =
        derivation.derive("remote_layout", remoteLayoutDerivationInput(projectRoot, evidenceJson))
            ?: legacyResolver(projectRoot, evidenceJson)

    /** The `remote_layout` input: flat observation evidence keyed by project root. */
    internal fun remoteLayoutDerivationInput(projectRoot: String, evidenceJson: String): String =
        com.google.gson.JsonParser.parseString(evidenceJson).asJsonObject
            .apply { addProperty("project_root", projectRoot) }
            .toString()

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
            columnOrder = root.get("column_order")?.takeIf { !it.isJsonNull }?.asString ?: "unknown",
            reason = root.get("reason")?.takeIf { !it.isJsonNull }?.asString,
        )
    } catch (e: Exception) {
        LOG.warn("[layout-detect] remote layout resolution unreadable: ${e.message}")
        null
    }

    /** Log a changed observation, and an unchanged one on every [OBSERVED_LAYOUT_HEARTBEAT]th run. */
    internal fun shouldLogObservedLayout(changed: Boolean, observation: Long): Boolean =
        changed || observation % OBSERVED_LAYOUT_HEARTBEAT == 0L

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

    /** GH #326: log a native-derived `[layout-detect]` line through the shared dedup. */
    internal fun logObservedLayout(line: String) = logLayoutObservation(line)

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

    /**
     * GH #320: the order provenance of columns built from [snapshots]. Only windows laid out at
     * distinct origins are measured geometry (`editor`); a shared origin (a Remote Dev backend, or
     * any unlaid-out splitter) leaves window-list order, which is `unknown`.
     */
    internal fun measuredColumnOrder(snapshots: List<LayoutWindowSnapshot>): String =
        if (sharesOrigin(snapshots)) "unknown" else "editor"

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
