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
import java.time.Instant
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

object TerminalUtil {
    private val LOG = Logger.getInstance(TerminalUtil::class.java)
    private val agentDocProjectRoots = ConcurrentHashMap<String, String>()
    private const val ROUTE_ERROR_DIAGNOSTICS_DIR = ".agent-doc/state/editor-route-errors"
    private const val RESTART_TELEMETRY_OPS_LOG_MAX_LINES = 400
    internal const val RUN_ROUTE_WAIT_FOR_READY_SECONDS = 120L
    private const val UI_OUTCOME_QUEUED_BEHIND_OWNER = "queued_behind_owner"
    private const val UI_OUTCOME_RECOVERED_AND_RETRIED = "recovered_and_retried"
    private const val SUPERVISOR_RESTART_REDIRECT_MARKER = "supervisor_restart_redirect"
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
    private val ROUTE_DRAFT_PREVIEW_REGEX = Regex("""\bdraft_preview="([^"]*)"""")
    private val ROUTE_QUEUE_PAUSED_REASON_REGEX = Regex(
        """failed_stage=queue_paused\s+reason=(.*?)\s+receipt_id=""",
        RegexOption.DOT_MATCHES_ALL,
    )
    private val ROUTE_QUEUE_PAUSED_STALE_PID_REGEX = Regex("""\bstale_pid=(\d+)""")
    private val ROUTE_AGENT_SWITCH_DEFERRED_REGEX = Regex(
        """authoritative actor record for (.+?) is running harness ([^,\s]+), but frontmatter now resolves to ([^;\s]+); deferring to boundary agent restart instead of replacing live pane""",
        RegexOption.DOT_MATCHES_ALL,
    )
    private val SESSION_STATUS_ACTOR_GENERATION_REGEX = Regex("""\bactor:\s+generation=(\d+)""")
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

        @Synchronized
        fun cancel(key: String): Boolean {
            val previous = active.remove(key)
            val live = previous?.takeIf { it.isAlive() }
            live?.cancelForReplacement()
            return live != null
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

    internal data class RunAgentDocQueuePaused(
        val reason: String,
        val restartSupervisorRedirect: Boolean,
        val stalePid: String,
    )

    internal data class RunAgentDocAgentSwitchDeferred(
        val previousHarness: String,
        val targetHarness: String,
        val queuePaused: Boolean,
        val forceRequired: Boolean,
        val supervisorUnavailable: Boolean,
        /**
         * The supervisor is already restarting to complete this switch
         * (`#actorswitchdeferbusyself`). The operator must NOT be told to interrupt
         * or force — that would abort the restart that is finishing their switch.
         */
        val restartInFlight: Boolean = false,
    )

    internal enum class RunAgentDocRouteFailureKind {
        PERSISTENT,
        STARTUP_NOT_READY,
        BUSY_RUNNING,
        QUEUED_PENDING,
        QUEUE_PAUSED,
        AGENT_SWITCH_DEFERRED,
        DISPATCH_START_UNPROVEN,
        PROTECTED_PROMPT_INPUT,
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
     * `.agent-doc/`. If the file lives inside a
     * submodule that is itself an agent-doc project (e.g. `src/session-share/`),
     * the submodule root is returned. Otherwise falls back to the IDE project's
     * `basePath`.
     *
     * This path resolver is deliberately local and never crosses JNA. Selection
     * and focus listeners run on IDEA's event-dispatch thread; resolving a path
     * through the native generation bridge there made an unrelated CRDT
     * socket call freeze the whole IDE until the native-call timeout.
     *
     * Returns `(projectRoot, relativePath)` where `relativePath` is `file.path`
     * relative to `projectRoot`, suitable for passing to `agent-doc` commands
     * run from that directory.
     */
    fun resolveProject(project: Project, file: VirtualFile): Pair<String, String> {
        val basePath = project.basePath
        val resolved = resolveProjectPath(basePath, file.path)
        if (basePath != null && resolved.first != basePath) {
            // Register resolved root with PatchWatcher on-demand. This handles submodule
            // roots that weren't present at startup (e.g. user opens a file in a freshly
            // cloned submodule). Idempotent — no-op if already registered.
            try {
                PatchWatcher.getInstance(project).registerRoot(resolved.first)
            } catch (_: Exception) { /* best-effort */ }
        }
        return resolved
    }

    internal fun resolveProjectPath(basePath: String?, filePath: String): Pair<String, String> {
        val normalizedFile = File(filePath).toPath().toAbsolutePath().normalize()
        val detectedRoot = nearestAgentDocProjectRoot(normalizedFile.toString())
        if (detectedRoot != null) {
            return detectedRoot to relativePathUnder(detectedRoot, normalizedFile.toString())
        }

        if (basePath != null) {
            val root = File(basePath).toPath().toAbsolutePath().normalize().toString()
            return root to relativePathUnder(root, normalizedFile.toString())
        }
        val parent = normalizedFile.parent?.toString() ?: "/"
        return parent to (normalizedFile.fileName?.toString() ?: normalizedFile.toString())
    }

    internal fun nearestAgentDocProjectRoot(filePath: String): String? {
        val normalizedFile = File(filePath).toPath().toAbsolutePath().normalize()
        val cacheKey = normalizedFile.toString()
        agentDocProjectRoots[cacheKey]?.let { return it }
        var current = normalizedFile.parent?.toFile()
        while (current != null) {
            if (File(current, ".agent-doc").isDirectory) {
                val root = current.toPath().toAbsolutePath().normalize().toString()
                agentDocProjectRoots[cacheKey] = root
                return root
            }
            current = current.parentFile
        }
        return null
    }

    private fun relativePathUnder(root: String, filePath: String): String {
        val rootPath = File(root).toPath().toAbsolutePath().normalize()
        val normalizedFile = File(filePath).toPath().toAbsolutePath().normalize()
        return if (normalizedFile.startsWith(rootPath)) {
            rootPath.relativize(normalizedFile).toString().replace(File.separatorChar, '/')
        } else {
            normalizedFile.toString()
        }
    }

    /**
     * Routes a document trigger command via the CP `editor_route` RPC.
     *
     * The project controller executes the existing route implementation, which:
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
        resolved: Pair<String, String>? = null,
    ) {
        // `#jbedtledger`: `resolveProject` crosses the FFI boundary. Callers that
        // already resolved this exact file (SubmitAction, to open the attempt
        // ledger) can hand the result over instead of paying for it twice per
        // action. The re-registration side effect inside `resolveProject` is
        // idempotent, so skipping the second call changes nothing but cost.
        val (cwd, relativePath) = resolved ?: resolveProject(project, file)
        val routeKey = RunAgentDocAttemptLedger.routeKey(cwd, relativePath)
        val documentPath = java.io.File(cwd, relativePath).absolutePath

        LOG.warn("[route] sendToTerminal: cwd=$cwd rel=$relativePath transport=cp")
        attempt?.recordIfCurrent("route_prepare")

        if (!commandPreAcquired) {
            when (editorCommandRegistry.request(routeKey, EditorCommandKind.RUN_AGENT_DOC)) {
                EditorCommandDecision.START_NOW -> Unit
                EditorCommandDecision.DEDUPE_ACTIVE_RUN -> {
                    LOG.warn("[state] Run Agent Doc already dispatching for $relativePath; coalescing duplicate click")
                    attempt?.finishIfCurrent(
                        "route_deduped_active_run",
                        error = "existing editor_route request is still in flight",
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
                EditorCommandDecision.PREEMPT_RUN_WITH_CLEAR,
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
            // Build a diagnostic command shape matching the CP editor_route request.
            val cmd = buildEditorRouteRequestCommand(relativePath)

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
            val layoutArgs = buildRouteLayoutArgs(
                visibleMdFiles = visibleMdFiles,
                editorLayout = editorLayout,
                focusedFile = manager.selectedTextEditor?.virtualFile
                    ?.takeIf { it.name.endsWith(".md") }
                    ?.path,
            )
            cmd.addAll(layoutArgs)

            // Pass focused file
            LOG.warn("[route] sending CP request: ${cmd.joinToString(" ")}")
            attempt?.recordIfCurrent("route_command_built", command = cmd)

            val handle = RetryingRouteHandle()
            val routeSlotAcquired = inFlightRouteRegistry.startIfIdle(routeKey, handle)
            if (!routeSlotAcquired) {
                LOG.warn("[state] editor_route request already alive for $relativePath; suppressing duplicate Run Agent Doc")
                attempt?.finishIfCurrent(
                    "route_process_already_in_flight",
                    command = cmd,
                    error = "existing editor_route request is still in flight",
                )
                showHint(project, "Run Agent Doc is already dispatching for $relativePath")
                onComplete?.invoke()
                return
            }

            val startedAt = System.currentTimeMillis()
            Thread {
                var finalStage: String? = null
                var finalError: String? = null
                var routeGeneration: Long? = null
                try {
                    attempt?.recordIfCurrent("route_start", command = cmd)
                    routeGeneration =
                        StateProjectionBridge.recordRouteDispatchStarted(documentPath, routeKey)
                    val routeResult = CpRouteClient.runEditorRoute(
                        projectRoot = cwd,
                        filePath = documentPath,
                        relativePath = relativePath,
                        layoutArgs = layoutArgs,
                        waitForReadySeconds = RUN_ROUTE_WAIT_FOR_READY_SECONDS,
                        attemptId = attempt?.id,
                        routeKey = attempt?.routeKey,
                    )
                    val output = routeResult.output
                    val exitCode = routeResult.exitCode
                    val elapsed = formatElapsedMillis(System.currentTimeMillis() - startedAt)
                    val failureKind = classifyRunAgentDocRouteFailure(output)
                    if (handle.wasCanceled()) {
                        LOG.warn("[route] superseded route exited after replacement for $relativePath")
                        finalStage = "route_superseded"
                        finalError = "route handle was canceled"
                    } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.BUSY_RUNNING) {
                        LOG.warn("[route] busy/running for $relativePath: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        notifyRunAgentDocStillRunning(project, relativePath, output)
                        finalStage = "route_busy_running"
                        finalError = routeAttemptError(exitCode, failureKind, output)
                    } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.STARTUP_NOT_READY) {
                        LOG.warn("[route] startup not ready for $relativePath: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        notifyRunAgentDocStillRunning(project, relativePath, output)
                        finalStage = "route_startup_not_ready"
                        finalError = routeAttemptError(exitCode, failureKind, output)
                    } else if (failureKind == RunAgentDocRouteFailureKind.QUEUED_PENDING) {
                        LOG.warn("[route] queued behind active turn for $relativePath: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        notifyRunAgentDocQueued(project, relativePath, output)
                        finalStage = "route_queued_pending"
                        finalError = routeAttemptError(exitCode, failureKind, output)
                    } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.QUEUE_PAUSED) {
                        LOG.warn("[route] queue paused for $relativePath: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        notifyRunAgentDocQueuePaused(project, file, relativePath, output)
                        finalStage = "route_queue_paused"
                        finalError = routeAttemptError(exitCode, failureKind, output)
                    } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.AGENT_SWITCH_DEFERRED) {
                        LOG.warn("[route] agent switch deferred for $relativePath: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        notifyRunAgentDocAgentSwitchDeferred(project, file, relativePath, output)
                        finalStage = "route_agent_switch_deferred"
                        finalError = routeAttemptError(exitCode, failureKind, output)
                    } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.DISPATCH_START_UNPROVEN) {
                        LOG.warn("[route] dispatch start unproven for $relativePath: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        notifyRunAgentDocDispatchUnproven(project, relativePath, output)
                        finalStage = "route_dispatch_start_unproven"
                        finalError = routeAttemptError(exitCode, failureKind, output)
                    } else if (exitCode != 0 && failureKind == RunAgentDocRouteFailureKind.PROTECTED_PROMPT_INPUT) {
                        LOG.warn("[route] protected prompt input for $relativePath: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        notifyRunAgentDocProtectedPromptInput(project, relativePath, output)
                        finalStage = "route_protected_prompt_input"
                        finalError = routeAttemptError(exitCode, failureKind, output)
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
                    } else {
                        LOG.warn("[route] SUCCESS: $output")
                        clearPersistedRouteFailureOutput(project, cwd, relativePath)
                        StateProjectionBridge.recordRouteDispatchProven(
                            documentPath,
                            routeGeneration,
                            "jetbrains:${routeKey.hashCode()}:command-plane",
                        )
                        finalStage = "route_success"
                    }
                } catch (e: Exception) {
                    finalStage = "route_exception"
                    finalError = e.message ?: e.javaClass.simpleName
                    throw e
                } finally {
                    finalStage?.let { stage ->
                        attempt?.finishIfCurrent(stage, command = cmd, error = finalError)
                        if (stage != "route_success" && stage != "route_superseded") {
                            StateProjectionBridge.recordRouteBlocked(
                                documentPath,
                                routeGeneration,
                                finalError ?: stage,
                            )
                        }
                    }
                    // #n529b: read the reactive lazily-kt mirror (advanced by the
                    // route facts recorded above) instead of the cold projection
                    // pull, so the logged route/transport/proof summary derives from
                    // tracked cells. Cold pull is kept only as a cold-start fallback
                    // + transport-patch-id backfill inside reactiveSummaryForFile.
                    StateProjectionBridge.reactiveSummaryForFile(documentPath)?.let {
                        LOG.warn("[state-projection] ${it.compact()} file=$relativePath")
                    }
                    handle.markCompleted()
                    inFlightRouteRegistry.clearIfCurrent(routeKey, handle)
                    if (finalStage != "route_superseded" && !handle.wasCanceled()) {
                        editorCommandRegistry.complete(routeKey, EditorCommandKind.RUN_AGENT_DOC)
                    }
                    onComplete?.invoke()
                }
            }.start()
        } catch (e: Exception) {
            editorCommandRegistry.complete(routeKey, EditorCommandKind.RUN_AGENT_DOC)
            attempt?.finishIfCurrent("route_exception", error = e.message ?: e.javaClass.simpleName)
            val generation = StateProjectionBridge.recordRouteDispatchStarted(documentPath, routeKey)
            StateProjectionBridge.recordRouteBlocked(
                documentPath,
                generation,
                e.message ?: e.javaClass.simpleName,
            )
            onComplete?.invoke()
            notifyError(project, "Failed to send Run Agent Doc through CP: ${e.message}")
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

    internal fun buildEditorRouteRequestCommand(relativePath: String): MutableList<String> =
        mutableListOf(
            "cp:editor_route",
            "--dispatch-only",
            "--plain-trigger",
            "--wait-for-ready",
            RUN_ROUTE_WAIT_FOR_READY_SECONDS.toString(),
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
                clearPersistedRouteFailureOutput(project, cwd, relativePath)
                notifyInfo(project, sessionStatusSuccessMessage(relativePath, output))
            },
            onComplete = onComplete,
        )
    }

    fun restartSession(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        runRestartSupervisorCommand(project, file, force = false, onComplete = onComplete)
    }

    fun restartAgentSession(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        runRestartCommand(
            project = project,
            file = file,
            commandName = "restart-agent",
            startedMessage = "Restarting agent for ${file.name}",
            force = false,
            onComplete = onComplete,
        )
    }

    private fun restartSupervisorAndResumePausedQueue(project: Project, file: VirtualFile) {
        runRestartSupervisorCommand(
            project = project,
            file = file,
            force = false,
            afterSuccess = {
                resumePausedQueue(
                    project,
                    file,
                    "#qpauseux: JetBrains Restart Supervisor and Resume Queue action",
                )
            },
        )
    }

    private fun resumePausedQueue(
        project: Project,
        file: VirtualFile,
        reason: String = "#qpauseux: JetBrains Resume Queue action",
        onComplete: (() -> Unit)? = null,
    ) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        runDocumentCommand(
            project = project,
            file = file,
            command = buildSessionCommand(agentDoc, listOf("status"), relativePath),
            startedMessage = "Checking session generation for ${file.name}",
            onSuccess = { statusRelativePath, output ->
                val generation = sessionStatusActorGeneration(output)
                if (generation == null) {
                    notifyWarning(
                        project,
                        "Cannot resume queue for $statusRelativePath.\nSession status did not include an actor generation. Use Show status, then resume from the CLI with the observed generation.",
                    )
                    onComplete?.invoke()
                    return@runDocumentCommand
                }
                runDocumentCommand(
                    project = project,
                    file = file,
                    command = buildAdminQueueResumeCommand(
                        agentDoc,
                        cwd,
                        statusRelativePath,
                        generation,
                        reason,
                    ),
                    startedMessage = "Resuming queue for ${file.name}",
                    onSuccess = { resumedPath, resumeOutput ->
                        if (adminQueueControlAccepted(resumeOutput)) {
                            recordQueuePausedRouteActionInvoked(project, file, "resume_queue", "accepted")
                            notifyInfo(project, "Queue resumed for $resumedPath.\n$resumeOutput")
                        } else {
                            recordQueuePausedRouteActionInvoked(project, file, "resume_queue", "rejected")
                            notifyWarning(project, "Queue resume did not apply for $resumedPath.\n$resumeOutput")
                        }
                    },
                    onFailure = { resumedPath, exitCode, resumeOutput ->
                        recordQueuePausedRouteActionInvoked(project, file, "resume_queue", "failed")
                        notifyError(project, "Queue resume failed for $resumedPath (exit $exitCode):\n$resumeOutput")
                    },
                    onComplete = onComplete,
                )
            },
            onFailure = { statusRelativePath, exitCode, output ->
                notifyError(project, "Cannot inspect session generation for $statusRelativePath (exit $exitCode):\n$output")
                onComplete?.invoke()
            },
        )
    }

    internal fun recordRestartAgentMenuInvoked(project: Project, file: VirtualFile) {
        try {
            val (cwd, relativePath) = resolveProject(project, file)
            val agentDocDir = File(cwd, ".agent-doc")
            if (!agentDocDir.isDirectory) {
                return
            }
            val logsDir = File(agentDocDir, "logs")
            if (!logsDir.isDirectory && !logsDir.mkdirs()) {
                LOG.warn("[restart-agent] failed to create ops.log directory at ${logsDir.path}")
                return
            }
            val timestamp = Instant.ofEpochSecond(Instant.now().epochSecond).toString()
            File(logsDir, "ops.log").appendText(
                buildRestartAgentMenuInvokedOpsLogLine(timestamp, relativePath) + System.lineSeparator(),
            )
        } catch (e: Exception) {
            LOG.warn("[restart-agent] failed to record menu invocation marker: ${e.message}", e)
        }
    }

    internal fun buildRestartAgentMenuInvokedOpsLogLine(timestamp: String, relativePath: String): String {
        val doc = File(relativePath).nameWithoutExtension.ifBlank { "unknown" }
        return "[$timestamp] restart_agent_menu_invoked file=$relativePath source=jetbrains action=restart_agent doc=$doc"
    }

    private fun recordQueuePausedRouteNotificationShown(
        project: Project,
        file: VirtualFile,
        paused: RunAgentDocQueuePaused,
    ) {
        try {
            val (cwd, relativePath) = resolveProject(project, file)
            val agentDocDir = File(cwd, ".agent-doc")
            if (!agentDocDir.isDirectory) {
                return
            }
            val logsDir = File(agentDocDir, "logs")
            if (!logsDir.isDirectory && !logsDir.mkdirs()) {
                LOG.warn("[route] failed to create ops.log directory at ${logsDir.path}")
                return
            }
            val timestamp = Instant.ofEpochSecond(Instant.now().epochSecond).toString()
            File(logsDir, "ops.log").appendText(
                buildQueuePausedRouteNotificationOpsLogLine(timestamp, relativePath, paused) + System.lineSeparator(),
            )
        } catch (e: Exception) {
            LOG.warn("[route] failed to record paused-queue notification marker: ${e.message}", e)
        }
    }

    private fun recordQueuePausedRouteActionInvoked(
        project: Project,
        file: VirtualFile,
        action: String,
        status: String,
    ) {
        try {
            val (cwd, relativePath) = resolveProject(project, file)
            val agentDocDir = File(cwd, ".agent-doc")
            if (!agentDocDir.isDirectory) {
                return
            }
            val logsDir = File(agentDocDir, "logs")
            if (!logsDir.isDirectory && !logsDir.mkdirs()) {
                LOG.warn("[route] failed to create ops.log directory at ${logsDir.path}")
                return
            }
            val timestamp = Instant.ofEpochSecond(Instant.now().epochSecond).toString()
            File(logsDir, "ops.log").appendText(
                buildQueuePausedRouteActionOpsLogLine(timestamp, relativePath, action, status) + System.lineSeparator(),
            )
        } catch (e: Exception) {
            LOG.warn("[route] failed to record paused-queue action marker: ${e.message}", e)
        }
    }

    internal fun buildQueuePausedRouteNotificationOpsLogLine(
        timestamp: String,
        relativePath: String,
        paused: RunAgentDocQueuePaused,
    ): String {
        val doc = File(relativePath).nameWithoutExtension.ifBlank { "unknown" }
        val action = if (paused.restartSupervisorRedirect) {
            "restart_supervisor_and_resume"
        } else {
            "resume_queue"
        }
        val outcome = if (paused.restartSupervisorRedirect) {
            UI_OUTCOME_RECOVERED_AND_RETRIED
        } else {
            "blocked_with_exact_unblocker"
        }
        val stalePid = paused.stalePid.ifBlank { "none" }
        return "[$timestamp] jb_queue_paused_route_notification file=$relativePath source=jetbrains ui_outcome=$outcome action=$action stale_pid=$stalePid unblocker=resume_or_clear_queue_control doc=$doc"
    }

    internal fun buildQueuePausedRouteActionOpsLogLine(
        timestamp: String,
        relativePath: String,
        action: String,
        status: String,
    ): String {
        val doc = File(relativePath).nameWithoutExtension.ifBlank { "unknown" }
        return "[$timestamp] jb_queue_paused_route_action file=$relativePath source=jetbrains action=$action status=$status doc=$doc"
    }

    /**
     * #s81q: Stop Agent — `agent-doc session stop-agent <relPath>`. Stops the
     * harness agent child while keeping the supervisor alive at its keepalive
     * prompt. Mirrors [restartSession]'s session-subcommand shape; the supervisor
     * stays running so "Restart Agent" can bring the harness back.
     */
    fun stopAgent(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        runSessionCommand(
            project = project,
            file = file,
            args = listOf("stop-agent"),
            startedMessage = "Stopping agent for ${file.name}",
            onSuccess = { relativePath, output ->
                showHint(project, output.ifBlank { "Stopped agent for $relativePath (supervisor still running)." })
            },
            onComplete = onComplete,
        )
    }

    /**
     * Cancel Turn — `agent-doc session cancel-turn <relPath>`. Cancels the
     * currently running turn while keeping the agent harness and its supervisor
     * alive. No-op when the agent is idle, so it never closes the agent. Mirrors
     * [stopAgent]'s session-subcommand shape.
     */
    fun cancelTurn(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        runSessionCommand(
            project = project,
            file = file,
            args = listOf("cancel-turn"),
            startedMessage = "Cancelling turn for ${file.name}",
            onSuccess = { relativePath, output ->
                showHint(project, output.ifBlank { "Cancelled turn for $relativePath (no-op if the agent was idle)." })
            },
            onComplete = onComplete,
        )
    }

    /**
     * #s81q: Kill Supervisor — `agent-doc admin kill-supervisor <relPath>`. Stops
     * the whole route-owned supervisor process for this document. The CLI refuses
     * to kill the caller's own ancestor, so this runs from the editor's project
     * root (not the supervisor's own pane). Unlike [stopAgent] this is an `admin`
     * subcommand, not a `session` one, so it builds the command directly rather
     * than through [buildSessionCommand].
     */
    fun killSupervisor(project: Project, file: VirtualFile, onComplete: (() -> Unit)? = null) {
        val (cwd, relativePath) = resolveProject(project, file)
        val agentDoc = resolveAgentDoc(cwd)
        val cmd = listOf(agentDoc, "admin", "kill-supervisor", relativePath)
        runDocumentCommand(
            project = project,
            file = file,
            command = cmd,
            startedMessage = "Killing supervisor for ${file.name}",
            onSuccess = { relPath, output ->
                notifyInfo(project, output.ifBlank { "Killed supervisor for $relPath." })
            },
            onComplete = onComplete,
        )
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
        afterSuccess: (() -> Unit)? = null,
        onComplete: (() -> Unit)? = null,
    ) {
        runRestartCommand(
            project = project,
            file = file,
            commandName = "restart-supervisor",
            startedMessage = "Restarting supervisor for ${file.name}",
            force = force,
            afterSuccess = afterSuccess,
            onComplete = onComplete,
        )
    }

    private fun runRestartCommand(
        project: Project,
        file: VirtualFile,
        commandName: String,
        startedMessage: String,
        force: Boolean,
        afterSuccess: (() -> Unit)? = null,
        onComplete: (() -> Unit)? = null,
    ) {
        val (telemetryCwd, _) = resolveProject(project, file)
        val telemetryStartLine = restartTelemetryOpsLogLineCount(telemetryCwd)
        runSessionCommand(
            project = project,
            file = file,
            args = if (force) listOf(commandName, "--force") else listOf(commandName),
            startedMessage = startedMessage,
            onSuccess = { relativePath, output ->
                val telemetry = readRestartSupervisorTelemetry(telemetryCwd, relativePath, telemetryStartLine)
                val message = restartSessionSuccessMessage(relativePath, output, telemetry)
                if (telemetry != null) {
                    LOG.info("[session_restart] $relativePath ${telemetry.eventNames.joinToString(",")} pane=${telemetry.pane}")
                    notifyInfo(project, message)
                } else {
                    showHint(project, message)
                }
                afterSuccess?.invoke()
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
            EditorCommandDecision.PREEMPT_RUN_WITH_CLEAR -> {
                val canceled = inFlightRouteRegistry.cancel(routeKey)
                LOG.warn(
                    "[state] Clear Session Context preempted Run Agent Doc dispatch for $relativePath; " +
                        "routeCanceled=$canceled"
                )
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
                clearPersistedRouteFailureOutput(project, cwd, relativePath)
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

    internal fun buildAdminQueueResumeCommand(
        agentDoc: String,
        projectRoot: String,
        relativePath: String,
        observedGeneration: Long,
        reason: String,
    ): List<String> = listOf(
        agentDoc,
        "admin",
        "queue",
        "resume",
        relativePath,
        "--project-root",
        projectRoot,
        "--observed-generation",
        observedGeneration.toString(),
        "--reason",
        reason,
        "--json",
    )

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

    internal fun sessionStatusActorGeneration(output: String): Long? =
        SESSION_STATUS_ACTOR_GENERATION_REGEX.find(output)
            ?.groupValues
            ?.getOrNull(1)
            ?.toLongOrNull()

    internal fun adminQueueControlAccepted(output: String): Boolean =
        Regex(""""status"\s*:\s*"accepted"""").containsMatchIn(output) ||
            output.lineSequence().any { line ->
                line.contains("queue_resumed accepted") || line.contains("queue_resumed status=accepted")
            }

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
        val base = output.ifBlank { "Recycle requested for supervisor handling $relativePath" }
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

    internal fun classifyRunAgentDocRouteFailure(output: String): RunAgentDocRouteFailureKind {
        return when {
            isRunAgentDocRouteQueued(output) -> RunAgentDocRouteFailureKind.QUEUED_PENDING
            parseRunAgentDocQueuePaused(output) != null -> RunAgentDocRouteFailureKind.QUEUE_PAUSED
            parseRunAgentDocAgentSwitchDeferred(output) != null -> RunAgentDocRouteFailureKind.AGENT_SWITCH_DEFERRED
            isDispatchOnlyActiveTurnBlocked(output) -> RunAgentDocRouteFailureKind.BUSY_RUNNING
            isDispatchOnlyBusyActorWaitTimeout(output) -> RunAgentDocRouteFailureKind.BUSY_RUNNING
            isLatestRunStillBootingBusy(output) -> RunAgentDocRouteFailureKind.BUSY_RUNNING
            isDispatchStartUnproven(output) -> RunAgentDocRouteFailureKind.DISPATCH_START_UNPROVEN
            isProtectedPromptInputRouteFailure(output) -> RunAgentDocRouteFailureKind.PROTECTED_PROMPT_INPUT
            isLatestRunStillBootingTimedOut(output) ->
                RunAgentDocRouteFailureKind.STARTUP_NOT_READY
            isStartingActorRouteFailure(output) ->
                RunAgentDocRouteFailureKind.STARTUP_NOT_READY
            else -> RunAgentDocRouteFailureKind.PERSISTENT
        }
    }

    private fun isProtectedPromptInputRouteFailure(output: String): Boolean {
        val lower = output.lowercase()
        return lower.contains("route refusing to dispatch") &&
            lower.contains("composer contains protected prompt input")
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

    private fun isLatestRunStillBootingTimedOut(output: String): Boolean {
        val lower = output.lowercase()
        return isLatestRunStillBootingShape(lower) && lower.contains("(timed_out)")
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

    internal fun parseRunAgentDocQueuePaused(output: String): RunAgentDocQueuePaused? {
        val lower = output.lowercase()
        if (!lower.contains("failed_stage=queue_paused")) {
            return null
        }
        val reason = ROUTE_QUEUE_PAUSED_REASON_REGEX.find(output)
            ?.groupValues
            ?.getOrNull(1)
            ?.trim()
            .orEmpty()
        val stalePid = ROUTE_QUEUE_PAUSED_STALE_PID_REGEX.find(output)
            ?.groupValues
            ?.getOrNull(1)
            .orEmpty()
        val restartRedirect = lower.contains(SUPERVISOR_RESTART_REDIRECT_MARKER) ||
            hasUserFacingOutcome(lower, UI_OUTCOME_RECOVERED_AND_RETRIED) ||
            lower.contains("next_action=restart_supervisor_once_and_retry")
        return RunAgentDocQueuePaused(
            reason = reason,
            restartSupervisorRedirect = restartRedirect,
            stalePid = stalePid,
        )
    }

    internal fun parseRunAgentDocAgentSwitchDeferred(output: String): RunAgentDocAgentSwitchDeferred? {
        val match = ROUTE_AGENT_SWITCH_DEFERRED_REGEX.find(output) ?: return null
        val lower = output.lowercase()
        val restartInFlight = lower.contains("harness restart is already in flight")
        // `#actorswitchdeferbusyself`: an in-flight restart never asks for `--force`,
        // so it can never be reported as force-required.
        val forceRequired =
            !restartInFlight && lower.contains("restart-supervisor") && lower.contains("--force")
        return RunAgentDocAgentSwitchDeferred(
            previousHarness = match.groupValues.getOrNull(2).orEmpty(),
            targetHarness = match.groupValues.getOrNull(3).orEmpty(),
            queuePaused = lower.contains("queue is paused"),
            forceRequired = forceRequired,
            restartInFlight = restartInFlight,
            supervisorUnavailable = lower.contains("supervisor is unreachable") ||
                lower.contains("supervisor is unhealthy") ||
                lower.contains("supervisor is paused"),
        )
    }

    private fun hasUserFacingOutcome(lowercaseOutput: String, outcome: String): Boolean {
        return lowercaseOutput.contains("ui_outcome=$outcome")
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

    internal fun buildRunAgentDocQueuePausedMessage(
        relativePath: String,
        paused: RunAgentDocQueuePaused,
    ): String = buildString {
        append("Agent Doc queue is paused for ")
        append(relativePath)
        append(".\n")
        if (paused.restartSupervisorRedirect) {
            append("This pause looks like stale-supervisor churn. Restart Supervisor and resume the queue, then run Agent Doc again.")
            if (paused.stalePid.isNotBlank()) {
                append("\nStale supervisor pid: ")
                append(paused.stalePid)
            }
        } else {
            append("Resume the queue when unattended dispatch should continue, or use Show status to inspect it first.")
        }
        if (paused.reason.isNotBlank()) {
            append("\nReason: ")
            append(paused.reason.take(600))
        }
    }

    internal fun buildRunAgentDocAgentSwitchDeferredMessage(
        relativePath: String,
        deferred: RunAgentDocAgentSwitchDeferred,
    ): String = buildString {
        append("Agent Doc did not switch harnesses for ")
        append(relativePath)
        append(".\nCurrent actor is ")
        append(deferred.previousHarness.ifBlank { "the previous harness" })
        append("; frontmatter now resolves to ")
        append(deferred.targetHarness.ifBlank { "the new harness" })
        append(". ")
        when {
            deferred.queuePaused -> {
                append("The queue is paused, so the boundary restart will not fire. Restart Supervisor and resume the queue, then run Agent Doc again.")
            }
            deferred.restartInFlight -> {
                append("The harness restart is already in flight and completes the switch at the next boundary. Wait for it, then run Agent Doc again — do not interrupt or force a restart.")
            }
            deferred.forceRequired -> {
                append("The pane is not at a dispatch-ready boundary. Use Interrupt and restart to force the harness switch.")
            }
            deferred.supervisorUnavailable -> {
                append("The supervisor is not healthy enough to reach the boundary restart. Restart Supervisor, then run Agent Doc again.")
            }
            else -> {
                append("The supervisor will switch at the next idle boundary. Use Restart Supervisor to switch now.")
            }
        }
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

    internal fun buildRunAgentDocProtectedPromptInputMessage(relativePath: String, routeOutput: String): String {
        val lines = routeOutput.lines()
        val snapshotPath = latestRegexValue(lines, ROUTE_SNAPSHOT_PATH_REGEX)
        val draftPreview = latestRegexValue(lines, ROUTE_DRAFT_PREVIEW_REGEX)
        return buildString {
            append("Agent Doc did not start for ")
            append(relativePath)
            append(".\nThe target pane has unsent prompt text. Clear or submit that draft, then run Agent Doc again.")
            if (draftPreview.isNotBlank()) {
                append("\nDraft preview: ")
                append(draftPreview)
            }
            if (snapshotPath.isNotBlank()) {
                append("\nRoute snapshot: ")
                append(snapshotPath)
            }
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

    private fun notifyRunAgentDocProtectedPromptInput(project: Project, relativePath: String, routeOutput: String) {
        val summary = buildRunAgentDocProtectedPromptInputMessage(relativePath, routeOutput)
        val snapshotPath = latestRegexValue(routeOutput.lines(), ROUTE_SNAPSHOT_PATH_REGEX)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.WARNING)
            notification.isImportant = true
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(routeOutput))
                showHint(project, "Copied protected-input route details for $relativePath")
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

    private fun notifyRunAgentDocQueuePaused(
        project: Project,
        file: VirtualFile,
        relativePath: String,
        routeOutput: String,
    ) {
        val paused = parseRunAgentDocQueuePaused(routeOutput)
            ?: RunAgentDocQueuePaused(reason = "", restartSupervisorRedirect = false, stalePid = "")
        val summary = buildRunAgentDocQueuePausedMessage(relativePath, paused)
        recordQueuePausedRouteNotificationShown(project, file, paused)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, NotificationType.INFORMATION)
            notification.isImportant = true
            if (paused.restartSupervisorRedirect) {
                notification.addAction(NotificationAction.createSimple("Restart Supervisor and resume") {
                    restartSupervisorAndResumePausedQueue(project, file)
                })
            } else {
                notification.addAction(NotificationAction.createSimple("Resume queue") {
                    resumePausedQueue(project, file)
                })
            }
            notification.addAction(NotificationAction.createSimple("Show status") {
                showSessionStatus(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(routeOutput))
                showHint(project, "Copied paused queue details for $relativePath")
            })
            notification.notify(project)
        } catch (_: Exception) {
            System.err.println("[agent-doc] $summary")
        }
    }

    private fun notifyRunAgentDocAgentSwitchDeferred(
        project: Project,
        file: VirtualFile,
        relativePath: String,
        routeOutput: String,
    ) {
        val deferred = parseRunAgentDocAgentSwitchDeferred(routeOutput)
            ?: RunAgentDocAgentSwitchDeferred(
                previousHarness = "",
                targetHarness = "",
                queuePaused = false,
                forceRequired = false,
                supervisorUnavailable = false,
            )
        val summary = buildRunAgentDocAgentSwitchDeferredMessage(relativePath, deferred)
        try {
            val notification = NotificationGroupManager.getInstance()
                .getNotificationGroup("Agent Doc")
                .createNotification(summary, if (deferred.forceRequired) NotificationType.WARNING else NotificationType.INFORMATION)
            notification.isImportant = true
            when {
                deferred.queuePaused -> {
                    notification.addAction(NotificationAction.createSimple("Restart Supervisor and resume") {
                        restartSupervisorAndResumePausedQueue(project, file)
                    })
                }
                // `#actorswitchdeferbusyself`: the restart completing this switch is
                // already running. Offer no restart action at all — every restart
                // action here would abort the switch the operator asked for.
                deferred.restartInFlight -> {}
                deferred.forceRequired -> {
                    notification.addAction(NotificationAction.createSimple("Interrupt and restart") {
                        interruptAndRestartSession(project, file)
                    })
                }
                else -> {
                    notification.addAction(NotificationAction.createSimple("Restart Supervisor") {
                        restartSession(project, file)
                    })
                }
            }
            notification.addAction(NotificationAction.createSimple("Show status") {
                showSessionStatus(project, file)
            })
            notification.addAction(NotificationAction.createSimple("Copy details") {
                CopyPasteManager.getInstance().setContents(StringSelection(routeOutput))
                showHint(project, "Copied harness-switch route details for $relativePath")
            })
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

    internal fun clearPersistedRouteFailureOutput(project: Project, cwd: String, relativePath: String): Boolean {
        val cleared = clearPersistedRouteFailureOutput(cwd, relativePath)
        if (cleared) {
            refreshRouteFailureStatus(project, cwd, relativePath, "route-failure-cleared")
        }
        return cleared
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
        refreshRouteFailureStatus(project, cwd, relativePath, "route-failure-persisted")
        val summary = buildString {
            append("editor_route failed for ")
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

    private fun refreshRouteFailureStatus(project: Project, cwd: String, relativePath: String, reason: String) {
        val file = File(cwd, relativePath)
        val filePath = try {
            file.canonicalPath
        } catch (_: Exception) {
            file.absolutePath
        }
        TurnStateBannerRefresher.getInstance(project).requestRefresh(filePath, reason)
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
