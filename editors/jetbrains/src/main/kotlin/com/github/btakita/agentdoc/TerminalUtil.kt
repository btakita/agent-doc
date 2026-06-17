package com.github.btakita.agentdoc

import com.intellij.codeInsight.hint.HintManager
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import java.awt.datatransfer.StringSelection
import java.io.File
import java.util.concurrent.TimeUnit

object TerminalUtil {
    private val LOG = Logger.getInstance(TerminalUtil::class.java)
    private const val ROUTE_ERROR_DIAGNOSTICS_DIR = ".agent-doc/state/editor-route-errors"
    private const val RESTART_TELEMETRY_OPS_LOG_MAX_LINES = 400
    private const val UI_OUTCOME_QUEUED_BEHIND_OWNER = "queued_behind_owner"
    internal const val STARTING_ACTOR_ROUTE_MAX_ATTEMPTS = 4
    private val STARTING_ACTOR_ROUTE_RETRY_DELAYS_MILLIS = longArrayOf(2_000L, 4_000L, 8_000L)
    private val BUSY_CLEAR_REFUSAL_HEADER_REGEX = Regex(
        """session_clear refused for (.+?) because pane (\S+) is (?:alive-busy|active_agent_doc|busy)""",
        RegexOption.DOT_MATCHES_ALL,
    )
    private val PROTECTED_CLEAR_REFUSAL_HEADER_REGEX = Regex(
        """session_clear refused for (.+?) because pane (\S+) contains protected prompt input""",
        RegexOption.DOT_MATCHES_ALL,
    )
    private val BUSY_RESTART_REFUSAL_HEADER_REGEX = Regex(
        """session_restart refused for (.+?) because pane (\S+) is alive-busy""",
        RegexOption.DOT_MATCHES_ALL,
    )
    private val STARTING_RESTART_REFUSAL_REGEX = Regex(
        """session_restart refused for (.+?) because the authoritative actor is still starting and (.+?)\. Wait for a dispatch-ready prompt""",
        RegexOption.DOT_MATCHES_ALL,
    )
    // #hj7s: a terminal editor (e.g. Claude Code `ctrl+g` edit-in-nvim) owns the
    // pane TTY, so restart is refused and the operator must close the editor.
    private val EDITOR_RESTART_REFUSAL_HEADER_REGEX = Regex(
        """session_restart refused for (.+?) because pane (\S+) is held by editor (\S+)""",
        RegexOption.DOT_MATCHES_ALL,
    )
    private val PROTECTED_CLEAR_REASON_REGEX = Regex("""reason=([^,)]+)""")
    private val BUSY_CLEAR_SOURCE_REGEX = Regex("""source=([^,)]+)""")
    private val BUSY_CLEAR_COMMAND_REGEX = Regex("""current_command=([^,)]+)""")
    private val OPS_LOG_FILE_REGEX = Regex("""\bfile=(\S+)""")
    private val OPS_LOG_PANE_REGEX = Regex("""\bpane=(\S+)""")
    private val OPS_LOG_STATE_REGEX = Regex("""\bstate=(\S+)""")
    private val OPS_LOG_CURRENT_COMMAND_REGEX = Regex("""\bcurrent_command=(\S+)""")
    private val ROUTE_EDITOR_ATTEMPT_REGEX = Regex("""\beditor_attempt_id=(\S+)""")
    private val ROUTE_SNAPSHOT_PATH_REGEX = Regex("""\bsnapshot_path=(\S+)""")
    private val RESTART_TELEMETRY_EVENT_NAMES = listOf(
        "session_restart_force_used",
        "session_restart_busy_pre_interrupt_idle",
        "session_restart_busy_force_killed",
    )

    internal interface InFlightRouteHandle {
        fun isAlive(): Boolean
        fun cancelForReplacement()
        fun wasCanceled(): Boolean
    }

    internal class ProcessRouteHandle(private val process: Process) : InFlightRouteHandle {
        @Volatile
        private var canceled = false

        override fun isAlive(): Boolean = process.isAlive

        override fun cancelForReplacement() {
            canceled = true
            process.destroy()
            if (!process.waitFor(200, TimeUnit.MILLISECONDS)) {
                process.destroyForcibly()
            }
        }

        override fun wasCanceled(): Boolean = canceled
    }

    internal class RetryingRouteHandle : InFlightRouteHandle {
        @Volatile
        private var activeProcess: Process? = null
        @Volatile
        private var canceled = false
        @Volatile
        private var completed = false

        fun bind(process: Process) {
            if (canceled) {
                process.destroy()
                if (!process.waitFor(200, TimeUnit.MILLISECONDS)) {
                    process.destroyForcibly()
                }
                return
            }
            activeProcess = process
        }

        fun markCompleted() {
            completed = true
            activeProcess = null
        }

        override fun isAlive(): Boolean = !completed && !canceled

        override fun cancelForReplacement() {
            canceled = true
            activeProcess?.let { process ->
                process.destroy()
                if (!process.waitFor(200, TimeUnit.MILLISECONDS)) {
                    process.destroyForcibly()
                }
            }
        }

        override fun wasCanceled(): Boolean = canceled
    }

    internal class InFlightRouteRegistry {
        private val active = mutableMapOf<String, InFlightRouteHandle>()

        @Synchronized
        fun startIfIdle(key: String, next: InFlightRouteHandle): Boolean {
            val previous = active[key]
            if (previous != null && previous.isAlive()) {
                return false
            }
            active[key] = next
            return true
        }

        @Synchronized
        fun replace(key: String, next: InFlightRouteHandle): Boolean {
            val previous = active.put(key, next)
            val replaced = previous?.takeIf { it.isAlive() }
            replaced?.cancelForReplacement()
            return replaced != null
        }

        @Synchronized
        fun clearIfCurrent(key: String, current: InFlightRouteHandle) {
            if (active[key] === current) {
                active.remove(key)
            }
        }
    }

    internal data class BusySessionClearRefusal(
        val file: String,
        val pane: String,
        val source: String,
        val currentCommand: String,
        val tail: String,
        val protectedReason: String = "",
    )

    internal data class BusySessionRestartRefusal(
        val file: String,
        val pane: String,
        val source: String,
        val currentCommand: String,
        val tail: String,
    )

    internal data class StartingSessionRestartRefusal(
        val file: String,
        val reason: String,
    )

    internal data class EditorHoldsPaneRestartRefusal(
        val file: String,
        val pane: String,
        val editor: String,
        val source: String,
        val currentCommand: String,
        val tail: String,
    )

    internal data class RestartSupervisorTelemetry(
        val forceUsed: Boolean,
        val busyPreInterruptIdle: Boolean,
        val busyForceKilled: Boolean,
        val pane: String,
        val state: String,
        val currentCommand: String,
        val eventNames: List<String>,
    )

    internal enum class RunAgentDocRouteFailureKind {
        PERSISTENT,
        RETRYABLE_STARTING,
        BUSY_RUNNING,
        QUEUED_PENDING,
        DISPATCH_START_UNPROVEN,
    }

    internal val inFlightRouteRegistry = InFlightRouteRegistry()
    internal val editorCommandRegistry = EditorCommandRegistry()

    private data class PendingRunAfterClear(
        val project: Project,
        val file: VirtualFile,
        val onComplete: (() -> Unit)?,
        val attempt: RunAgentDocAttemptLedger.Attempt?,
    )

    private val pendingRunAfterClear = mutableMapOf<String, PendingRunAfterClear>()

    fun relativePath(project: Project, file: VirtualFile): String {
        val basePath = project.basePath ?: return file.path
        return if (file.path.startsWith(basePath)) {
            file.path.removePrefix("$basePath/")
        } else {
            file.path
        }
    }

    /**
     * Resolve the agent-doc project root for [file].
     *
     * Walks up from the file's parent looking for the nearest ancestor with
     * `.agent-doc/` (via the shared FFI helper). If the file lives inside a
     * submodule that is itself an agent-doc project (e.g. `src/session-share/`),
     * the submodule root is returned. Otherwise falls back to the IDE project's
     * `basePath`.
     *
     * Returns `(projectRoot, relativePath)` where `relativePath` is `file.path`
     * relative to `projectRoot`, suitable for passing to `agent-doc` commands
     * run from that directory.
     */
    fun resolveProject(project: Project, file: VirtualFile): Pair<String, String> {
        val basePath = project.basePath
        val ffi = NativePatching.resolveProjectPath(file.path)
        if (ffi != null) {
            // Register resolved root with PatchWatcher on-demand. This handles submodule
            // roots that weren't present at startup (e.g. user opens a file in a freshly
            // cloned submodule). Idempotent — no-op if already registered.
            if (basePath != null && ffi.first != basePath) {
                try {
                    PatchWatcher.getInstance(project).registerRoot(ffi.first)
                } catch (_: Exception) { /* best-effort */ }
            }
            return ffi
        }
        // FFI unavailable or no `.agent-doc/` ancestor — fall back to workspace basePath.
        if (basePath != null) {
            return Pair(basePath, relativePath(project, file))
        }
        return Pair(java.io.File(file.path).parent ?: "/", java.io.File(file.path).name)
    }

    /**
     * Routes a document trigger command via `agent-doc route --dispatch-only --plain-trigger`.
     *
     * This calls `agent-doc route --dispatch-only --plain-trigger <path>` which:
     * 1. Reads the session UUID from the file's frontmatter
     * 2. Looks up the tmux pane for that session
     * 3. Resolves the active harness trigger and sends the bare reopen through the
     *    owning supervisor into the live session
     * 4. Auto-starts a new agent session if needed
     */
    private fun rememberRunAfterClear(routeKey: String, pending: PendingRunAfterClear): PendingRunAfterClear? =
        synchronized(pendingRunAfterClear) {
            pendingRunAfterClear.put(routeKey, pending)
        }

    private fun takeRunAfterClear(routeKey: String): PendingRunAfterClear? =
        synchronized(pendingRunAfterClear) {
            pendingRunAfterClear.remove(routeKey)
        }

    private fun completeClearCommand(routeKey: String, clearSucceeded: Boolean) {
        when (editorCommandRegistry.complete(routeKey, EditorCommandKind.CLEAR_SESSION_CONTEXT)) {
            EditorCommandCompletion.START_QUEUED_RUN -> {
                val pending = takeRunAfterClear(routeKey)
                if (clearSucceeded && pending != null) {
                    LOG.warn("[state] starting queued Run Agent Doc after Clear Session Context for ${pending.file.name}")
                    pending.attempt?.recordIfCurrent("route_start_after_clear")
                    sendToTerminal(
                        pending.project,
                        pending.file,
                        onComplete = pending.onComplete,
                        attempt = pending.attempt,
                        commandPreAcquired = true,
                    )
                } else {
                    if (pending != null) {
                        pending.attempt?.finishIfCurrent(
                            "route_clear_failed_before_dispatch",
                            error = "Clear Session Context did not complete synchronously",
                        )
                        pending.onComplete?.invoke()
                    }
                    editorCommandRegistry.complete(routeKey, EditorCommandKind.RUN_AGENT_DOC)
                }
            }
            EditorCommandCompletion.IDLE,
            EditorCommandCompletion.IGNORED -> Unit
        }
    }

    internal fun sendToTerminal(
        project: Project,
        file: VirtualFile,
        onComplete: (() -> Unit)? = null,
        attempt: RunAgentDocAttemptLedger.Attempt? = null,
        commandPreAcquired: Boolean = false,
    ) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        val routeKey = RunAgentDocAttemptLedger.routeKey(cwd, relativePath)

        LOG.warn("[route] sendToTerminal: cwd=$cwd rel=$relativePath binary=$agentDoc")
        attempt?.recordIfCurrent("route_prepare")

        if (!commandPreAcquired) {
            when (editorCommandRegistry.request(routeKey, EditorCommandKind.RUN_AGENT_DOC)) {
                EditorCommandDecision.START_NOW -> Unit
                EditorCommandDecision.DEDUPE_ACTIVE_RUN -> {
                    LOG.warn("[state] Run Agent Doc already dispatching for $relativePath; keeping existing route alive")
                    attempt?.finishIfCurrent(
                        "route_already_in_flight",
                        error = "existing Run Agent Doc route is still dispatching",
                    )
                    showHint(project, "Run Agent Doc is already dispatching for $relativePath")
                    onComplete?.invoke()
                    return
                }
                EditorCommandDecision.QUEUE_RUN_AFTER_CLEAR -> {
                    val replaced = rememberRunAfterClear(
                        routeKey,
                        PendingRunAfterClear(project, file, onComplete, attempt),
                    )
                    replaced?.attempt?.finishIfCurrent(
                        "route_queued_after_clear_superseded",
                        error = "newer Run Agent Doc queued behind Clear Session Context",
                    )
                    replaced?.onComplete?.invoke()
                    attempt?.recordIfCurrent("route_queued_after_clear")
                    showHint(project, "Run Agent Doc will start after Clear Session Context finishes for $relativePath")
                    return
                }
                EditorCommandDecision.DEDUPE_ACTIVE_CLEAR -> {
                    attempt?.finishIfCurrent(
                        "route_blocked_by_clear",
                        error = "Clear Session Context is already running",
                    )
                    onComplete?.invoke()
                    return
                }
                EditorCommandDecision.REJECT_CLEAR_DURING_RUN,
                EditorCommandDecision.IGNORED -> {
                    attempt?.finishIfCurrent(
                        "route_state_rejected",
                        error = "unexpected editor command state",
                    )
                    onComplete?.invoke()
                    return
                }
            }
        }

        // is_busy guard removed: no production code sets the status signals,
        // so the guard only produced false positives (blocked every route attempt)

        try {
            // Build route command with optional layout args
            val cmd = buildRunRouteCommand(agentDoc, relativePath)

            val manager = com.intellij.openapi.fileEditor.FileEditorManager.getInstance(project)
            val visibleMdFiles = SyncLayoutAction.collectVisibleMarkdownFiles(manager.selectedFiles)
            val editorLayout = SyncLayoutAction.absolutizeEditorLayout(
                cwd,
                SyncLayoutAction.normalizeEditorLayout(
                    project.basePath,
                    cwd,
                    LayoutDetector.detectEditorLayout(project),
                ),
            )
            cmd.addAll(
                buildRouteLayoutArgs(
                    visibleMdFiles = visibleMdFiles,
                    editorLayout = editorLayout,
                    focusedFile = manager.selectedTextEditor?.virtualFile
                        ?.takeIf { it.name.endsWith(".md") }
                        ?.path,
                )
            )

            // Pass focused file
            LOG.warn("[route] executing: ${cmd.joinToString(" ")}")
            attempt?.recordIfCurrent("route_command_built", command = cmd)

            val handle = RetryingRouteHandle()
            if (!inFlightRouteRegistry.startIfIdle(routeKey, handle)) {
                LOG.warn("[state] route process already alive for $relativePath; suppressing duplicate Run Agent Doc")
                attempt?.finishIfCurrent(
                    "route_process_already_in_flight",
                    command = cmd,
                    error = "existing route process is still alive",
                )
                showHint(project, "Run Agent Doc is already dispatching for $relativePath")
                onComplete?.invoke()
                return
            }

            val startedAt = System.currentTimeMillis()
            Thread {
                var finalStage: String? = null
                var finalError: String? = null
                try {
                    var routeAttempt = 1
                    while (!handle.wasCanceled() && routeAttempt <= STARTING_ACTOR_ROUTE_MAX_ATTEMPTS) {
                        attempt?.recordIfCurrent("route_start", command = cmd)
                        val process = ProcessBuilder(cmd)
                            .directory(java.io.File(cwd))
                            .redirectErrorStream(true)
                            .apply {
                                attempt?.let {
                                    environment()["AGENT_DOC_EDITOR_ROUTE_ATTEMPT_ID"] = it.id
                                    environment()["AGENT_DOC_EDITOR_ROUTE_KEY"] = it.routeKey
                                }
                            }
                            .start()
                        handle.bind(process)
                        val output = process.inputStream.bufferedReader().readText()
                        val exitCode = process.waitFor()
                        val elapsed = formatElapsedMillis(System.currentTimeMillis() - startedAt)
                        val failureKind = classifyRunAgentDocRouteFailure(output)
                        if (handle.wasCanceled()) {
                            LOG.warn("[route] superseded route exited after replacement for $relativePath")
                            finalStage = "route_superseded"
                            finalError = "route handle was canceled"
                            break
                        } else if (
                            exitCode != 0 &&
                            isRetryableRunAgentDocRouteFailure(failureKind) &&
                            routeAttempt < STARTING_ACTOR_ROUTE_MAX_ATTEMPTS
                        ) {
                            attempt?.recordIfCurrent(
                                "route_retryable_starting",
                                command = cmd,
                                error = routeAttemptError(exitCode, failureKind, output),
                            )
                            val delayMillis = startingActorRouteRetryDelayMillis(routeAttempt)
                            LOG.warn(
                                "[route] actor still starting for $relativePath; retrying attempt ${routeAttempt + 1}/$STARTING_ACTOR_ROUTE_MAX_ATTEMPTS after ${delayMillis}ms"
                            )
                            if (!sleepBeforeRetry(handle, delayMillis)) {
                                LOG.warn("[route] superseded starting-actor retry for $relativePath")
                                finalStage = "route_superseded"
                                finalError = "superseded during starting-actor retry"
                                break
                            }
                            routeAttempt += 1
                            continue
                        } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.BUSY_RUNNING) {
                            LOG.warn("[route] busy/running after retry budget for $relativePath: $output")
                            clearPersistedRouteFailureOutput(cwd, relativePath)
                            notifyRunAgentDocStillRunning(project, relativePath, output)
                            finalStage = "route_busy_running"
                            finalError = routeAttemptError(exitCode, failureKind, output)
                            break
                        } else if (failureKind == RunAgentDocRouteFailureKind.QUEUED_PENDING) {
                            LOG.warn("[route] queued behind active turn for $relativePath: $output")
                            clearPersistedRouteFailureOutput(cwd, relativePath)
                            notifyRunAgentDocQueued(project, relativePath, output)
                            finalStage = "route_queued_pending"
                            finalError = routeAttemptError(exitCode, failureKind, output)
                            break
                        } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.DISPATCH_START_UNPROVEN) {
                            LOG.warn("[route] dispatch start unproven for $relativePath: $output")
                            clearPersistedRouteFailureOutput(cwd, relativePath)
                            notifyRunAgentDocDispatchUnproven(project, relativePath, output)
                            finalStage = "route_dispatch_start_unproven"
                            finalError = routeAttemptError(exitCode, failureKind, output)
                            break
                        } else if (exitCode != 0) {
                            LOG.warn("[route] FAILED (exit $exitCode): $output")
                            notifyPersistentRouteFailure(
                                project = project,
                                cwd = cwd,
                                relativePath = relativePath,
                                exitCode = exitCode,
                                elapsed = elapsed,
                                routeOutput = output,
                            )
                            finalStage = "route_failed"
                            finalError = routeAttemptError(exitCode, failureKind, output)
                            break
                        } else {
                            LOG.warn("[route] SUCCESS: $output")
                            clearPersistedRouteFailureOutput(cwd, relativePath)
                            finalStage = "route_success"
                            break
                        }
                    }
                } catch (e: Exception) {
                    finalStage = "route_exception"
                    finalError = e.message ?: e.javaClass.simpleName
                    throw e
                } finally {
                    finalStage?.let { stage ->
                        attempt?.finishIfCurrent(stage, command = cmd, error = finalError)
                    }
                    handle.markCompleted()
                    inFlightRouteRegistry.clearIfCurrent(routeKey, handle)
                    editorCommandRegistry.complete(routeKey, EditorCommandKind.RUN_AGENT_DOC)
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            editorCommandRegistry.complete(routeKey, EditorCommandKind.RUN_AGENT_DOC)
            attempt?.finishIfCurrent("route_exception", error = e.message ?: e.javaClass.simpleName)
            onComplete?.invoke()
            notifyError(project, "Failed to run agent-doc: ${e.message}\nLooked for: $agentDoc")
        }
    }

    private fun routeAttemptError(
        exitCode: Int,
        failureKind: RunAgentDocRouteFailureKind,
        output: String,
    ): String {
        val compact = output.replace("\r", "\\r").replace("\n", "\\n").take(2000)
        return "exit=$exitCode failure=$failureKind output=$compact"
    }

    internal fun buildRunRouteCommand(agentDoc: String, relativePath: String): MutableList<String> =
        mutableListOf(
            agentDoc,
            "route",
            "--dispatch-only",
            "--plain-trigger",
            // #run-agent-doc-latency: the plugin already awaited typing idle via the
            // FFI typing tracker (TypingTracker.awaitIdle) before saving and routing,
            // so route's own mtime debounce is pure redundant latency on the editor
            // path. Pass --debounce 0 to skip it; the binary still treats the
            // cross-process typing indicator as authoritative for non-editor callers.
            "--debounce",
            "0",
            "--wait-for-ready",
            "60",
            relativePath,
        )

    internal fun buildRouteLayoutArgs(
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout?,
        focusedFile: String?,
    ): List<String> {
        val args = mutableListOf<String>()
        if (editorLayout != null && editorLayout.columns.size > 1) {
            for (col in editorLayout.columns) {
                args.addAll(listOf("--col", col.files.joinToString(",")))
            }
        } else if (visibleMdFiles.isNotEmpty()) {
            args.addAll(listOf("--col", visibleMdFiles.joinToString(",")))
        }
        if (focusedFile != null) {
            args.addAll(listOf("--focus", focusedFile))
        }
        return args
    }

    /**
     * Runs `agent-doc fix <path>` for the active markdown document.
     *
     * This is the editor-side recovery path for a document whose tmux/session
     * ownership metadata or live pane state needs deterministic repair before
     * another routed reopen is attempted.
     */
    fun fixDocument(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        try {
            val process = ProcessBuilder(agentDoc, "fix", relativePath)
                .directory(java.io.File(cwd))
                .redirectErrorStream(true)
                .start()

            showHint(project, "Fixing $relativePath")

            Thread {
                try {
                    val output = process.inputStream.bufferedReader().readText()
                    val exitCode = process.waitFor()
                    if (exitCode != 0) {
                        notifyError(project, "agent-doc fix failed (exit $exitCode):\n$output")
                    } else {
                        showHint(project, output.trim().ifEmpty { "Fixed $relativePath" })
                    }
                } finally {
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            onComplete?.invoke()
            notifyError(project, "Failed to run agent-doc fix: ${e.message}\nLooked for: $agentDoc")
        }
    }

    /**
     * Runs a document session via `agent-doc run --agent <agent>`.
     *
     * This calls `agent-doc run --agent <agent> <path>` which:
     * 1. Computes the diff for the document
     * 2. Builds a prompt for the agent
     * 3. Sends the prompt to the specified agent backend
     * 4. Updates the document with the response
     */
    fun runWithAgent(project: Project, agent: String, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        try {
            val process = ProcessBuilder(agentDoc, "run", "--agent", agent, relativePath)
                .directory(java.io.File(cwd))
                .redirectErrorStream(true)
                .start()

            // Show quick inline hint near cursor
            showHint(project, "Running with $agent: $relativePath")

            // Read output in background thread to avoid blocking EDT
            Thread {
                try {
                    val output = process.inputStream.bufferedReader().readText()
                    val exitCode = process.waitFor()
                    if (exitCode != 0) {
                        notifyError(project, "agent-doc run failed (exit $exitCode):\n$output")
                    } else {
                        // Notify success and expire quickly
                        notifyInfo(project, "Agent $agent finished: $relativePath")
                    }

                    // For Junie agent, open the request file in the editor so the user (or Junie agent) sees the diff
                    if (agent == "junie") {
                        openJunieRequest(project)
                    }
                } finally {
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            onComplete?.invoke()
            notifyError(project, "Failed to run agent-doc: ${e.message}\nLooked for: $agentDoc")
        }
    }

    fun showSessionStatus(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, _) = resolveProject(project, file)
        runSessionCommand(
            project = project,
            file = file,
            args = listOf("status"),
            startedMessage = "Loading session status for ${file.name}",
            onSuccess = { relativePath, output ->
                clearPersistedRouteFailureOutput(cwd, relativePath)
                notifyInfo(project, sessionStatusSuccessMessage(relativePath, output))
            },
            onComplete = onComplete,
        )
    }

    fun restartSession(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        runRestartSupervisorCommand(project, file, force = false, onComplete = onComplete)
    }

    /**
     * #plugin-cleanup-menu-command: resolve the project root for a project-level
     * cleanup command. These commands operate on the whole session registry, not
     * a single document, so they run in the focused .md file's project root when
     * one is open, else the IDE project base path.
     */
    internal fun cleanupProjectRoot(project: Project): String {
        val manager = FileEditorManager.getInstance(project)
        val focused = manager.selectedTextEditor?.virtualFile?.takeIf { it.name.endsWith(".md") }
            ?: manager.selectedFiles.firstOrNull { it.name.endsWith(".md") }
        if (focused != null) {
            return resolveProject(project, focused).first
        }
        return project.basePath ?: "."
    }

    /**
     * #plugin-cleanup-menu-command: thin wrapper that shells a project-level
     * `agent-doc` cleanup command (e.g. `resync --fix`, `gc`) in the project root
     * and surfaces the result as an Event-Log notification. No cleanup logic lives
     * in the plugin — the CLI owns it; this only reports the outcome.
     */
    internal fun runProjectCleanupCommand(project: Project, label: String, args: List<String>) {
        val projectRoot = cleanupProjectRoot(project)
        val agentDoc = resolveAgentDoc(projectRoot)
        showHint(project, "$label: running agent-doc ${args.joinToString(" ")}…")
        Thread {
            try {
                val cmd = listOf(agentDoc) + args
                val result = SyncLayoutAction.runCommandWithTimeout(cmd, projectRoot)
                LOG.info("[cleanup] $label exit=${result.exitCode} cmd=${cmd.joinToString(" ")}")
                if (result.exitCode != 0) {
                    val reason = if (result.timedOut) "timed out" else "failed (exit ${result.exitCode})"
                    notifyError(project, "$label $reason:\n${result.output}")
                } else {
                    notifyInfo(project, "$label complete.\n${result.output.ifBlank { "No changes." }}")
                }
            } catch (e: Exception) {
                notifyError(project, "$label failed: ${e.message}")
            }
        }.start()
    }

    fun resyncFixSessions(project: Project) =
        runProjectCleanupCommand(project, "Resync / Fix Sessions", listOf("resync", "--fix"))

    fun gcStaleSessions(project: Project) =
        runProjectCleanupCommand(project, "GC Stale Sessions", listOf("gc"))

    private fun runRestartSupervisorCommand(
        project: Project,
        file: VirtualFile,
        force: Boolean,
        onComplete: (() -> Unit)? = null,
    ) {
        val (telemetryCwd, _) = resolveProject(project, file)
        val telemetryStartLine = restartTelemetryOpsLogLineCount(telemetryCwd)
        runSessionCommand(
            project = project,
            file = file,
            args = if (force) listOf("restart-supervisor", "--force") else listOf("restart-supervisor"),
            startedMessage = "Restarting supervisor for ${file.name}",
            onSuccess = { relativePath, output ->
                val telemetry = readRestartSupervisorTelemetry(telemetryCwd, relativePath, telemetryStartLine)
                val message = restartSessionSuccessMessage(relativePath, output, telemetry)
                if (telemetry != null) {
                    LOG.info("[session_restart] $relativePath ${telemetry.eventNames.joinToString(",")} pane=${telemetry.pane}")
                    notifyInfo(project, message)
                } else {
                    showHint(project, message)
                }
            },
            onFailure = { relativePath, exitCode, output ->
                // #hj7s: an editor holding the pane refuses for BOTH Restart and
                // --force Interrupt-and-Restart, so this handler runs for force too
                // (the force path previously had no onFailure and failed silently).
                val editorRefusal = parseEditorHoldsPaneRestartRefusal(output)
                if (editorRefusal != null) {
                    notifyEditorHoldsPaneRestartBlocked(project, file, relativePath, editorRefusal, output)
                } else if (force) {
                    notifyError(project, "agent-doc command failed (exit $exitCode):\n$output")
                } else {
                    val busyRefusal = parseBusySessionRestartRefusal(output)
                    if (busyRefusal != null) {
                        notifyBusySessionRestartBlocked(project, file, relativePath, busyRefusal, output)
                    } else {
                        val startingRefusal = parseStartingSessionRestartRefusal(output)
                        if (startingRefusal != null) {
                            notifyStartingSessionRestartBlocked(project, file, relativePath, startingRefusal, output)
                        } else {
                            notifyError(project, "agent-doc command failed (exit $exitCode):\n$output")
                        }
                    }
                }
            },
            onComplete = onComplete,
        )
    }

    fun interruptAndRestartSession(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        ApplicationManager.getApplication().invokeLater {
            val decision = Messages.showYesNoDialog(
                project,
                "Interrupt the running agent-doc turn and restart its supervisor? Unsaved work in the terminal session may be discarded.",
                "Interrupt and Restart Supervisor",
                "Interrupt and restart",
                "Cancel",
                Messages.getWarningIcon(),
            )
            if (decision != Messages.YES) {
                onComplete?.invoke()
                return@invokeLater
            }
            runRestartSupervisorCommand(project, file, force = true, onComplete = onComplete)
        }
    }

    fun compactExchange(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, relativePath) = resolveProject(project, file)
        runDocumentCommand(
            project = project,
            file = file,
            command = buildCompactExchangeCommand(resolveAgentDoc(cwd), relativePath),
            startedMessage = "Compacting exchange for ${file.name}",
            onSuccess = { resolvedPath, output ->
                showHint(project, output.ifBlank { "Compacted exchange for $resolvedPath" })
            },
            onComplete = onComplete,
        )
    }

    fun clearSessionContext(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, relativePath) = resolveProject(project, file)
        val routeKey = RunAgentDocAttemptLedger.routeKey(cwd, relativePath)
        when (editorCommandRegistry.request(routeKey, EditorCommandKind.CLEAR_SESSION_CONTEXT)) {
            EditorCommandDecision.START_NOW -> Unit
            EditorCommandDecision.DEDUPE_ACTIVE_CLEAR -> {
                LOG.warn("[state] Clear Session Context already running for $relativePath")
                showHint(project, "Clear Session Context is already running for $relativePath")
                onComplete?.invoke()
                return
            }
            EditorCommandDecision.REJECT_CLEAR_DURING_RUN -> {
                LOG.warn("[state] Clear Session Context blocked while Run Agent Doc is dispatching for $relativePath")
                notifyWarning(
                    project,
                    "Clear Session Context is blocked while Run Agent Doc is dispatching for $relativePath.\n" +
                        "Wait for dispatch to finish, or use Interrupt and Clear Session Context.",
                )
                onComplete?.invoke()
                return
            }
            EditorCommandDecision.DEDUPE_ACTIVE_RUN,
            EditorCommandDecision.QUEUE_RUN_AFTER_CLEAR,
            EditorCommandDecision.IGNORED -> {
                onComplete?.invoke()
                return
            }
        }
        var clearFinishedNow = false
        runSessionCommand(
            project = project,
            file = file,
            args = listOf("clear"),
            startedMessage = "Clearing session context for ${file.name}",
            onSuccess = { clearRelativePath, output ->
                // #autoloop-command-preemption Phase 2b: a non-interrupting clear
                // against a busy auto-loop now succeeds by DEFERRING (pause the
                // loop, deliver the clear at the next idle gap, resume) instead of
                // hard-blocking. Surface that as a distinct "deferred" notice so
                // the operator knows it will run shortly, not that it ran now.
                val deferredClear = isDeferredQueuePreemptClear(output)
                clearFinishedNow = !deferredClear
                if (deferredClear) {
                    notifyInfo(
                        project,
                        "Clear Session Context deferred for $clearRelativePath.\n" +
                            "The pane is busy under an active queue auto-loop, so the loop was paused " +
                            "and the clear will run automatically at the next idle gap, then the loop resumes. " +
                            "Use Interrupt and clear to clear immediately instead.",
                    )
                } else {
                    showHint(project, output.ifBlank { "Cleared session context for $clearRelativePath" })
                }
            },
            onFailure = { clearRelativePath, exitCode, output ->
                val busyRefusal = parseBusySessionClearRefusal(output)
                if (busyRefusal != null) {
                    notifyBusySessionClearBlocked(project, file, clearRelativePath, busyRefusal, output)
                } else {
                    notifyError(
                        project,
                        "agent-doc command failed (exit $exitCode):\n$output",
                    )
                }
            },
            onComplete = {
                completeClearCommand(routeKey, clearFinishedNow)
                onComplete?.invoke()
            },
        )
    }

    fun refreshAndRetryClearSessionContext(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        runSessionCommand(
            project = project,
            file = file,
            args = listOf("status"),
            startedMessage = "Refreshing session status for ${file.name}",
            onSuccess = { relativePath, output ->
                val (cwd, _) = resolveProject(project, file)
                clearPersistedRouteFailureOutput(cwd, relativePath)
                if (sessionStatusShowsIdleDirectPane(output)) {
                    clearSessionContext(project, file, onComplete)
                } else {
                    notifyInfo(project, sessionStatusSuccessMessage(relativePath, output))
                    onComplete?.invoke()
                }
            },
            onFailure = { _, exitCode, output ->
                notifyError(project, "agent-doc command failed (exit $exitCode):\n$output")
                onComplete?.invoke()
            },
        )
    }

    fun interruptAndClearSessionContext(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        ApplicationManager.getApplication().invokeLater {
            val decision = Messages.showYesNoDialog(
                project,
                "Interrupt the running agent-doc turn and clear its session context? Unsaved work in the terminal session may be discarded.",
                "Interrupt and Clear Session Context",
                "Interrupt and clear",
                "Cancel",
                Messages.getWarningIcon(),
            )
            if (decision != Messages.YES) {
                onComplete?.invoke()
                return@invokeLater
            }
            runSessionCommand(
                project = project,
                file = file,
                args = listOf("interrupt-clear"),
                startedMessage = "Interrupting and clearing session context for ${file.name}",
                onSuccess = { relativePath, output ->
                    showHint(project, output.ifBlank { "Interrupted and cleared session context for $relativePath" })
                },
                onComplete = onComplete,
            )
        }
    }

    fun copySessionDiagnostics(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        runSessionCommand(
            project = project,
            file = file,
            args = listOf("doctor"),
            startedMessage = "Collecting session diagnostics for ${file.name}",
            onSuccess = { relativePath, output ->
                CopyPasteManager.getInstance().setContents(StringSelection(output))
                showHint(project, "Copied session diagnostics for $relativePath")
            },
            onComplete = onComplete,
        )
    }

    private fun runSessionCommand(
        project: Project,
        file: VirtualFile,
        args: List<String>,
        startedMessage: String,
        onSuccess: (String, String) -> Unit,
        onFailure: ((String, Int, String) -> Unit)? = null,
        onComplete: (() -> Unit)? = null,
    ) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        val cmd = buildSessionCommand(agentDoc, args, relativePath)
        runDocumentCommand(
            project = project,
            file = file,
            command = cmd,
            startedMessage = startedMessage,
            onSuccess = onSuccess,
            onFailure = onFailure,
            onComplete = onComplete,
        )
    }

    private fun runDocumentCommand(
        project: Project,
        file: VirtualFile,
        command: List<String>,
        startedMessage: String,
        onSuccess: (String, String) -> Unit,
        onFailure: ((String, Int, String) -> Unit)? = null,
        onComplete: (() -> Unit)? = null,
    ) {
        val (cwd, relativePath) = resolveProject(project, file)
        try {
            val process = ProcessBuilder(command)
                .directory(java.io.File(cwd))
                .redirectErrorStream(true)
                .start()

            showHint(project, startedMessage)

            Thread {
                try {
                    val output = process.inputStream.bufferedReader().readText().trim()
                    val exitCode = process.waitFor()
                    if (exitCode != 0) {
                        onFailure?.invoke(relativePath, exitCode, output)
                            ?: notifyError(
                                project,
                                "agent-doc command failed (exit $exitCode):\n$output",
                            )
                    } else {
                        onSuccess(relativePath, output)
                    }
                } finally {
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            onComplete?.invoke()
            val binary = command.firstOrNull() ?: "agent-doc"
            notifyError(project, "Failed to run agent-doc command: ${e.message}\nLooked for: $binary")
        }
    }

    fun resolveAgentDoc(basePath: String? = null): String {
        val candidates = listOfNotNull(
            basePath?.let { "$it/.bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/.local/bin/agent-doc" },
            System.getenv("HOME")?.let { "$it/.cargo/bin/agent-doc" },
            "/usr/local/bin/agent-doc"
        )
        for (path in candidates) {
            if (java.io.File(path).canExecute()) {
                return path
            }
        }
        return "agent-doc"
    }

    internal fun buildSessionCommand(
        agentDoc: String,
        args: List<String>,
        relativePath: String,
    ): List<String> = buildList {
        add(agentDoc)
        add("session")
        addAll(args)
        add(relativePath)
    }

    internal fun buildCompactExchangeCommand(
        agentDoc: String,
        relativePath: String,
    ): List<String> = listOf(
        agentDoc,
        "compact",
        relativePath,
        "--component",
        "exchange",
        "--commit",
    )

    internal fun sessionStatusSuccessMessage(relativePath: String, output: String): String =
        output.ifBlank { "Loaded session status for $relativePath" }

    internal fun sessionStatusShowsIdleDirectPane(output: String): Boolean =
        output.lineSequence().any { line ->
            line.startsWith("live_pane:") &&
                line.contains("state=alive-idle") &&
                line.contains("prompt_ready=true")
        }

    internal fun restartSessionSuccessMessage(
        relativePath: String,
        output: String,
        telemetry: RestartSupervisorTelemetry?,
    ): String {
        val base = output.ifBlank { "Restart requested for supervisor handling $relativePath" }
        return if (telemetry == null) {
            base
        } else {
            "$base\n${buildRestartSupervisorTelemetryMessage(telemetry)}"
        }
    }

    internal fun buildRestartSupervisorTelemetryMessage(telemetry: RestartSupervisorTelemetry): String = buildString {
        append("Recovery path: ")
        when {
            telemetry.busyForceKilled -> append("forced restart interrupted a busy pane and restarted the supervisor")
            telemetry.busyPreInterruptIdle -> append("forced restart interrupted a busy pane that reached idle before restart")
            telemetry.forceUsed -> append("forced restart was used")
            else -> append("restart completed")
        }
        if (telemetry.pane.isNotBlank() && telemetry.pane != "unknown") {
            append(" (pane ")
            append(telemetry.pane)
            append(")")
        }
        append(".")
        if (telemetry.currentCommand.isNotBlank() && telemetry.currentCommand != "unknown") {
            append("\nInterrupted command: ")
            append(telemetry.currentCommand)
        }
        if (telemetry.eventNames.isNotEmpty()) {
            append("\nEvents: ")
            append(telemetry.eventNames.joinToString(", "))
        }
    }

    /// True when `agent-doc session clear` succeeded by deferring against a busy
    /// auto-loop (`#autoloop-command-preemption` Phase 2b). The binary prints
    /// `session_clear deferred for ...` to stderr on that path.
    internal fun isDeferredQueuePreemptClear(output: String): Boolean =
        output.contains("session_clear deferred for")

    internal fun parseBusySessionClearRefusal(output: String): BusySessionClearRefusal? {
        val protectedMatch = PROTECTED_CLEAR_REFUSAL_HEADER_REGEX.find(output)
        if (protectedMatch != null) {
            val detail = output.substring(protectedMatch.range.last + 1)
            val source = BUSY_CLEAR_SOURCE_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
            val currentCommand = BUSY_CLEAR_COMMAND_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
            val reason = PROTECTED_CLEAR_REASON_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
            return BusySessionClearRefusal(
                file = protectedMatch.groupValues[1],
                pane = protectedMatch.groupValues[2],
                source = source.ifBlank { "unknown" },
                currentCommand = currentCommand.ifBlank { "unknown" },
                tail = extractBusyClearTail(detail),
                protectedReason = reason.ifBlank { "protected prompt input" },
            )
        }
        val match = BUSY_CLEAR_REFUSAL_HEADER_REGEX.find(output) ?: return null
        val detail = output.substring(match.range.last + 1)
        val source = BUSY_CLEAR_SOURCE_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
        val currentCommand = BUSY_CLEAR_COMMAND_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
        return BusySessionClearRefusal(
            file = match.groupValues[1],
            pane = match.groupValues[2],
            source = source.ifBlank { "unknown" },
            currentCommand = currentCommand.ifBlank { "unknown" },
            tail = extractBusyClearTail(detail),
        )
    }

    internal fun parseBusySessionRestartRefusal(output: String): BusySessionRestartRefusal? {
        val match = BUSY_RESTART_REFUSAL_HEADER_REGEX.find(output) ?: return null
        val detail = output.substring(match.range.last + 1)
        val source = BUSY_CLEAR_SOURCE_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
        val currentCommand = BUSY_CLEAR_COMMAND_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
        return BusySessionRestartRefusal(
            file = match.groupValues[1],
            pane = match.groupValues[2],
            source = source.ifBlank { "unknown" },
            currentCommand = currentCommand.ifBlank { "unknown" },
            tail = extractBusyClearTail(detail),
        )
    }

    internal fun parseStartingSessionRestartRefusal(output: String): StartingSessionRestartRefusal? {
        val match = STARTING_RESTART_REFUSAL_REGEX.find(output) ?: return null
        return StartingSessionRestartRefusal(
            file = match.groupValues[1],
            reason = match.groupValues[2].trim(),
        )
    }

    internal fun parseEditorHoldsPaneRestartRefusal(output: String): EditorHoldsPaneRestartRefusal? {
        val match = EDITOR_RESTART_REFUSAL_HEADER_REGEX.find(output) ?: return null
        val detail = output.substring(match.range.last + 1)
        val source = BUSY_CLEAR_SOURCE_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
        val currentCommand = BUSY_CLEAR_COMMAND_REGEX.find(detail)?.groupValues?.getOrNull(1).orEmpty()
        return EditorHoldsPaneRestartRefusal(
            file = match.groupValues[1],
            pane = match.groupValues[2],
            editor = match.groupValues[3],
            source = source.ifBlank { "unknown" },
            currentCommand = currentCommand.ifBlank { "unknown" },
            tail = extractBusyClearTail(detail),
        )
    }

    private fun extractBusyClearTail(detail: String): String {
        val marker = "tail="
        val start = detail.indexOf(marker)
        if (start < 0) {
            return ""
        }
        val rawTail = detail.substring(start + marker.length)
            .substringBefore("). Run `agent-doc session status")
            .substringBefore("). Run agent-doc session status")
            .substringBefore("). Clear the prompt input manually")
            .substringBefore("). Wait for an idle prompt")
            .trim()
            .removeSuffix(").")
            .removeSuffix(")")
            .trim()
        return rawTail
            .removeSurrounding("\"")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    }

    internal fun parseRestartSupervisorTelemetry(
        lines: Iterable<String>,
        cwd: String,
        relativePath: String,
    ): RestartSupervisorTelemetry? {
        val matched = lines.filter { line ->
            restartTelemetryEventName(line) != null && opsLogLineMatchesPath(line, cwd, relativePath)
        }
        if (matched.isEmpty()) {
            return null
        }

        val eventNames = mutableListOf<String>()
        for (line in matched) {
            val name = restartTelemetryEventName(line) ?: continue
            if (!eventNames.contains(name)) {
                eventNames.add(name)
            }
        }
        return RestartSupervisorTelemetry(
            forceUsed = eventNames.contains("session_restart_force_used"),
            busyPreInterruptIdle = eventNames.contains("session_restart_busy_pre_interrupt_idle"),
            busyForceKilled = eventNames.contains("session_restart_busy_force_killed"),
            pane = latestRegexValue(matched, OPS_LOG_PANE_REGEX).ifBlank { "unknown" },
            state = latestRegexValue(matched, OPS_LOG_STATE_REGEX).ifBlank { "unknown" },
            currentCommand = latestRegexValue(matched, OPS_LOG_CURRENT_COMMAND_REGEX).ifBlank { "unknown" },
            eventNames = eventNames,
        )
    }

    private fun readRestartSupervisorTelemetry(
        cwd: String,
        relativePath: String,
        startLine: Int?,
    ): RestartSupervisorTelemetry? {
        val logFile = File(cwd, ".agent-doc/logs/ops.log")
        if (!logFile.isFile) {
            return null
        }
        return try {
            val lines = logFile.readLines()
            val candidateLines = if (startLine != null && startLine <= lines.size) {
                lines.drop(startLine)
            } else {
                lines.takeLast(RESTART_TELEMETRY_OPS_LOG_MAX_LINES)
            }
            parseRestartSupervisorTelemetry(
                candidateLines.takeLast(RESTART_TELEMETRY_OPS_LOG_MAX_LINES),
                cwd,
                relativePath,
            )
        } catch (e: Exception) {
            LOG.warn("[session_restart] failed to read restart telemetry: ${e.message}")
            null
        }
    }

    private fun restartTelemetryOpsLogLineCount(cwd: String): Int? {
        val logFile = File(cwd, ".agent-doc/logs/ops.log")
        if (!logFile.isFile) {
            return 0
        }
        return try {
            logFile.useLines { it.count() }
        } catch (e: Exception) {
            LOG.warn("[session_restart] failed to count restart telemetry: ${e.message}")
            null
        }
    }

    private fun restartTelemetryEventName(line: String): String? =
        RESTART_TELEMETRY_EVENT_NAMES.firstOrNull { line.contains(it) }

    private fun opsLogLineMatchesPath(line: String, cwd: String, relativePath: String): Boolean {
        val loggedFile = OPS_LOG_FILE_REGEX.find(line)?.groupValues?.getOrNull(1) ?: return false
        val normalizedLoggedFile = normalizePath(loggedFile)
        val normalizedRelative = normalizePath(relativePath)
        if (normalizedLoggedFile == normalizedRelative) {
            return true
        }
        val absolute = normalizePath(File(cwd, relativePath).absolutePath)
        if (normalizedLoggedFile == absolute) {
            return true
        }
        val canonical = try {
            normalizePath(File(cwd, relativePath).canonicalPath)
        } catch (_: Exception) {
            absolute
        }
        return normalizedLoggedFile == canonical || normalizedLoggedFile.endsWith("/$normalizedRelative")
    }

    private fun latestRegexValue(lines: List<String>, regex: Regex): String {
        for (line in lines.asReversed()) {
            val value = regex.find(line)?.groupValues?.getOrNull(1)
            if (!value.isNullOrBlank()) {
                return value
            }
        }
        return ""
    }

    private fun normalizePath(path: String): String = path.replace('\\', '/')

    internal fun isStartingActorRouteFailure(output: String): Boolean {
        return output.contains("authoritative actor generation") &&
            output.contains("route will not inject a new trigger") &&
            output.contains("the authoritative actor is still starting")
    }

    internal fun isRetryableRunAgentDocRouteFailure(output: String): Boolean {
        return isRetryableRunAgentDocRouteFailure(classifyRunAgentDocRouteFailure(output))
    }

    private fun isRetryableRunAgentDocRouteFailure(kind: RunAgentDocRouteFailureKind): Boolean {
        // Only the actor-startup guard gets a short plugin retry. A dispatch-only
        // latest-run timeout has already spent the route ready-wait window, so
        // retrying it silently can leave Run Agent Doc looking unresponsive.
        return kind == RunAgentDocRouteFailureKind.RETRYABLE_STARTING
    }

    internal fun classifyRunAgentDocRouteFailure(output: String): RunAgentDocRouteFailureKind {
        return when {
            isRunAgentDocRouteQueued(output) -> RunAgentDocRouteFailureKind.QUEUED_PENDING
            isDispatchOnlyActiveTurnBlocked(output) -> RunAgentDocRouteFailureKind.BUSY_RUNNING
            isDispatchOnlyBusyActorWaitTimeout(output) -> RunAgentDocRouteFailureKind.BUSY_RUNNING
            isLatestRunStillBootingBusy(output) -> RunAgentDocRouteFailureKind.BUSY_RUNNING
            isDispatchStartUnproven(output) -> RunAgentDocRouteFailureKind.DISPATCH_START_UNPROVEN
            isStartingActorRouteFailure(output) ->
                RunAgentDocRouteFailureKind.RETRYABLE_STARTING
            else -> RunAgentDocRouteFailureKind.PERSISTENT
        }
    }

    private fun isDispatchStartUnproven(output: String): Boolean {
        val lower = output.lowercase()
        return lower.contains("accepted_without_dispatch_start_proof") ||
            lower.contains("route_dispatch_only_submit_unproven") ||
            (
                lower.contains("dispatch-only") &&
                    lower.contains("only pane-input acceptance proof") &&
                    lower.contains("dispatch-start proof")
            )
    }

    private fun isLatestRunStillBootingBusy(output: String): Boolean {
        val lower = output.lowercase()
        return isLatestRunStillBootingShape(lower) && lower.contains("(active codex turn)")
    }

private fun isDispatchOnlyActiveTurnBlocked(output: String): Boolean {
    val lower = output.lowercase()
    val hasActiveTurnCue = lower.contains("opencode active turn") ||
        lower.contains("active codex turn") ||
        lower.contains("active claude turn")
    return lower.contains("dispatch-only") &&
        hasActiveTurnCue &&
        (lower.contains("pane still shows") || lower.contains("pane is busy on an active"))
}

    private fun isDispatchOnlyBusyActorWaitTimeout(output: String): Boolean {
        val lower = output.lowercase()
        return lower.contains("dispatch-only route will not inject a new trigger") &&
            lower.contains("the authoritative actor is busy") &&
            lower.contains("did not return to a dispatch-ready prompt")
    }

    private fun isLatestRunStillBootingShape(lower: String): Boolean {
        return lower.contains("dispatch-only") &&
            lower.contains("latest run is still booting") &&
            lower.contains("never reached a dispatch-ready prompt")
    }

    private fun isRunAgentDocRouteQueued(output: String): Boolean {
        val lower = output.lowercase()
        return hasUserFacingOutcome(lower, UI_OUTCOME_QUEUED_BEHIND_OWNER) ||
            lower.contains("queued pending dispatch") &&
            (lower.contains("active agent:queue") || lower.contains("agent:queue auto"))
    }

    private fun hasUserFacingOutcome(lowercaseOutput: String, outcome: String): Boolean {
        return lowercaseOutput.contains("ui_outcome=$outcome")
    }

    internal fun startingActorRouteRetryDelayMillis(completedAttempts: Int): Long {
        val index = (completedAttempts - 1).coerceIn(0, STARTING_ACTOR_ROUTE_RETRY_DELAYS_MILLIS.lastIndex)
        return STARTING_ACTOR_ROUTE_RETRY_DELAYS_MILLIS[index]
    }

    private fun sleepBeforeRetry(handle: InFlightRouteHandle, delayMillis: Long): Boolean {
        var remaining = delayMillis
        while (remaining > 0) {
            if (handle.wasCanceled()) {
                return false
            }
            val chunk = minOf(remaining, 100L)
            Thread.sleep(chunk)
            remaining -= chunk
        }
        return !handle.wasCanceled()
    }

    fun showHint(project: Project, message: String) {
        ApplicationManager.getApplication().invokeLater {
            val editor = FileEditorManager.getInstance(project).selectedTextEditor ?: return@invokeLater
            HintManager.getInstance().showInformationHint(editor, message)
        }
    }

    fun notifyError(project: Project, content: String) {
        notify(project, content, NotificationType.ERROR)
    }

    fun notifyWarning(project: Project, content: String) {
        notify(project, content, NotificationType.WARNING)
    }

    private fun notifyBusySessionClearBlocked(
        project: Project,
        file: VirtualFile,
        relativePath: String,
        refusal: BusySessionClearRefusal,
        rawOutput: String,
    ) {
        val summary = buildBusySessionClearBlockedMessage(relativePath, refusal)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            if (refusal.protectedReason.isBlank()) {
                notification.addAction(NotificationAction.createSimple("Refresh and retry") {
                    refreshAndRetryClearSessionContext(project, file)
                })
            }
            notification.addAction(NotificationAction.createSimple("Interrupt and clear") {
                interruptAndClearSessionContext(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Show status") {
                showSessionStatus(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(rawOutput))
                showHint(project, "Copied busy session details for $relativePath")
            })
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    private fun notifyBusySessionRestartBlocked(
        project: Project,
        file: VirtualFile,
        relativePath: String,
        refusal: BusySessionRestartRefusal,
        rawOutput: String,
    ) {
        val summary = buildBusySessionRestartBlockedMessage(relativePath, refusal)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            notification.addAction(NotificationAction.createSimple("Interrupt and restart") {
                interruptAndRestartSession(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Show status") {
                showSessionStatus(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(rawOutput))
                showHint(project, "Copied busy restart details for $relativePath")
            })
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    private fun notifyStartingSessionRestartBlocked(
        project: Project,
        file: VirtualFile,
        relativePath: String,
        refusal: StartingSessionRestartRefusal,
        rawOutput: String,
    ) {
        val summary = buildStartingSessionRestartBlockedMessage(relativePath, refusal)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            notification.addAction(NotificationAction.createSimple("Interrupt and restart") {
                interruptAndRestartSession(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Show status") {
                showSessionStatus(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(rawOutput))
                showHint(project, "Copied starting-actor restart details for $relativePath")
            })
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    private fun notifyEditorHoldsPaneRestartBlocked(
        project: Project,
        file: VirtualFile,
        relativePath: String,
        refusal: EditorHoldsPaneRestartRefusal,
        rawOutput: String,
    ) {
        val summary = buildEditorHoldsPaneRestartBlockedMessage(relativePath, refusal)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            // No "Interrupt and restart" action: --force does not bypass the editor
            // guard. The operator must close the editor manually (#hj7s).
            notification.addAction(NotificationAction.createSimple("Show status") {
                showSessionStatus(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(rawOutput))
                showHint(project, "Copied editor-holds-pane restart details for $relativePath")
            })
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    internal fun buildBusySessionClearBlockedMessage(
        relativePath: String,
        refusal: BusySessionClearRefusal,
    ): String = buildString {
        if (refusal.protectedReason.isNotBlank()) {
            append("Clear Session Context is blocked for ")
            append(relativePath)
            append(".\nPane ")
            append(refusal.pane)
            append(" contains protected prompt input")
            append(" (")
            append(refusal.protectedReason)
            append("). Use Interrupt and clear to discard the prompt input, or Show status to inspect the session.")
            if (refusal.tail.isNotBlank() && refusal.tail != "unknown") {
                append("\nLatest pane output: ")
                append(refusal.tail)
            }
            return@buildString
        }
        append("Session is still running for ")
        append(relativePath)
        append(".\nPane ")
        append(refusal.pane)
        append(" is busy")
        if (refusal.currentCommand.isNotBlank() && refusal.currentCommand != "unknown") {
            append(" (")
            append(refusal.currentCommand)
            append(")")
        }
        append(". Wait for the turn to finish, then retry Clear Session Context.")
        append(" Use Refresh and retry if the pane has returned to an idle prompt, or Interrupt and clear to discard the running turn.")
        if (refusal.tail.isNotBlank() && refusal.tail != "unknown") {
            append("\nLatest pane output: ")
            append(refusal.tail)
        }
    }

    internal fun buildBusySessionRestartBlockedMessage(
        relativePath: String,
        refusal: BusySessionRestartRefusal,
    ): String = buildString {
        append("Restart Supervisor is blocked for ")
        append(relativePath)
        append(".\nPane ")
        append(refusal.pane)
        append(" is busy")
        if (refusal.currentCommand.isNotBlank() && refusal.currentCommand != "unknown") {
            append(" (")
            append(refusal.currentCommand)
            append(")")
        }
        append(". Use Interrupt and restart to stop the running turn and restart the supervisor, or Show status to inspect the session.")
        if (refusal.tail.isNotBlank() && refusal.tail != "unknown") {
            append("\nLatest pane output: ")
            append(refusal.tail)
        }
    }

    internal fun buildStartingSessionRestartBlockedMessage(
        relativePath: String,
        refusal: StartingSessionRestartRefusal,
    ): String = buildString {
        append("Restart Supervisor is blocked for ")
        append(relativePath)
        append(".\nThe authoritative actor is still starting")
        if (refusal.reason.isNotBlank()) {
            append(" and ")
            append(refusal.reason)
        }
        append(". Use Interrupt and restart to stop the current supervisor generation and restart anyway, or Show status to inspect the session.")
    }

    internal fun buildEditorHoldsPaneRestartBlockedMessage(
        relativePath: String,
        refusal: EditorHoldsPaneRestartRefusal,
    ): String = buildString {
        append("Restart Supervisor is blocked for ")
        append(relativePath)
        append(".\nA terminal editor")
        val editor = refusal.editor.ifBlank { refusal.currentCommand }
        if (editor.isNotBlank() && editor != "unknown") {
            append(" (")
            append(editor)
            append(")")
        }
        append(" holds pane ")
        append(refusal.pane)
        append(" — e.g. Claude Code `ctrl+g` edit-in-nvim. Close the editor (for example `:wq` in nvim) and retry; Interrupt and restart will not bypass it. Use Show status to inspect the session.")
        if (refusal.tail.isNotBlank() && refusal.tail != "unknown") {
            append("\nLatest pane output: ")
            append(refusal.tail)
        }
    }

    private fun notify(project: Project, content: String, type: NotificationType) {
        try {
            NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(content, type)
                .notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $content")
        }
    }

    internal fun buildRunAgentDocStillRunningMessage(relativePath: String): String {
        return "Agent Doc is still running for $relativePath.\n" +
            "The Codex pane has an active turn and is not ready for another routed follow-up yet. Wait for the turn to finish, then run Agent Doc again."
    }

    internal fun buildRunAgentDocQueuedMessage(relativePath: String): String {
        return "Agent Doc is queued behind the active owner for $relativePath.\n" +
            "It should run when that turn drains."
    }

    internal fun buildRunAgentDocDispatchUnprovenMessage(relativePath: String, routeOutput: String): String {
        val lines = routeOutput.lines()
        val attemptId = latestRegexValue(lines, ROUTE_EDITOR_ATTEMPT_REGEX)
        val snapshotPath = latestRegexValue(lines, ROUTE_SNAPSHOT_PATH_REGEX)
        return buildString {
            append("Agent Doc did not start for ")
            append(relativePath)
            append(".\nPane input was accepted, but dispatch-start proof did not appear.")
            if (attemptId.isNotBlank()) {
                append("\nAttempt: ")
                append(attemptId)
            }
            if (snapshotPath.isNotBlank()) {
                append("\nRoute snapshot: ")
                append(snapshotPath)
            }
            append("\nWait for an idle prompt or restart the session, then run Agent Doc again.")
        }
    }

    private fun notifyRunAgentDocStillRunning(project: Project, relativePath: String, routeOutput: String) {
        val summary = buildRunAgentDocStillRunningMessage(relativePath)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(routeOutput))
                showHint(project, "Copied running route details for $relativePath")
            })
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    private fun notifyRunAgentDocDispatchUnproven(project: Project, relativePath: String, routeOutput: String) {
        val summary = buildRunAgentDocDispatchUnprovenMessage(relativePath, routeOutput)
        val snapshotPath = latestRegexValue(routeOutput.lines(), ROUTE_SNAPSHOT_PATH_REGEX)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(routeOutput))
                showHint(project, "Copied unproven route details for $relativePath")
            })
            if (snapshotPath.isNotBlank()) {
                notification.addAction(NotificationAction.createSimple("Copy snapshot path") {
                    CopyPasteManager.getInstance().setContents(StringSelection(snapshotPath))
                    showHint(project, "Copied route snapshot path for $relativePath")
                })
            }
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    private fun notifyRunAgentDocQueued(project: Project, relativePath: String, routeOutput: String) {
        val summary = buildRunAgentDocQueuedMessage(relativePath)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(routeOutput))
                showHint(project, "Copied queued route details for $relativePath")
            })
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    internal fun routeFailureDiagnosticsFile(cwd: String, relativePath: String): File {
        val diagnosticsDir = File(cwd, ROUTE_ERROR_DIAGNOSTICS_DIR)
        val sanitized = relativePath
            .replace('\\', '/')
            .replace(Regex("[^A-Za-z0-9._/-]+"), "_")
            .replace("/", "__")
            .ifBlank { "route-error" }
        return File(diagnosticsDir, "$sanitized.txt")
    }

    internal fun persistRouteFailureOutput(cwd: String, relativePath: String, routeOutput: String): File? {
        return try {
            val diagnostics = routeFailureDiagnosticsFile(cwd, relativePath)
            diagnostics.parentFile?.mkdirs()
            diagnostics.writeText(routeOutput)
            diagnostics
        } catch (_: Exception) {
            null
        }
    }

    internal fun clearPersistedRouteFailureOutput(cwd: String, relativePath: String): Boolean {
        return try {
            val diagnostics = routeFailureDiagnosticsFile(cwd, relativePath)
            diagnostics.exists() && diagnostics.delete()
        } catch (_: Exception) {
            false
        }
    }

    internal fun notifyPersistentRouteFailure(
        project: Project,
        cwd: String,
        relativePath: String,
        exitCode: Int,
        elapsed: String,
        routeOutput: String,
    ) {
        val savedFile = persistRouteFailureOutput(cwd, relativePath, routeOutput)
        val summary = buildString {
            append("agent-doc route failed for ")
            append(relativePath)
            append(" after ")
            append(elapsed)
            append(" (exit ")
            append(exitCode)
            append(").")
            if (savedFile != null) {
                append("\nSaved exact route output to ")
                append(savedFile.relativeToOrSelf(File(cwd)).path)
            } else {
                append("\n")
                append(routeOutput)
            }
        }
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.ERROR)
            notification.isImportant = true
            notification.addAction(NotificationAction.createSimple("Copy route error") {
                CopyPasteManager.getInstance().setContents(StringSelection(routeOutput))
                showHint(project, "Copied route error for $relativePath")
            })
            if (savedFile != null) {
                notification.addAction(NotificationAction.createSimple("Open saved error") {
                    openFile(project, savedFile)
                })
                notification.addAction(NotificationAction.createSimple("Copy error path") {
                    CopyPasteManager.getInstance().setContents(StringSelection(savedFile.path))
                    showHint(project, "Copied saved error path for $relativePath")
                })
            }
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    /**
     * Extracts a brief layout description from a command list.
     * Returns a string like "--col a.md,b.md --col c.md" or "focus a.md",
     * suitable for showing in a notification balloon.
     */
    fun formatLayoutSummary(cmd: List<String>): String {
        // Find the subcommand (sync or focus)
        val subcommand = cmd.getOrNull(1) ?: return cmd.joinToString(" ")
        return when (subcommand) {
            "sync" -> {
                val parts = mutableListOf<String>()
                var focusFile: String? = null
                var i = 2
                while (i < cmd.size) {
                    if (cmd[i] == "--col" && i + 1 < cmd.size) {
                        parts.add("--col ${cmd[i + 1]}")
                        i += 2
                    } else if (cmd[i] == "--focus" && i + 1 < cmd.size) {
                        focusFile = cmd[i + 1]
                        i += 2
                    } else {
                        i++
                    }
                }
                val focusSuffix = if (focusFile != null) " [focus: $focusFile]" else ""
                "Sync: ${parts.joinToString(" ")}$focusSuffix"
            }
            "focus" -> "Focus: ${cmd.getOrNull(2) ?: ""}"
            else -> cmd.drop(1).joinToString(" ")
        }
    }

    fun notifyInfo(project: Project, content: String) {
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(content, NotificationType.INFORMATION)
            notification.notify(project)
            // Auto-expire after 3 seconds
            Thread {
                Thread.sleep(3000)
                notification.expire()
            }.start()
        } catch (_: Exception) {
            System.err.println("[agent-doc] $content")
        }
    }

    private fun formatElapsedMillis(elapsedMs: Long): String {
        val seconds = elapsedMs / 1000.0
        return if (seconds >= 10.0) {
            String.format("%.0fs", seconds)
        } else {
            String.format("%.1fs", seconds)
        }
    }

    /**
     * Opens the ~/.cache/junie-bridge/request.md file in the editor.
     * This file is written by junie-bridge.sh and contains the diff/prompt for Junie.
     */
    private fun openJunieRequest(project: Project) {
        val home = System.getProperty("user.home") ?: return
        val requestPath = "$home/.cache/junie-bridge/request.md"
        val requestFile = File(requestPath)
        if (!requestFile.exists()) return

        openFile(project, requestFile) {
            // Copy the diff content to clipboard to make it even easier to send to Junie
            try {
                val content = requestFile.readText()
                CopyPasteManager.getInstance().setContents(StringSelection(content))
                showHint(project, "Opened Junie request (diff copied to clipboard)")
            } catch (e: Exception) {
                showHint(project, "Opened Junie request")
            }
        }
    }

    private fun openFile(project: Project, file: File, afterOpen: (() -> Unit)? = null) {
        if (!file.exists()) return
        ApplicationManager.getApplication().invokeLater {
            val virtualFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(file) ?: return@invokeLater
            FileEditorManager.getInstance(project).openTextEditor(
                OpenFileDescriptor(project, virtualFile),
                true
            )
            afterOpen?.invoke()
        }
    }
}
