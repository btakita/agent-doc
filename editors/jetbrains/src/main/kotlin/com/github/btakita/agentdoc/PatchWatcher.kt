package com.github.btakita.agentdoc

import com.sun.jna.Pointer
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.TextEditor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vcs.changes.VcsDirtyScopeManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.util.Alarm
import java.io.File
import java.lang.reflect.Field
import java.lang.reflect.Method
import java.nio.file.FileSystems
import java.nio.file.Path
import java.nio.file.StandardWatchEventKinds
import java.nio.file.WatchService
import java.security.MessageDigest

/**
 * Watches `.agent-doc/patches/` for JSON patch files and applies them
 * via IntelliJ's Document API. This avoids external file change dialogs
 * and cursor jumps that occur when agent-doc writes directly to disk.
 *
 * Flow:
 * 1. `agent-doc write --ipc` writes `<hash>.json` to `.agent-doc/patches/`
 * 2. This watcher detects the new file via NIO WatchService
 * 3. Reads the JSON, finds the target document, applies patches
 * 4. Saves the document and deletes the JSON file (ACK)
 * 5. agent-doc polls for deletion and updates the snapshot
 *
 * **Multi-root:** a single watcher tracks every nested `.agent-doc/` project
 * under `project.basePath` (scanned at startup) plus any additional roots
 * discovered at runtime via [registerRoot] (called by actions when they
 * resolve a submodule root via FFI). Each root has its own NIO
 * WatchService thread, its own FFI socket listener, and its own applied-
 * patch dedup cache. Writes from `src/<submodule>/.agent-doc/patches/` are
 * applied by the same plugin instance that handles the parent's patches.
 */
class PatchWatcher(private val project: Project) : Disposable {

    private data class RootState(
        val root: String,
        val patchesDir: File,
        @Volatile var watchThread: Thread? = null,
        @Volatile var ipcCallback: AgentDocLib.IpcMessageCallback? = null,
        @Volatile var ipcCallbackV2: AgentDocLib.IpcMessageCallbackV2? = null,
    )

    /** Registered roots, keyed by absolute path. Written through [registerRoot]. */
    private val rootStates = java.util.concurrent.ConcurrentHashMap<String, RootState>()

    /** Dedup cache keyed by patch_id (globally unique). Shared across all roots. */
    private val appliedPatchIds = java.util.concurrent.ConcurrentHashMap<String, Long>()

    /** Patch files delayed because the target document is still being edited. */
    private val scheduledPatchRetries = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()

    /** Boundary reposition requests delayed because the target document is still being edited. */
    private val scheduledRepositionRetries = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()

    /** Patch ids that already hit an IntelliJ File Cache Conflict and must not write on Cancel. */
    private val memoryDiskConflictDeferredPatchIds = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()

    @Volatile private var memoryDiskConflictReflectionWarned = false

    /**
     * Signal that the most recent successful [applyPatch] / [applyPatchViaVfs]
     * call was structurally a no-op against the live buffer (response was
     * already present). Read by the v2 socket callback path to map the outcome
     * to `already_applied` so the binary skips the file-IPC fallback.
     *
     * Set inside the apply functions immediately before returning `true` from
     * the `result == content` branch; reset to `false` at the start of the v2
     * callback path. Safe because `invokeAndWait` blocks the FFI thread until
     * the EDT closure completes, so reads/writes do not race.
     *
     * Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
     * `#ipcpluginalready`.
     */
    @Volatile private var lastApplyWasNoOp = false

    @Volatile private var lastApplyWasDeferredForConflict = false

    @Volatile private var lastApplyRejectedConflictCancel = false

    @Volatile private var running = false

    private val APPLIED_PATCH_TTL_MS = 60_000L // 60s TTL

    /** Record a patch_id as applied (call after successful applyPatch). */
    private fun recordApplied(patchId: String?) {
        if (patchId == null) return
        appliedPatchIds[patchId] = System.currentTimeMillis()
        // Lazy cleanup: remove entries older than TTL
        val cutoff = System.currentTimeMillis() - APPLIED_PATCH_TTL_MS
        appliedPatchIds.entries.removeIf { it.value < cutoff }
    }

    /** Check if a patch_id was already applied (dedup guard). */
    private fun isAlreadyApplied(patchId: String?): Boolean {
        if (patchId == null) return false
        val ts = appliedPatchIds[patchId] ?: return false
        // Expired entries don't count
        if (System.currentTimeMillis() - ts > APPLIED_PATCH_TTL_MS) {
            appliedPatchIds.remove(patchId)
            return false
        }
        return true
    }

    fun start() {
        if (running) return
        running = true
        val basePath = project.basePath ?: return
        registerRoot(basePath)
        // Scan for nested .agent-doc/ dirs under basePath (submodules, nested repos).
        // Each discovered parent (the directory CONTAINING .agent-doc/) gets its own watcher.
        for (nested in discoverNestedRoots(basePath)) {
            registerRoot(nested)
        }
    }

    /**
     * Register a root directory (must contain or will contain `.agent-doc/patches/`).
     * Spawns a dedicated watch thread + FFI socket listener for this root.
     * Idempotent: calling with an already-registered root is a no-op.
     *
     * Called at startup for [project.basePath] + nested discoveries, and at runtime
     * by action code when a submodule root is resolved via FFI.
     */
    fun registerRoot(root: String) {
        if (!running) return
        if (rootStates.containsKey(root)) return
        val patchesDir = File(root, ".agent-doc/patches")
        if (!patchesDir.exists()) {
            patchesDir.mkdirs()
        }
        val state = RootState(root, patchesDir)
        if (rootStates.putIfAbsent(root, state) != null) return // race: another caller won

        state.watchThread = Thread({
            try {
                watchLoop(state, patchesDir.toPath())
            } catch (_: InterruptedException) {
                // Normal shutdown
            } catch (e: Exception) {
                if (running) {
                    LOG.warn("PatchWatcher error for root $root", e)
                }
            }
        }, "agent-doc-patch-watcher-${File(root).name}").apply {
            isDaemon = true
            start()
        }

        startSocketListenerViaFfi(state)
        processPendingPatches(patchesDir)
        LOG.info("[patch-watcher] registered root: $root")
    }

    /**
     * Scan under [basePath] for nested `.agent-doc/` dirs. Returns the PARENT of each
     * match (the directory that contains `.agent-doc/`). Skips common build/VCS dirs
     * and caps depth to avoid runaway traversals.
     */
    private fun discoverNestedRoots(basePath: String): List<String> {
        val skip = setOf(
            ".git", ".idea", ".gradle", ".agent-doc",
            "node_modules", "target", "build", "dist", "out",
            "venv", ".venv", "__pycache__",
        )
        val found = mutableListOf<String>()
        val base = File(basePath).absoluteFile
        fun scan(dir: File, depth: Int) {
            if (depth > 6) return
            val children = dir.listFiles() ?: return
            for (child in children) {
                if (!child.isDirectory) continue
                if (child.name in skip) continue
                val agentDocDir = File(child, ".agent-doc")
                if (agentDocDir.isDirectory) {
                    val absolute = child.absolutePath
                    if (absolute != base.absolutePath) {
                        found.add(absolute)
                    }
                }
                scan(child, depth + 1)
            }
        }
        scan(base, 0)
        return found
    }

    /**
     * Walk up from [filePath] to find the enclosing `.agent-doc/` root directory.
     * Returns the root path (parent of `.agent-doc/`) or null if not found.
     * Mirrors find_project_root in the Rust binary.
     */
    private fun resolveRootFor(filePath: String): String? {
        var dir: File? = File(filePath).absoluteFile.parentFile
        while (dir != null) {
            if (File(dir, ".agent-doc").isDirectory) {
                return dir.absolutePath
            }
            dir = dir.parentFile
        }
        return null
    }

    /**
     * Start socket IPC listener via FFI for a given root.
     * The callback dispatches messages to the EDT for Document API operations.
     * Keeps a strong reference to the callback (in [state]) to prevent GC.
     */
    private fun startSocketListenerViaFfi(state: RootState) {
        val lib = AgentDocLib.get()
        if (lib == null) {
            LOG.info("[socket] FFI unavailable, socket listener not started for ${state.root} (file-based IPC only)")
            return
        }

        // Prefer the v2 listener so we can emit `already_applied` acks. Older
        // binaries do not export v2 — fall back to v1 silently.
        // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
        // `#ipcpluginalready`.
        val callbackV2 = object : AgentDocLib.IpcMessageCallbackV2 {
            override fun invoke(message: Pointer): Int {
                return try {
                    val json = message.getString(0)
                    handleSocketMessageV2(json)
                } catch (e: Exception) {
                    LOG.warn("[socket] callback error", e)
                    0
                }
            }
        }
        val v2Started = try {
            lib.agent_doc_start_ipc_listener_v2(state.root, callbackV2)
        } catch (_: UnsatisfiedLinkError) {
            false
        } catch (_: NoSuchMethodError) {
            false
        }
        if (v2Started) {
            state.ipcCallbackV2 = callbackV2
            LOG.info("[socket] IPC listener v2 started via FFI for ${state.root}")
            return
        }

        val callback = object : AgentDocLib.IpcMessageCallback {
            override fun invoke(message: Pointer): Boolean {
                return try {
                    val json = message.getString(0)
                    handleSocketMessageV2(json) == 1
                } catch (e: Exception) {
                    LOG.warn("[socket] callback error", e)
                    false
                }
            }
        }
        state.ipcCallback = callback

        val started = lib.agent_doc_start_ipc_listener(state.root, callback)
        if (started) {
            LOG.info("[socket] IPC listener v1 started via FFI for ${state.root} (binary lacks v2)")
        } else {
            LOG.warn("[socket] Failed to start IPC listener via FFI for ${state.root}")
        }
    }

    private fun watchLoop(state: RootState, dir: Path) {
        val watchService: WatchService = FileSystems.getDefault().newWatchService()
        dir.register(watchService, StandardWatchEventKinds.ENTRY_CREATE, StandardWatchEventKinds.ENTRY_MODIFY)

        while (running) {
            val key = watchService.poll(500, java.util.concurrent.TimeUnit.MILLISECONDS) ?: continue
            val loopStart = System.nanoTime()
            var overflow = false
            for (event in key.pollEvents()) {
                if (event.kind() == StandardWatchEventKinds.OVERFLOW) {
                    overflow = true
                    continue
                }
                val filename = event.context() as? Path ?: continue
                if (filename.toString() == "vcs-refresh.signal") {
                    val signalFile = dir.resolve(filename).toFile()
                    if (signalFile.exists()) {
                        signalFile.delete()
                        refreshVcs()
                    }
                } else if (filename.toString().endsWith(".json")) {
                    val patchFile = dir.resolve(filename).toFile()
                    if (patchFile.exists()) {
                        processPatchFile(patchFile)
                    }
                }
            }
            // On inotify overflow, scan the directory for any missed files.
            // This handles the case where heavy I/O (e.g. cargo build) fills
            // the kernel's inotify buffer and events are silently dropped.
            if (overflow) {
                LOG.info("[patch-watcher] inotify overflow detected, scanning for missed files")
                processPendingPatches(dir.toFile())
            }
            val loopMs = (System.nanoTime() - loopStart) / 1_000_000
            if (loopMs > 50) LOG.info("[perf] watchLoop iteration: ${loopMs}ms")
            if (!key.reset()) break
        }

        watchService.close()
    }

    private fun processPendingPatches(dir: File) {
        // Process any pending VCS refresh signal
        val signalFile = File(dir, "vcs-refresh.signal")
        if (signalFile.exists()) {
            signalFile.delete()
            refreshVcs()
        }
        // Process any pending patch files
        val files = dir.listFiles { f -> f.extension == "json" } ?: return
        for (file in files) {
            processPatchFile(file)
        }
    }

    /**
     * Handle a socket IPC message from the FFI listener.
     * Called on the FFI listener thread — dispatches to EDT for Document API operations.
     * Returns true if handled, false on error.
     */
    /**
     * Handle a socket IPC message and return the v2 ack outcome.
     *
     * Return values match the FFI v2 contract:
     * - 0 → ack `{"status":"error"}` (apply failed)
     * - 1 → ack `{"status":"ok"}` (apply succeeded with content change)
     * - 2 → ack `{"status":"error","reason":"already_applied"}` (patch text
     *   already present in live buffer; binary skips file-IPC fallback so a
     *   duplicate response heading cannot land)
     *
     * Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
     * `#ipcpluginalready`.
     */
    private fun handleSocketMessageV2(json: String): Int {
        val type = extractStringField(json, "type") ?: return 0

        return when (type) {
            "patch" -> {
                val patch = parsePatchJson(json) ?: return 0
                if (isClaimedByForceDisk(patch.patchId, patch.file)) {
                    LOG.info("[socket] dedup: sentinel exists for patch_id ${patch.patchId} — emitting already_applied")
                    return APPLY_ALREADY_APPLIED
                }
                if (isAlreadyApplied(patch.patchId)) {
                    LOG.info("[socket] dedup: patch_id ${patch.patchId} already applied — emitting already_applied")
                    return APPLY_ALREADY_APPLIED
                }
                if (!patch.fullContent.isNullOrEmpty()) {
                    LOG.warn("[socket] full-content IPC is disabled; rejecting patch_id ${patch.patchId} for ${patch.file}")
                    return APPLY_FAILED
                }
                if (!awaitIdleBeforeDocumentMutation(patch.file, "socket patch")) {
                    return APPLY_FAILED
                }
                var applied = false
                var wasNoOp = false
                ApplicationManager.getApplication().invokeAndWait {
                    // Re-check under EDT to avoid TOCTOU race with file watcher
                    if (isAlreadyApplied(patch.patchId)) {
                        LOG.info("[socket] dedup (EDT): patch_id ${patch.patchId} already applied — emitting already_applied")
                        wasNoOp = true
                        return@invokeAndWait
                    }
                    lastApplyWasNoOp = false
                    applied = try {
                        applyPatch(patch)
                    } catch (e: Exception) {
                        LOG.warn("[socket] Failed to apply patch", e)
                        false
                    }
                    if (applied) {
                        recordApplied(patch.patchId)
                        wasNoOp = lastApplyWasNoOp
                        lastApplyWasNoOp = false
                    }
                }
                if (applied || wasNoOp) {
                    val root = resolveRootFor(patch.file) ?: project.basePath
                    if (root != null) {
                        val hash = docHash(patch.file)
                        val patchFile = File(root, ".agent-doc/patches/$hash.json")
                        if (patchFile.exists()) {
                            LOG.info("[socket] cleaning stale patch file after socket delivery: ${patchFile.name}")
                            patchFile.delete()
                        }
                    }
                }
                when {
                    !applied && !wasNoOp -> APPLY_FAILED
                    wasNoOp -> APPLY_ALREADY_APPLIED
                    else -> APPLY_APPLIED
                }
            }
            "reposition" -> {
                val file = extractStringField(json, "file") ?: return APPLY_FAILED
                val boundaryId = extractStringField(json, "boundary_id")
                val preserveHead = extractBooleanField(json, "preserve_head")
                repositionBoundaryViaDocument(file, boundaryId, preserveHead)
                APPLY_APPLIED
            }
            "vcs_refresh" -> {
                refreshVcs()
                APPLY_APPLIED
            }
            else -> {
                LOG.warn("[socket] Unknown message type: $type")
                APPLY_FAILED
            }
        }
    }

    /**
     * Reposition boundary marker in a document via Document API.
     * Used by socket IPC "reposition" messages.
     *
     * Waits for typing to settle via FFI `await_idle` (blocking, runs on
     * background thread) before applying the reposition on the EDT.
     * This prevents keystroke loss from Document API writes racing with
     * user input.
     */
    private fun repositionBoundaryViaDocument(filePath: String, boundaryId: String? = null, preserveHead: Boolean = false) {
        com.intellij.util.concurrency.AppExecutorUtil.getAppExecutorService().submit {
            val lib = AgentDocLib.get()
            val idle = lib?.agent_doc_await_idle(filePath, TypingTracker.DEBOUNCE_MS, 5_000) ?: true
            if (!idle) {
                LOG.warn("[patch-watcher] typing debounce timed out before reposition for $filePath; retrying")
                scheduleRepositionRetry(filePath, boundaryId, preserveHead)
                return@submit
            }
            ApplicationManager.getApplication().invokeLater {
                val reposStart = System.nanoTime()
                val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeLater
                targetFile.refresh(false, false)
                val fdm = FileDocumentManager.getInstance()
                val document = fdm.getDocument(targetFile) ?: return@invokeLater
                val diskContent = String(targetFile.contentsToByteArray(), targetFile.charset)
                if (!fdm.isDocumentUnsaved(document)) {
                    fdm.reloadFromDisk(document)
                }

                WriteCommandAction.runWriteCommandAction(project, "Agent Doc Reposition", null, {
                    val content = document.text
                    val proof = EditorApplyProof(content, document.modificationStamp)
                    val sourceContent =
                        if (preserveHead && shouldPreferCommittedDiskContentForRepositionUtil(content, diskContent)) {
                            diskContent
                        } else {
                            content
                        }
                    val result = if (preserveHead) {
                        NativePatching.repositionBoundaryToEndPreserveHead(sourceContent, boundaryId)
                            ?: repositionBoundaryToEnd(sourceContent, "exchange", boundaryId, preserveHead = true)
                    } else {
                        NativePatching.repositionBoundaryToEnd(sourceContent, boundaryId)
                            ?: repositionBoundaryToEnd(sourceContent, "exchange", boundaryId)
                    } ?: return@runWriteCommandAction
                    if (result != content && editorApplyProofStillCurrentUtil(proof, document.text, document.modificationStamp)) {
                        LOG.info(
                            documentMutationDiagnosticUtil(
                                "repositionBoundary", filePath, boundaryId, "document_api",
                                content, result, document.modificationStamp, true,
                            )
                        )
                        // #jb-settext-payload-log: capture the exact setText
                        // payload at debug level for IPC-corruption forensics.
                        if (LOG.isDebugEnabled) {
                            LOG.debug(
                                "[patch-watcher] setText payload (repositionBoundary) for $filePath boundaryId=$boundaryId (${result.length} chars):\n$result"
                            )
                        }
                        document.setText(result)
                    } else if (result != content) {
                        LOG.warn("[patch-watcher] stale editor generation before reposition for $filePath; retrying")
                        scheduleRepositionRetry(filePath, boundaryId, preserveHead)
                    }
                })
                FileDocumentManager.getInstance().saveDocument(document)
                val reposMs = (System.nanoTime() - reposStart) / 1_000_000
                if (reposMs > 50) LOG.info("[perf] repositionBoundary: ${reposMs}ms $filePath")
            }
        }
    }

    private fun processPatchFile(patchFile: File) {
        try {
            val parseStart = System.nanoTime()
            val json = patchFile.readText()
            val patch = parsePatchJson(json) ?: return
            val parseMs = (System.nanoTime() - parseStart) / 1_000_000
            if (parseMs > 10) LOG.info("[perf] processPatchFile parse: ${parseMs}ms ${patchFile.name}")

            // Startup dedup guard: check if this patch was already applied in a previous cycle.
            // Uses snapshot timestamp — if the snapshot is newer than the patch file, the
            // binary already saved a snapshot after applying this patch, so it's stale.
            // Boundary-ID checks are unreliable because boundaries get consumed on apply.
            if (isPatchAlreadyApplied(patch, patchFile)) {
                LOG.info("[patch-watcher] dedup: snapshot newer than patch file, deleting stale: ${patchFile.name}")
                patchFile.delete()
                return
            }

            // Generation fence (#late-ipc-patch-plugin-apply-fence): drop a patch
            // whose generation token proves it is superseded — its cycle already
            // committed, or the live doc moved on from the baseline it targeted —
            // instead of applying it late and re-materializing a duplicate Re block.
            if (isPatchGenerationSuperseded(patch)) {
                LOG.info("[patch-watcher] generation fence: dropping superseded patch: ${patchFile.name}")
                patchFile.delete()
                return
            }

            // patch_id dedup: if socket IPC already applied this logical write, skip.
            if (isAlreadyApplied(patch.patchId)) {
                LOG.info("[patch-watcher] dedup: patch_id ${patch.patchId} already applied via socket — deleting: ${patchFile.name}")
                patchFile.delete()
                return
            }

            if (!patch.fullContent.isNullOrEmpty()) {
                LOG.warn("[patch-watcher] full-content IPC is disabled; deleting stale/foreign patch file: ${patchFile.name}")
                patchFile.delete()
                return
            }

            if (!awaitIdleBeforeDocumentMutation(patch.file, "file patch")) {
                schedulePatchRetry(patchFile, "typing active")
                return
            }
            ApplicationManager.getApplication().invokeLater {
                if (isClaimedByForceDisk(patch.patchId, patch.file) || isPatchAlreadyApplied(patch, patchFile)) {
                    LOG.info("[patch-watcher] dedup (inner): skipping apply for ${patchFile.name}")
                    patchFile.delete()
                    return@invokeLater
                }
                // Re-check the generation fence under EDT: the producing cycle may
                // have committed (or a later cycle rewritten the doc) between the
                // file-thread check and this EDT dispatch.
                if (isPatchGenerationSuperseded(patch)) {
                    LOG.info("[patch-watcher] generation fence (EDT): dropping superseded patch: ${patchFile.name}")
                    patchFile.delete()
                    return@invokeLater
                }
                // Re-check patch_id dedup under EDT (socket handler may have applied between queue and EDT dispatch)
                if (isAlreadyApplied(patch.patchId)) {
                    LOG.info("[patch-watcher] dedup (EDT): patch_id ${patch.patchId} already applied — deleting: ${patchFile.name}")
                    patchFile.delete()
                    return@invokeLater
                }
                val applyStart = System.nanoTime()
                val applied = try {
                    applyPatch(patch)
                } catch (e: Exception) {
                    LOG.warn("Failed to apply patch from ${patchFile.name}", e)
                    false
                }
                val applyMs = (System.nanoTime() - applyStart) / 1_000_000
                if (applyMs > 50) LOG.info("[perf] applyPatch: ${applyMs}ms ${patch.file}")
                if (applied) {
                    recordApplied(patch.patchId)
                    patchFile.delete()
                } else if (lastApplyWasDeferredForConflict) {
                    schedulePatchRetry(patchFile, "File Cache Conflict pending")
                } else if (lastApplyRejectedConflictCancel) {
                    LOG.warn("Patch rejected after File Cache Conflict cancel, leaving file for explicit retry: ${patchFile.name}")
                } else {
                    LOG.warn("Patch not applied, leaving file for retry: ${patchFile.name}")
                }
            }
        } catch (e: Exception) {
            LOG.warn("Failed to read patch file ${patchFile.name}", e)
        }
    }

    /**
     * Write the final document content to the ack-content sidecar file via FFI.
     * Called after every successful apply so the CLI binary can use it as the
     * snapshot source instead of the 200ms sleep + re-read heuristic.
     * Keyed by patch_id (not file path) — all path logic lives in Rust.
     */
    private fun writeAckContent(patchId: String?, content: String, filePath: String? = null) {
        if (patchId == null) return
        val root = filePath?.let { resolveRootFor(it) } ?: project.basePath ?: return
        val lib = AgentDocLib.get() ?: run {
            LOG.debug("[ack-content] FFI unavailable, skipping ack-content write")
            return
        }
        if (!lib.agent_doc_write_ack_content(root, patchId, content)) {
            LOG.warn("[ack-content] FFI write_ack_content returned false for patch_id $patchId")
        }
    }

    private fun awaitIdleBeforeDocumentMutation(filePath: String, operation: String): Boolean {
        val lib = AgentDocLib.get() ?: return true
        val idle = lib.agent_doc_await_idle(filePath, TypingTracker.DEBOUNCE_MS, 5_000)
        if (!idle) {
            LOG.warn("[patch-watcher] typing debounce timed out before $operation for $filePath")
        }
        return idle
    }

    private fun schedulePatchRetry(patchFile: File, reason: String) {
        val key = try {
            patchFile.canonicalPath
        } catch (_: Exception) {
            patchFile.absolutePath
        }
        if (!scheduledPatchRetries.add(key)) return
        LOG.info("[patch-watcher] deferring ${patchFile.name}: $reason")
        com.intellij.util.concurrency.AppExecutorUtil.getAppExecutorService().submit {
            try {
                Thread.sleep(TypingTracker.DEBOUNCE_MS)
                if (running && patchFile.exists()) {
                    processPatchFile(patchFile)
                }
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
            } finally {
                scheduledPatchRetries.remove(key)
            }
        }
    }

    private fun scheduleRepositionRetry(filePath: String, boundaryId: String?, preserveHead: Boolean) {
        val key = "$filePath|${boundaryId ?: ""}|$preserveHead"
        if (!scheduledRepositionRetries.add(key)) return
        com.intellij.util.concurrency.AppExecutorUtil.getAppExecutorService().submit {
            try {
                Thread.sleep(TypingTracker.DEBOUNCE_MS)
                if (running) {
                    repositionBoundaryViaDocument(filePath, boundaryId, preserveHead)
                }
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
            } finally {
                scheduledRepositionRetries.remove(key)
            }
        }
    }

    /**
     * Check if --force-disk claimed this patch via FFI.
     * Returns true if the sentinel exists (patch already applied by CLI disk write).
     * Sentinel is deleted by the Rust side on check (one-time use).
     */
    private fun isClaimedByForceDisk(patchId: String?, filePath: String? = null): Boolean {
        if (patchId == null) return false
        val root = filePath?.let { resolveRootFor(it) } ?: project.basePath ?: return false
        val lib = AgentDocLib.get() ?: return false
        return lib.agent_doc_is_claimed_by_force_disk(root, patchId).also { claimed ->
            if (claimed) LOG.info("[patch-watcher] dedup: patch_id $patchId claimed by force-disk — skipping apply")
        }
    }

    private fun applyPatch(patch: IpcPatch): Boolean {
        lastApplyWasNoOp = false
        lastApplyWasDeferredForConflict = false
        lastApplyRejectedConflictCancel = false

        var targetFile = LocalFileSystem.getInstance().findFileByPath(patch.file)
        if (targetFile == null) {
            // Retry once after a short delay — file might not be indexed yet
            Thread.sleep(200)
            LocalFileSystem.getInstance().refresh(false)
            targetFile = LocalFileSystem.getInstance().findFileByPath(patch.file)
        }
        if (targetFile == null) {
            LOG.warn("Target file not found: ${patch.file}")
            return false
        }

        // Refresh VirtualFile from disk to ensure we have latest content
        // (boundary markers may have been written to disk by agent-doc boundary)
        targetFile.refresh(false, false)

        // Reload document from disk if it was externally modified
        val fdm = FileDocumentManager.getInstance()
        val document = fdm.getDocument(targetFile)

        if (document == null) {
            // File not open in editor — apply patches via VFS (no Document API needed).
            // This avoids the "externally modified" dialog for background tabs.
            LOG.info("No document for ${patch.file}, applying via VFS")
            return applyPatchViaVfs(targetFile, patch)
        }

        if (hasPendingMemoryDiskConflict(targetFile)) {
            markPatchDeferredForMemoryDiskConflict(patch)
            lastApplyWasDeferredForConflict = true
            LOG.warn("[patch-watcher] File Cache Conflict pending for ${patch.file}; deferring patch until user resolves dialog")
            return false
        }

        val wasDeferredForConflict = wasPatchDeferredForMemoryDiskConflict(patch)
        if (memoryDiskConflictCancelLikelyUtil(
                wasDeferredForConflict,
                fdm.isDocumentUnsaved(document),
                document.modificationStamp,
                targetFile.modificationStamp,
            )
        ) {
            lastApplyRejectedConflictCancel = true
            LOG.warn("[patch-watcher] File Cache Conflict kept memory changes for ${patch.file}; rejecting patch without mutating document")
            return false
        }
        if (wasDeferredForConflict) {
            clearPatchDeferredForMemoryDiskConflict(patch)
        }

        if (fdm.isDocumentUnsaved(document)) {
            // Document has unsaved changes — don't reload (preserve user edits)
        } else {
            // Reload from disk to pick up boundary changes from agent-doc boundary
            fdm.reloadFromDisk(document)
        }

        // Capture caret offset before write action (for cursor-aware append ordering)
        val caretOffset: Int? = try {
            val editors = FileEditorManager.getInstance(project).getAllEditors(targetFile)
            val textEditor = editors.firstOrNull { it is TextEditor } as? TextEditor
            textEditor?.editor?.caretModel?.offset
        } catch (_: Exception) {
            null
        }

        // Compute the patched result OUTSIDE the write action to avoid
        // blocking the EDT for no-op patches. Only acquire the write lock
        // if the content actually changed.
        val content = document.text
        val proof = EditorApplyProof(content, document.modificationStamp)

        if (!patch.fullContent.isNullOrEmpty()) {
            LOG.warn("[patch-watcher] full-content IPC is disabled; rejecting patch_id ${patch.patchId} for ${patch.file}")
            return false
        }

        // Component-based patching (template/stream-mode documents)
        var result = content
        val diskContent = try {
            String(targetFile.contentsToByteArray(), targetFile.charset)
        } catch (_: Exception) {
            null
        }
        if (patchReplayAlreadyPresentUtil(
                patch,
                listOfNotNull(content, diskContent),
            ) { payload -> NativePatching.patchContentAlreadyCommitted(patch.file, payload) }
        ) {
            LOG.info("[patch-watcher] dedup: response patch_id ${patch.patchId} already present in live disk/committed content — skipping stale replay")
            writeAckContent(patch.patchId, diskContent ?: content, patch.file)
            lastApplyWasNoOp = true
            return true
        }

        // Apply frontmatter patch first (before component patches)
        if (!patch.frontmatter.isNullOrBlank()) {
            result = NativePatching.mergeFrontmatter(result, patch.frontmatter)
                ?: applyFrontmatterPatchKotlin(result, patch.frontmatter)
        }

        // Converge the queue opening-tag `auto` attribute if requested. A content
        // patch cannot change an opening-tag attribute, so this is the only seam
        // that converges a live buffer's queue tag after a halt
        // (#adoc-queue-ipc-buffer-divergence).
        if (patch.queueAuto != null) {
            result = NativePatching.convergeQueueAuto(result, patch.queueAuto) ?: result
        }

        for (p in patch.patches) {
            val effectiveBoundaryId = if (p.ensureBoundary && p.boundaryId == null) {
                findBoundaryInComponent(result, p.component)
            } else {
                p.boundaryId
            }
            result = applyComponentPatchNative(result, p.component, p.content, caretOffset, effectiveBoundaryId)
        }

        // Apply unmatched content to exchange or output component
        if (patch.unmatched.isNotBlank()) {
            val exchangeResult = applyComponentPatchNative(result, "exchange", patch.unmatched, caretOffset)
            result = if (exchangeResult != result) exchangeResult
                else applyComponentPatchNative(result, "output", patch.unmatched, caretOffset)
        }

        // Reposition boundary to end of exchange if requested
        if (patch.repositionBoundary) {
            result = if (patch.preserveHead) {
                NativePatching.repositionBoundaryToEndPreserveHead(result, patch.repositionBoundaryId)
                    ?: repositionBoundaryToEnd(result, "exchange", patch.repositionBoundaryId, preserveHead = true)
                    ?: result
            } else {
                NativePatching.repositionBoundaryToEnd(result, patch.repositionBoundaryId)
                    ?: repositionBoundaryToEnd(result, "exchange", patch.repositionBoundaryId)
                    ?: result
            }
        }

        // Normalize after patches and boundary reposition so prompts typed after
        // the prior boundary are included in the user region seen by the sidecar.
        if (patch.normalizePrefixLines.isNotEmpty()) {
            result = normalizeExchangePrefixes(result, patch.normalizePrefixLines)
        }
        result = annotateExchangeHeadingsAgainstBaselineUtil(result, "exchange", content) ?: result
        result = NativePatching.normalizeTemplateStructure(result) ?: run {
            LOG.warn("Patch rejected by native template-structure guard for ${patch.file}")
            return false
        }

        if (result == content) {
            LOG.warn("Patch produced no changes for ${patch.file}")
            writeAckContent(patch.patchId, document.text, patch.file)
            lastApplyWasNoOp = true
            return true
        }

        if (!applyProofStillCurrent(proof, document, patch.file, "component patch")) {
            return false
        }
        var wrote = false
        WriteCommandAction.runWriteCommandAction(project, "Agent Doc Patch", null, {
            if (!editorApplyProofStillCurrentUtil(proof, document.text, document.modificationStamp)) {
                LOG.warn("[patch-watcher] stale editor generation during component patch for ${patch.file}; rejecting")
                return@runWriteCommandAction
            }
            LOG.info(
                documentMutationDiagnosticUtil(
                    "applyPatch.component", patch.file, patch.patchId, "document_api",
                    content, result, document.modificationStamp, true,
                )
            )
            // #jb-settext-payload-log: the diagnostic above logs only hashes +
            // lengths. To capture the exact corrupting payload for the IPC
            // duplication family, log the full setText payload at debug level
            // (only when `#com.github.btakita.agentdoc` debug logging is on).
            if (LOG.isDebugEnabled) {
                LOG.debug(
                    "[patch-watcher] setText payload (applyPatch.component) for ${patch.file} patchId=${patch.patchId} (${result.length} chars):\n$result"
                )
            }
            document.setText(result)
            wrote = true
            LOG.info("Patch applied to ${patch.file} (${result.length - content.length} chars changed)")
        })
        if (!wrote) {
            return false
        }

        // Save the document to disk (so snapshot can read it)
        FileDocumentManager.getInstance().saveDocument(document)
        writeAckContent(patch.patchId, document.text, patch.file)
        // Note: do NOT call agent_doc_commit here. The plugin committing within the IPC
        // window races with the skill's `agent-doc commit` call, causing the binary commit
        // to be a no-op (FFI already committed). The binary's git::commit handles boundary
        // markers and HEAD repositioning; the FFI commit skips all of that. The preflight
        // sweep (Fix 5) handles missed commits as a backstop for interrupted sessions.
        return true
    }

    private fun patchConflictKey(patch: IpcPatch): String =
        patch.patchId ?: patch.file

    private fun markPatchDeferredForMemoryDiskConflict(patch: IpcPatch) {
        memoryDiskConflictDeferredPatchIds.add(patchConflictKey(patch))
    }

    private fun wasPatchDeferredForMemoryDiskConflict(patch: IpcPatch): Boolean =
        memoryDiskConflictDeferredPatchIds.contains(patchConflictKey(patch))

    private fun clearPatchDeferredForMemoryDiskConflict(patch: IpcPatch) {
        memoryDiskConflictDeferredPatchIds.remove(patchConflictKey(patch))
    }

    private fun hasPendingMemoryDiskConflict(targetFile: VirtualFile): Boolean {
        val fdm = FileDocumentManager.getInstance()
        return try {
            val resolverField = findFieldInHierarchy(fdm.javaClass, "myConflictResolver")
                ?: return false
            resolverField.isAccessible = true
            val resolver = resolverField.get(fdm) ?: return false
            val hasConflict = findMethodInHierarchy(resolver.javaClass, "hasConflict", VirtualFile::class.java)
                ?: return false
            hasConflict.isAccessible = true
            hasConflict.invoke(resolver, targetFile) as? Boolean ?: false
        } catch (e: Exception) {
            if (!memoryDiskConflictReflectionWarned) {
                memoryDiskConflictReflectionWarned = true
                LOG.warn("[patch-watcher] unable to inspect IntelliJ File Cache Conflict state; proceeding without conflict deferral", e)
            }
            false
        }
    }

    private fun findFieldInHierarchy(type: Class<*>, name: String): Field? {
        var current: Class<*>? = type
        while (current != null) {
            try {
                return current.getDeclaredField(name)
            } catch (_: NoSuchFieldException) {
                current = current.superclass
            }
        }
        return null
    }

    private fun findMethodInHierarchy(type: Class<*>, name: String, vararg parameterTypes: Class<*>): Method? {
        var current: Class<*>? = type
        while (current != null) {
            try {
                return current.getDeclaredMethod(name, *parameterTypes)
            } catch (_: NoSuchMethodException) {
                current = current.superclass
            }
        }
        return null
    }

    private fun applyProofStillCurrent(
        proof: EditorApplyProof,
        document: com.intellij.openapi.editor.Document,
        filePath: String,
        operation: String,
    ): Boolean {
        if (editorApplyProofStillCurrentUtil(proof, document.text, document.modificationStamp)) {
            return true
        }
        LOG.warn("[patch-watcher] stale editor generation before $operation for $filePath; rejecting patch")
        return false
    }

    /**
     * Apply patches via VFS for files not open in the editor.
     * Reads content from disk, applies patches in memory, writes back through VFS.
     * This avoids the "externally modified" dialog since it goes through IntelliJ's VFS layer.
     */
    private fun applyPatchViaVfs(targetFile: com.intellij.openapi.vfs.VirtualFile, patch: IpcPatch): Boolean {
        try {
            val content = String(targetFile.contentsToByteArray(), targetFile.charset)

            if (!patch.fullContent.isNullOrEmpty()) {
                LOG.warn("[patch-watcher] VFS full-content IPC is disabled; rejecting patch_id ${patch.patchId} for ${patch.file}")
                return false
            }

            // Component-based patching
            var result = content
            if (patchReplayAlreadyPresentUtil(
                    patch,
                    listOf(content),
                ) { payload -> NativePatching.patchContentAlreadyCommitted(patch.file, payload) }
            ) {
                LOG.info("[patch-watcher] dedup: VFS response patch_id ${patch.patchId} already present in disk/committed content — skipping stale replay")
                writeAckContent(patch.patchId, content, patch.file)
                lastApplyWasNoOp = true
                return true
            }

            if (!patch.frontmatter.isNullOrBlank()) {
                result = NativePatching.mergeFrontmatter(result, patch.frontmatter)
                    ?: applyFrontmatterPatchKotlin(result, patch.frontmatter)
            }

            // Converge the queue opening-tag `auto` attribute if requested
            // (#adoc-queue-ipc-buffer-divergence).
            if (patch.queueAuto != null) {
                result = NativePatching.convergeQueueAuto(result, patch.queueAuto) ?: result
            }

            for (p in patch.patches) {
                val effectiveBoundaryId = if (p.ensureBoundary && p.boundaryId == null) {
                    findBoundaryInComponent(result, p.component)
                } else {
                    p.boundaryId
                }
                result = applyComponentPatchNative(result, p.component, p.content, null, effectiveBoundaryId)
            }

            if (patch.unmatched.isNotBlank()) {
                val exchangeResult = applyComponentPatchNative(result, "exchange", patch.unmatched, null)
                result = if (exchangeResult != result) exchangeResult
                    else applyComponentPatchNative(result, "output", patch.unmatched, null)
            }

            if (patch.repositionBoundary) {
                result = if (patch.preserveHead) {
                    NativePatching.repositionBoundaryToEndPreserveHead(result, patch.repositionBoundaryId)
                        ?: repositionBoundaryToEnd(result, "exchange", patch.repositionBoundaryId, preserveHead = true)
                        ?: result
                } else {
                    NativePatching.repositionBoundaryToEnd(result, patch.repositionBoundaryId)
                        ?: repositionBoundaryToEnd(result, "exchange", patch.repositionBoundaryId)
                        ?: result
                }
            }

            // Normalize after patches and boundary reposition so prompts typed
            // after the prior boundary are included in the user region.
            if (patch.normalizePrefixLines.isNotEmpty()) {
                result = normalizeExchangePrefixes(result, patch.normalizePrefixLines)
            }
            result = annotateExchangeHeadingsAgainstBaselineUtil(result, "exchange", content) ?: result
            result = NativePatching.normalizeTemplateStructure(result) ?: run {
                LOG.warn("VFS patch rejected by native template-structure guard for ${patch.file}")
                return false
            }

            if (result != content) {
                var wrote = false
                ApplicationManager.getApplication().runWriteAction {
                    // Re-read disk immediately before the whole-buffer write. `result`
                    // was computed from `content` captured at the top of this apply; if
                    // disk changed since (file opened + typed into, or another writer),
                    // fail closed instead of clobbering the newer bytes
                    // (#ipcfullprompt-recur2). The binary retries from the patch file.
                    val currentDisk = try {
                        String(targetFile.contentsToByteArray(), targetFile.charset)
                    } catch (_: Exception) {
                        content
                    }
                    if (!vfsDiskContentStillCurrentUtil(content, currentDisk)) {
                        LOG.warn("[patch-watcher] stale disk content before VFS write for ${patch.file}; rejecting patch_id ${patch.patchId}")
                        return@runWriteAction
                    }
                    targetFile.setBinaryContent(result.toByteArray(targetFile.charset))
                    wrote = true
                }
                if (!wrote) {
                    return false
                }
                LOG.info("VFS patch applied to ${patch.file} (${result.length - content.length} chars changed)")
            } else {
                LOG.warn("VFS patch produced no changes for ${patch.file}")
                lastApplyWasNoOp = true
            }
            writeAckContent(patch.patchId, result, patch.file)
            return true
        } catch (e: Exception) {
            LOG.warn("Failed to apply patch via VFS for ${patch.file}", e)
            return false
        }
    }

    /**
     * Apply a component patch, preferring native FFI with Kotlin fallback.
     *
     * The native library handles code block detection, attribute parsing,
     * and mode resolution identically to the CLI — eliminating duplicated logic.
     */
    private fun applyComponentPatchNative(doc: String, component: String, content: String, caretOffset: Int? = null, boundaryId: String? = null): String {
        val mode = extractComponentMode(doc, component)
        if (mode == "append" && appendPatchAlreadyPresentUtil(doc, component, content)) {
            LOG.info("Patch dedup: append content already present in $component")
            return doc
        }

        // Boundary marker takes precedence for append mode
        if (mode == "append" && !boundaryId.isNullOrBlank()) {
            val nativeResult = NativePatching.applyComponentPatchWithBoundary(doc, component, content, mode, boundaryId)
            if (nativeResult != null) return nativeResult
            // Kotlin fallback for boundary-based append
            val kotlinResult = applyComponentPatchWithBoundary(doc, component, content, boundaryId)
            if (kotlinResult != null) return kotlinResult
        }

        // When no boundary exists for append mode, insert before the close tag
        // (end-of-exchange). Never use caret-based insertion — it places content
        // at the cursor position which is unpredictable.
        if (mode == "append") {
            val result = applyComponentPatchEndOfExchange(doc, component, content)
            if (result != null) return result
        }

        return NativePatching.applyComponentPatch(doc, component, content, mode)
            ?: applyComponentPatchKotlin(doc, component, content)
    }

    /**
     * Add `❯ ` prefix to specific lines within the exchange component.
     * Delegates to [normalizeExchangePrefixesUtil] for testability.
     */
    private fun normalizeExchangePrefixes(doc: String, lines: List<String>): String =
        normalizeExchangePrefixesUtil(doc, lines)

    /**
     * Find a boundary marker UUID inside a component's content.
     * Returns the boundary ID if found, null otherwise.
     */
    private fun findBoundaryInComponent(doc: String, component: String): String? {
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val closeTag = "<!-- /agent:$component -->"
        val boundaryPattern = Regex("""<!-- agent:boundary:([a-z0-9][a-z0-9:-]*) -->""")

        val openMatch = openPattern.find(doc) ?: return null
        val contentStart = openMatch.range.last + 1
        val closeIdx = doc.indexOf(closeTag, contentStart)
        if (closeIdx < 0) return null

        val componentContent = doc.substring(contentStart, closeIdx)
        val boundaryMatch = boundaryPattern.findAll(componentContent).lastOrNull()
        return boundaryMatch?.groupValues?.getOrNull(1)
    }

    /**
     * Extract the patch mode for a component from its inline attributes.
     * Returns "replace" as default if not specified.
     */
    private fun extractComponentMode(doc: String, component: String): String {
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val match = openPattern.find(doc) ?: return defaultMode(component)
        val attrs = match.groupValues.getOrNull(1) ?: return defaultMode(component)
        val patchMatch = Regex("""patch=(\w+)""").find(attrs)
        val modeMatch = Regex("""mode=(\w+)""").find(attrs)
        return patchMatch?.groupValues?.getOrNull(1)
            ?: modeMatch?.groupValues?.getOrNull(1) ?: defaultMode(component)
    }

    /** Built-in default modes matching the Rust binary's `default_mode()`. */
    private fun defaultMode(component: String): String {
        return when (component) {
            "exchange", "findings" -> "append"
            else -> "replace"
        }
    }

    /**
     * Kotlin fallback: merge YAML key/value pairs into the document's frontmatter.
     * Parses the existing frontmatter, updates matching keys, preserves others.
     */
    private fun applyFrontmatterPatchKotlin(doc: String, yamlFields: String): String {
        if (!doc.startsWith("---\n")) return doc

        val endIdx = doc.indexOf("\n---\n", 4)
        if (endIdx < 0) return doc

        val existingYaml = doc.substring(4, endIdx)
        val body = doc.substring(endIdx + 5) // skip \n---\n

        // Parse existing frontmatter as key/value pairs (preserve order)
        val existing = LinkedHashMap<String, String>()
        for (line in existingYaml.lines()) {
            val colonIdx = line.indexOf(':')
            if (colonIdx > 0) {
                val key = line.substring(0, colonIdx).trim()
                val value = line.substring(colonIdx + 1).trim()
                existing[key] = value
            }
        }

        // Merge new fields
        for (line in yamlFields.lines()) {
            val colonIdx = line.indexOf(':')
            if (colonIdx > 0) {
                val key = line.substring(0, colonIdx).trim()
                val value = line.substring(colonIdx + 1).trim()
                if (key.isNotEmpty()) {
                    existing[key] = value
                }
            }
        }

        // Rebuild frontmatter
        val newYaml = existing.entries.joinToString("\n") { "${it.key}: ${it.value}" }
        return "---\n$newYaml\n---\n$body"
    }

    /**
     * Cursor-aware append: insert patch content before the caret position
     * when the caret is inside the target component. This ensures agent
     * responses appear above where the user is typing.
     *
     * Returns null if the caret is NOT inside the component (fall back to normal append).
     */
    private fun applyComponentPatchWithCaret(doc: String, component: String, content: String, caretOffset: Int): String? {
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val closeTag = "<!-- /agent:$component -->"

        val codeRanges = findCodeBlockRanges(doc)

        val openMatch = openPattern.findAll(doc).firstOrNull { match ->
            codeRanges.none { range -> match.range.first >= range.first && match.range.first < range.second }
        } ?: return null

        val contentStart = openMatch.range.last + 1

        var searchFrom = contentStart
        var closeIdx: Int
        while (true) {
            closeIdx = doc.indexOf(closeTag, searchFrom)
            if (closeIdx < 0) return null
            if (codeRanges.none { range -> closeIdx >= range.first && closeIdx < range.second }) break
            searchFrom = closeIdx + closeTag.length
        }

        // Check if caret is inside this component
        if (caretOffset < contentStart || caretOffset > closeIdx) return null

        // Insert patch content at the line boundary before the caret
        val insertAt = doc.lastIndexOf('\n', caretOffset - 1).let {
            if (it >= contentStart) it + 1 else contentStart
        }

        val before = doc.substring(0, insertAt)
        val after = doc.substring(insertAt)
        return before + content.trimEnd() + "\n" + after
    }

    /**
     * Kotlin fallback for boundary-aware append.
     *
     * Finds `<!-- agent:boundary:ID -->` inside the component and inserts
     * response content at that position, replacing the marker.
     * Returns null if boundary marker is not found.
     */
    private fun applyComponentPatchWithBoundary(doc: String, component: String, content: String, boundaryId: String): String? {
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val closeTag = "<!-- /agent:$component -->"
        val boundaryMarker = "<!-- agent:boundary:$boundaryId -->"

        val codeRanges = findCodeBlockRanges(doc)

        val openMatch = openPattern.findAll(doc).firstOrNull { match ->
            codeRanges.none { range -> match.range.first >= range.first && match.range.first < range.second }
        } ?: return null

        val contentStart = openMatch.range.last + 1

        var searchFrom = contentStart
        var closeIdx: Int
        while (true) {
            closeIdx = doc.indexOf(closeTag, searchFrom)
            if (closeIdx < 0) return null
            if (codeRanges.none { range -> closeIdx >= range.first && closeIdx < range.second }) break
            searchFrom = closeIdx + closeTag.length
        }

        // Find boundary marker inside component
        val boundaryIdx = doc.indexOf(boundaryMarker, contentStart)
        if (boundaryIdx < 0 || boundaryIdx >= closeIdx) return null

        // Find start of the boundary marker line
        val lineStart = doc.lastIndexOf('\n', boundaryIdx - 1).let {
            if (it >= contentStart) it + 1 else contentStart
        }

        // Find end of the boundary marker line (including trailing newline)
        val markerEnd = boundaryIdx + boundaryMarker.length
        val lineEnd = if (markerEnd < closeIdx && doc.getOrNull(markerEnd) == '\n') markerEnd + 1 else markerEnd

        // Replace the boundary marker with response content + new boundary.
        // The boundary is consumed and re-inserted at end of exchange, matching
        // the binary's post-patch behavior in apply_patches_with_overrides().
        val newBoundaryId = java.util.UUID.randomUUID().toString().substring(0, 8)
        val newBoundary = "<!-- agent:boundary:$newBoundaryId -->"
        val before = doc.substring(0, lineStart)
        val after = doc.substring(lineEnd.coerceAtMost(closeIdx))
        return before + content.trimEnd() + "\n" + newBoundary + "\n" + after
    }

    /**
     * End-of-exchange append: insert content right before the close tag.
     * Used when no boundary marker exists. Inserts a new boundary after
     * the content so future cycles can use boundary-based insertion.
     */
    private fun applyComponentPatchEndOfExchange(doc: String, component: String, content: String): String? {
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val closeTag = "<!-- /agent:$component -->"

        val codeRanges = findCodeBlockRanges(doc)

        val openMatch = openPattern.findAll(doc).firstOrNull { match ->
            codeRanges.none { range -> match.range.first >= range.first && match.range.first < range.second }
        } ?: return null

        val contentStart = openMatch.range.last + 1

        var searchFrom = contentStart
        var closeIdx: Int
        while (true) {
            closeIdx = doc.indexOf(closeTag, searchFrom)
            if (closeIdx < 0) return null
            if (codeRanges.none { range -> closeIdx >= range.first && closeIdx < range.second }) break
            searchFrom = closeIdx + closeTag.length
        }

        // Insert content + new boundary right before the close tag
        val newBoundaryId = java.util.UUID.randomUUID().toString().substring(0, 8)
        val newBoundary = "<!-- agent:boundary:$newBoundaryId -->"
        val before = doc.substring(0, closeIdx)
        val after = doc.substring(closeIdx)
        val prefix = if (before.endsWith("\n")) "" else "\n"
        return before + prefix + content.trimEnd() + "\n" + newBoundary + "\n" + after
    }

    /**
     * Kotlin fallback: replace content between component markers.
     * Used when native library is unavailable.
     */
    private fun applyComponentPatchKotlin(doc: String, component: String, content: String): String {
        // Match open tag with optional attributes: <!-- agent:name ... -->
        val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
        val closeTag = "<!-- /agent:$component -->"

        val codeRanges = findCodeBlockRanges(doc)

        // Find the first open tag match that is NOT inside a fenced code block
        val openMatch = openPattern.findAll(doc).firstOrNull { match ->
            codeRanges.none { range -> match.range.first >= range.first && match.range.first < range.second }
        } ?: return doc

        val contentStart = openMatch.range.last + 1

        // Find close tag that is also NOT inside a fenced code block
        var searchFrom = contentStart
        var closeIdx: Int
        while (true) {
            closeIdx = doc.indexOf(closeTag, searchFrom)
            if (closeIdx < 0) return doc
            if (codeRanges.none { range -> closeIdx >= range.first && closeIdx < range.second }) break
            searchFrom = closeIdx + closeTag.length
        }

        // Check mode from inline attributes: patch= takes precedence, mode= as fallback
        val attrs = openMatch.groupValues.getOrNull(1) ?: ""
        val patchMatch = Regex("""patch=(\w+)""").find(attrs)
        val modeMatch = Regex("""mode=(\w+)""").find(attrs)
        val mode = patchMatch?.groupValues?.getOrNull(1)
            ?: modeMatch?.groupValues?.getOrNull(1) ?: "replace"

        val before = doc.substring(0, contentStart)
        val existingContent = doc.substring(contentStart, closeIdx)
        val after = doc.substring(closeIdx)

        return when (mode) {
            "append" -> before + existingContent.trimEnd() + "\n" + content.trimEnd() + "\n" + after
            "prepend" -> before + "\n" + content.trimEnd() + "\n" + existingContent.trimStart() + after
            else -> before + "\n" + content.trimEnd() + "\n" + after // replace
        }
    }

    private fun findCodeBlockRanges(doc: String) = findCodeBlockRangesUtil(doc)

    /** Debounce window for VCS refresh signals (ms). Multiple commits within this window coalesce into one refresh. */
    private val vcsRefreshAlarm = Alarm(Alarm.ThreadToUse.SWING_THREAD, this)
    private val VCS_REFRESH_DEBOUNCE_MS = 500

    /**
     * Trigger a debounced VCS refresh so git gutter updates after an external commit.
     * Called when `agent-doc commit` writes a `vcs-refresh.signal` file.
     * Multiple signals within 500ms coalesce into a single refresh.
     */
    private fun refreshVcs() {
        vcsRefreshAlarm.cancelAllRequests()
        vcsRefreshAlarm.addRequest({
            try {
                // Refresh VFS first so IntelliJ sees the new git state on disk
                LocalFileSystem.getInstance().refresh(true)
                // Then mark VCS dirty to re-compute git gutter annotations
                VcsDirtyScopeManager.getInstance(project).markEverythingDirty()
                LOG.info("[vcs] Triggered VFS + VCS refresh after external commit (debounced)")
            } catch (e: Exception) {
                LOG.warn("[vcs] Failed to refresh VCS state", e)
            }
        }, VCS_REFRESH_DEBOUNCE_MS)
    }

    private fun repositionBoundaryToEnd(
        doc: String,
        component: String,
        boundaryId: String? = null,
        preserveHead: Boolean = false,
    ) = repositionBoundaryToEndUtil(doc, component, boundaryId, preserveHead)

    override fun dispose() {
        running = false
        val lib = AgentDocLib.get()
        for (state in rootStates.values) {
            state.watchThread?.interrupt()
            state.watchThread = null
            try { lib?.agent_doc_stop_ipc_listener(state.root) } catch (_: Exception) {}
            state.ipcCallback = null
        }
        rootStates.clear()
    }

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(PatchWatcher::class.java)
        private val instances = mutableMapOf<Project, PatchWatcher>()

        const val APPLY_FAILED = 0
        const val APPLY_APPLIED = 1
        const val APPLY_ALREADY_APPLIED = 2

        /**
         * Check if a patch file is stale (already applied in a previous cycle).
         *
         * Compares the patch file's modification time against the snapshot for the same
         * document. If the snapshot is NEWER than the patch file, the binary already saved
         * a snapshot after applying this patch — the file is stale.
         *
         * Boundary-ID checks were the original approach but are unreliable: when a patch is
         * applied, its boundary marker is consumed and replaced. So checking "is boundary in
         * doc" returns false for BOTH "not yet applied" AND "already applied" after a
         * subsequent cycle. Snapshot timestamp is the correct signal.
         */
        fun isPatchAlreadyApplied(patch: IpcPatch, patchFile: File): Boolean {
            // Walk up from the document to find .agent-doc/ — mirrors find_project_root in Rust.
            var dir: File? = File(patch.file).parentFile
            while (dir != null) {
                val agentDocDir = File(dir, ".agent-doc")
                if (agentDocDir.isDirectory) {
                    val docHash = docHash(patch.file)
                    val snapshotFile = File(agentDocDir, "snapshots/$docHash.md")
                    if (snapshotFile.exists() && snapshotFile.lastModified() > patchFile.lastModified()) {
                        return true
                    }
                    break
                }
                dir = dir.parentFile
            }
            return false
        }

        /** Compute SHA256 hex of a path string — mirrors snapshot::doc_hash in the Rust binary. */
        fun docHash(path: String): String {
            val digest = MessageDigest.getInstance("SHA-256")
            val bytes = digest.digest(path.toByteArray(Charsets.UTF_8))
            return bytes.joinToString("") { "%02x".format(it) }
        }

        /** Compute SHA256 hex of content bytes — mirrors debounce::content_hash in the Rust binary. */
        fun contentHash(content: String): String {
            val digest = MessageDigest.getInstance("SHA-256")
            val bytes = digest.digest(content.toByteArray(Charsets.UTF_8))
            return bytes.joinToString("") { "%02x".format(it) }
        }

        /**
         * Apply-side generation fence (`#late-ipc-patch-plugin-apply-fence`).
         *
         * A queued file patch tagged by the binary
         * ([queue_file_ipc_reposition_boundary] in write.rs) carries a generation
         * token: `cycle_id` (the cycle that produced it) and `baseline_hash`
         * (SHA-256 of the live doc the patch targeted). A LATE applier — this
         * watcher, possibly running minutes after the producing cycle committed —
         * must DROP a patch whose generation is already superseded instead of
         * re-applying it and re-materializing a duplicate `### Re:` block:
         *
         *  - **cycle already committed:** the document's persisted `cycle_state`
         *    advanced the SAME `cycle_id` this patch was produced in to
         *    `committed`. The cycle is closed; replaying its reposition can only
         *    duplicate (mirrors the write-side `try_ipc` `already_committed` guard).
         *  - **baseline drift:** the live doc no longer hashes to `baseline_hash`.
         *    A later cycle rewrote the doc, so the patch targets a stale baseline.
         *
         * Fails OPEN: returns false on any IO/parse error or when the patch
         * carries no generation token, so a legitimate patch is never dropped
         * because state could not be read.
         */
        fun isPatchGenerationSuperseded(patch: IpcPatch, liveContent: String?): Boolean {
            // Baseline drift: live doc moved on from the baseline this patch targeted.
            if (patch.baselineHash != null && liveContent != null) {
                if (contentHash(liveContent) != patch.baselineHash) {
                    LOG.info(
                        "[patch-watcher] generation fence: baseline drift (live doc moved on from queued baseline) for ${patch.file}",
                    )
                    return true
                }
            }
            // Cycle already committed: the producing cycle is closed.
            if (patch.cycleId != null && cycleAlreadyCommitted(patch.file, patch.cycleId)) {
                LOG.info(
                    "[patch-watcher] generation fence: cycle ${patch.cycleId} already committed for ${patch.file}",
                )
                return true
            }
            return false
        }

        /** EDT/file-thread variant that reads the live doc content from disk. */
        private fun isPatchGenerationSuperseded(patch: IpcPatch): Boolean {
            if (patch.baselineHash == null && patch.cycleId == null) return false
            val liveContent = try {
                val f = File(patch.file)
                if (f.exists()) f.readText() else null
            } catch (e: Exception) {
                LOG.debug("[patch-watcher] generation fence: could not read live doc ${patch.file}: ${e.message}")
                null
            }
            return isPatchGenerationSuperseded(patch, liveContent)
        }

        /**
         * Mirror of `flow::closeout::cycle_already_committed`: true when the
         * document's persisted cycle_state has the SAME `cycle_id` and phase
         * `committed`. Reads `.agent-doc/state/cycles/<doc-hash>.json`.
         */
        fun cycleAlreadyCommitted(docPath: String, cycleId: String): Boolean {
            var dir: File? = File(docPath).parentFile
            while (dir != null) {
                val agentDocDir = File(dir, ".agent-doc")
                if (agentDocDir.isDirectory) {
                    val stateFile = File(agentDocDir, "state/cycles/${docHash(docPath)}.json")
                    if (!stateFile.exists()) return false
                    return try {
                        val root = com.google.gson.JsonParser.parseString(stateFile.readText()).asJsonObject
                        val stateCycleId = root.get("cycle_id")?.asString
                        val phase = root.get("phase")?.asString
                        stateCycleId == cycleId && phase == "committed"
                    } catch (e: Exception) {
                        LOG.debug("[patch-watcher] generation fence: could not read cycle_state for $docPath: ${e.message}")
                        false
                    }
                }
                dir = dir.parentFile
            }
            return false
        }

        fun getInstance(project: Project): PatchWatcher {
            return instances.getOrPut(project) {
                PatchWatcher(project).also { it.start() }
            }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }
    }
}

/** Parsed IPC patch payload. */
data class IpcPatch(
    val file: String,
    val patches: List<ComponentPatch>,
    val unmatched: String,
    val frontmatter: String?,
    val fullContent: String?,
    val repositionBoundary: Boolean = false,
    val repositionBoundaryId: String? = null,
    /** Preserve transient `(HEAD)` response markers during post-commit editor cleanup. */
    val preserveHead: Boolean = false,
    /** Lines whose plain text should be prefixed with `❯ ` in the exchange component. */
    val normalizePrefixLines: List<String> = emptyList(),
    /** UUID identifying this patch — used for ack-content sidecar and claimed-patches sentinel. */
    val patchId: String? = null,
    /** Historical source-buffer proof for disabled fullContent payloads. */
    val expectedContentHash: String? = null,
    val expectedContentLen: Int? = null,
    /**
     * Desired state of the `agent:queue` opening-tag `auto` attribute for queue
     * convergence patches (`#adoc-queue-ipc-buffer-divergence`). `null` means the
     * patch does not converge the queue tag; `true`/`false` converge the live
     * buffer's tag via the `agent_doc_converge_queue_auto` FFI seam — a content
     * patch alone cannot add/remove an opening-tag attribute.
     */
    val queueAuto: Boolean? = null,
    /**
     * Generation token (`#late-ipc-patch-plugin-apply-fence`). The cycle that
     * produced this patch. A LATE applier drops the patch when the document's
     * persisted `cycle_state` already advanced this same cycle to `committed`.
     */
    val cycleId: String? = null,
    /**
     * Generation token (`#late-ipc-patch-plugin-apply-fence`). SHA-256 hex of
     * the live document the patch targeted at queue time
     * (`debounce::content_hash`). A LATE applier drops the patch when the live
     * doc no longer hashes to this value — the doc moved on to a later cycle.
     */
    val baselineHash: String? = null,
)

data class ComponentPatch(
    val component: String,
    val content: String,
    val boundaryId: String? = null,
    val ensureBoundary: Boolean = false,
)

internal data class EditorApplyProof(
    val content: String,
    val modificationStamp: Long,
)

internal fun editorApplyProofStillCurrentUtil(
    proof: EditorApplyProof,
    currentContent: String,
    currentModificationStamp: Long,
): Boolean =
    proof.modificationStamp == currentModificationStamp && proof.content == currentContent

/**
 * VFS-path analog of [editorApplyProofStillCurrentUtil] (#ipcfullprompt-recur2).
 *
 * The VFS write path ([PatchWatcher.applyPatchViaVfs]) computes the patched
 * result from a disk read taken at the start of the apply, then writes the whole
 * buffer back via `setBinaryContent`. There is no editor `Document` and therefore
 * no `modificationStamp`, so the guard re-reads disk immediately before the write
 * and confirms it still matches the bytes the result was computed from. If disk
 * changed underneath us (the file was opened + typed into, or another writer ran),
 * the whole-buffer write must fail closed rather than clobber the newer content —
 * leaving the patch file in place for the binary to retry. Returns true when the
 * current disk content still matches the bytes the patch was computed against.
 */
internal fun vfsDiskContentStillCurrentUtil(
    contentAtComputeTime: String,
    currentDiskContent: String,
): Boolean = contentAtComputeTime == currentDiskContent

internal fun sha256HexUtf8(content: String): String =
    MessageDigest.getInstance("SHA-256")
        .digest(content.toByteArray(Charsets.UTF_8))
        .joinToString("") { "%02x".format(it.toInt() and 0xff) }

internal fun fullContentExpectedBufferMatchesUtil(
    currentContent: String,
    expectedHash: String?,
    expectedLen: Int?,
): Boolean {
    if (expectedHash.isNullOrBlank()) return true
    if (expectedLen != null && currentContent.toByteArray(Charsets.UTF_8).size != expectedLen) {
        return false
    }
    return sha256HexUtf8(currentContent) == expectedHash
}

internal fun memoryDiskConflictCancelLikelyUtil(
    wasDeferredForConflict: Boolean,
    documentUnsaved: Boolean,
    documentModificationStamp: Long,
    fileModificationStamp: Long,
): Boolean =
    wasDeferredForConflict && documentUnsaved && documentModificationStamp != fileModificationStamp

internal fun findCodeBlockRangesUtil(doc: String): List<Pair<Int, Int>> {
    val ranges = mutableListOf<Pair<Int, Int>>()
    val fencePattern = Regex("""^[ \t]*```""", RegexOption.MULTILINE)
    var insideFence = false
    var fenceStart = 0

    for (match in fencePattern.findAll(doc)) {
        if (!insideFence) {
            fenceStart = match.range.first
            insideFence = true
        } else {
            val lineEnd = doc.indexOf('\n', match.range.last + 1)
            val blockEnd = if (lineEnd >= 0) lineEnd + 1 else doc.length
            ranges.add(Pair(fenceStart, blockEnd))
            insideFence = false
        }
    }

    return ranges
}

internal fun repositionBoundaryToEndUtil(
    doc: String,
    component: String,
    boundaryId: String? = null,
    preserveHead: Boolean = false,
): String? {
    val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
    val closeTag = "<!-- /agent:$component -->"
    val boundaryPattern = Regex("""<!-- agent:boundary:([a-z0-9][a-z0-9:-]*) -->""")

    val codeRanges = findCodeBlockRangesUtil(doc)

    val openMatch = openPattern.findAll(doc).firstOrNull { match ->
        codeRanges.none { range -> match.range.first >= range.first && match.range.first < range.second }
    } ?: return null

    val contentStart = openMatch.range.last + 1
    var searchFrom = contentStart
    var closeIdx: Int
    while (true) {
        closeIdx = doc.indexOf(closeTag, searchFrom)
        if (closeIdx < 0) return null
        if (codeRanges.none { range -> closeIdx >= range.first && closeIdx < range.second }) break
        searchFrom = closeIdx + closeTag.length
    }

    val componentContent = doc.substring(contentStart, closeIdx)
    val allBoundaries = boundaryPattern.findAll(componentContent).toList()
    if (allBoundaries.isEmpty()) return null

    if (allBoundaries.size == 1) {
        val onlyMatch = allBoundaries[0]
        val boundaryEnd = contentStart + onlyMatch.range.last + 1
        val afterBoundary = doc.substring(boundaryEnd, closeIdx).trim()
        if (afterBoundary.isEmpty()) return null
    }

    val lines = componentContent.split("\n")
    val filteredLines = lines.filter { line ->
        val trimmed = line.trim()
        !(trimmed.startsWith("<!-- agent:boundary:") && trimmed.endsWith(" -->"))
    }
    val filteredContent = filteredLines.joinToString("\n")
    val cleanContent = if (preserveHead) filteredContent else stripTransientHeadMarkers(filteredContent)

    val newBoundaryId = boundaryId ?: java.util.UUID.randomUUID().toString().substring(0, 8)
    val newBoundary = "<!-- agent:boundary:$newBoundaryId -->"
    val before = doc.substring(0, contentStart)
    val after = doc.substring(closeIdx)
    val prefix = if (cleanContent.endsWith("\n")) "" else "\n"
    return before + cleanContent + prefix + newBoundary + "\n" + after
}

internal fun annotateExchangeHeadingsAgainstBaselineUtil(doc: String, component: String, baselineDoc: String): String? {
    val currentRange = findComponentRangeUtil(doc, component) ?: return null
    val baselineRange = findComponentRangeUtil(baselineDoc, component)

    val currentContent = doc.substring(currentRange.first, currentRange.second)
    val baselineContent = baselineRange?.let { baselineDoc.substring(it.first, it.second) } ?: ""
    fun normalizeForCompare(value: String): String =
        stripTransientHeadMarkers(value)
            .lines()
            .filterNot { it.trim().startsWith("<!-- agent:boundary:") }
            .joinToString("\n")
    if (normalizeForCompare(currentContent) == normalizeForCompare(baselineContent)) {
        return doc
    }

    val annotated = annotateReHeadingsWithHeadUtil(currentContent, collectReHeadingsUtil(baselineContent))
    if (annotated == currentContent) {
        return doc
    }
    return doc.substring(0, currentRange.first) + annotated + doc.substring(currentRange.second)
}

internal fun appendPatchAlreadyPresentUtil(doc: String, component: String, content: String): Boolean {
    val range = findComponentRangeUtil(doc, component) ?: return false
    val patch = normalizeAppendPatchContentForCompare(content)
    if (patch.isEmpty()) return false
    val existing = normalizeAppendPatchContentForCompare(doc.substring(range.first, range.second))
    return existing.contains(patch)
}

internal fun patchReplayAlreadyPresentUtil(
    patch: IpcPatch,
    candidateContents: List<String>,
    committedContentAlreadyPresent: (String) -> Boolean = { false },
): Boolean {
    val payloads = replayResponsePayloads(patch)
    if (payloads.isEmpty()) return false
    return payloads.all { payload ->
        candidateContents.any { content -> appendPatchAlreadyPresentUtil(content, "exchange", payload) } ||
            committedContentAlreadyPresent(payload)
    }
}

private fun replayResponsePayloads(patch: IpcPatch): List<String> {
    val payloads = mutableListOf<String>()
    for (componentPatch in patch.patches) {
        if (componentPatch.component == "exchange" && looksLikeResponseReplayPayload(componentPatch.content)) {
            payloads.add(componentPatch.content)
        }
    }
    if (looksLikeResponseReplayPayload(patch.unmatched)) {
        payloads.add(patch.unmatched)
    }
    return payloads
}

private fun looksLikeResponseReplayPayload(content: String): Boolean {
    val trimmed = content.trim()
    return trimmed.contains("\n### Re:") ||
        trimmed.startsWith("### Re:") ||
        trimmed.contains("\n## Assistant") ||
        trimmed.startsWith("## Assistant")
}

private fun normalizeAppendPatchContentForCompare(content: String): String {
    return stripTransientHeadMarkers(content)
        .split("\n")
        .filterNot {
            val trimmed = it.trim()
            trimmed.startsWith("<!-- agent:boundary:") && trimmed.endsWith(" -->")
        }
        .joinToString("\n")
        .trim()
}

private fun findComponentRangeUtil(doc: String, component: String): Pair<Int, Int>? {
    val openPattern = Regex("""<!-- agent:${Regex.escape(component)}(\s[^>]*)? -->""")
    val closeTag = "<!-- /agent:$component -->"
    val codeRanges = findCodeBlockRangesUtil(doc)

    val openMatch = openPattern.findAll(doc).firstOrNull { match ->
        codeRanges.none { range -> match.range.first >= range.first && match.range.first < range.second }
    } ?: return null

    val contentStart = openMatch.range.last + 1
    var searchFrom = contentStart
    var closeIdx: Int
    while (true) {
        closeIdx = doc.indexOf(closeTag, searchFrom)
        if (closeIdx < 0) return null
        if (codeRanges.none { range -> closeIdx >= range.first && closeIdx < range.second }) break
        searchFrom = closeIdx + closeTag.length
    }

    return Pair(contentStart, closeIdx)
}

private fun stripTransientHeadMarkers(content: String): String {
    val lines = content.split("\n")
    val result = mutableListOf<String>()
    var inFence = false
    var fenceChar = '\u0000'
    var fenceLen = 0

    for (line in lines) {
        val trimmed = line.trimStart()
        val first = trimmed.firstOrNull()
        val runLen = if (first == null) 0 else trimmed.takeWhile { it == first }.length

        if (!inFence && (first == '`' || first == '~') && runLen >= 3) {
            inFence = true
            fenceChar = first
            fenceLen = runLen
            result.add(line)
            continue
        }
        if (inFence) {
            if (first == fenceChar && runLen >= fenceLen && trimmed.drop(runLen).trim().isEmpty()) {
                inFence = false
            }
            result.add(line)
            continue
        }

        if (Regex("""^\s*#{1,6}\s""").containsMatchIn(line) && line.endsWith(" (HEAD)")) {
            result.add(line.removeSuffix(" (HEAD)"))
        } else {
            result.add(line)
        }
    }

    return result.joinToString("\n")
}

private fun stripReHeadingAttributionForCompare(content: String): String {
    val codeRanges = findCodeBlockRangesUtil(content)
    val normalized = mutableListOf<String>()
    var offset = 0

    for (line in content.split("\n")) {
        val inCode = codeRanges.any { range -> offset >= range.first && offset < range.second }
        offset += line.length + 1
        if (inCode) {
            normalized.add(line)
            continue
        }

        val trimmed = line.trimStart()
        val hashCount = trimmed.takeWhile { it == '#' }.length
        if (hashCount in 1..6 && trimmed.getOrNull(hashCount) == ' ') {
            val afterHash = trimmed.drop(hashCount).trimStart()
            if (afterHash.startsWith("Re:")) {
                val dash = line.lastIndexOf(" — ")
                if (dash >= 0) {
                    normalized.add(line.substring(0, dash))
                    continue
                }
            }
        }

        normalized.add(line)
    }

    return normalized.joinToString("\n")
}

private fun normalizeAnsweredPromptPrefixesForCompare(content: String): String {
    val currentRange = findComponentRangeUtil(content, "exchange") ?: return content
    val exchange = content.substring(currentRange.first, currentRange.second)
    val codeRanges = findCodeBlockRangesUtil(exchange)
    val lines = exchange.split("\n").toMutableList()
    val lineOffsets = IntArray(lines.size)
    var offset = 0

    for (idx in lines.indices) {
        lineOffsets[idx] = offset
        offset += lines[idx].length + 1
    }

    fun inCodeBlock(idx: Int): Boolean =
        codeRanges.any { range -> lineOffsets[idx] >= range.first && lineOffsets[idx] < range.second }

    fun nextMeaningfulLine(afterIdx: Int): String? {
        for (idx in (afterIdx + 1) until lines.size) {
            if (inCodeBlock(idx)) continue
            val trimmed = lines[idx].trim()
            if (trimmed.isEmpty()) continue
            if (trimmed.startsWith("<!-- agent:boundary:")) return null
            if (trimmed.startsWith("<!--")) continue
            return lines[idx]
        }
        return null
    }

    for (idx in lines.indices) {
        if (inCodeBlock(idx)) continue
        val trimmed = lines[idx].trimStart()
        if (!trimmed.startsWith("❯ ")) continue
        val next = nextMeaningfulLine(idx) ?: continue
        if (!next.trimStart().startsWith("### Re:")) continue
        lines[idx] = lines[idx].replaceFirst(Regex("""^(\s*)❯\s"""), "$1")
    }

    return content.substring(0, currentRange.first) + lines.joinToString("\n") + content.substring(currentRange.second)
}

internal fun shouldPreferCommittedDiskContentForRepositionUtil(editorContent: String, diskContent: String): Boolean {
    if (!outsideComponentContentMatchesExactly(editorContent, diskContent, "exchange")) {
        return false
    }

    fun normalize(content: String): String =
        stripReHeadingAttributionForCompare(
            normalizeAnsweredPromptPrefixesForCompare(stripTransientHeadMarkers(content))
                .lines()
                .filterNot { it.trim().startsWith("<!-- agent:boundary:") }
                .joinToString("\n")
        )

    return normalize(editorContent) == normalize(diskContent)
}

private fun outsideComponentContentMatchesExactly(left: String, right: String, component: String): Boolean {
    val leftRange = findComponentRangeUtil(left, component) ?: return left == right
    val rightRange = findComponentRangeUtil(right, component) ?: return false

    return left.substring(0, leftRange.first) == right.substring(0, rightRange.first) &&
        left.substring(leftRange.second) == right.substring(rightRange.second)
}

/** Stable, collision-tolerant content fingerprint for forensic log correlation. */
internal fun documentMutationContentHashUtil(content: String): String =
    "%08x:%d".format(content.hashCode(), content.length)

/**
 * The post-boundary tail of the `exchange` component: everything after the last
 * `<!-- agent:boundary:* -->` marker up to the exchange close. This is the live
 * user-prompt region; comparing it pre/post a `setText` reveals prompt
 * duplication (grows) or deletion (shrinks) caused by a whole-buffer mutation.
 */
internal fun postBoundaryExchangeRegionUtil(content: String): String {
    val range = findComponentRangeUtil(content, "exchange") ?: return ""
    val body = content.substring(range.first, range.second)
    val lastBoundary = body.lastIndexOf("<!-- agent:boundary:")
    val tailStart = if (lastBoundary < 0) {
        0
    } else {
        val nl = body.indexOf('\n', lastBoundary)
        if (nl < 0) body.length else nl + 1
    }
    return body.substring(tailStart).trim()
}

/**
 * Structured diagnostic for an editor-visible whole-buffer mutation (`setText`).
 * Logged at every such site so a full-document IPC corruption packet can be
 * reconstructed from `idea.log` without a live debugger — operation, patch id,
 * transport, pre/post fingerprints, and whether the post-boundary user region
 * changed (the corruption/duplication signal). See
 * tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md (#ipcfullprompt-recur).
 */
internal fun documentMutationDiagnosticUtil(
    operation: String,
    file: String,
    patchId: String?,
    transport: String,
    preContent: String,
    postContent: String,
    modStamp: Long,
    idleReached: Boolean,
): String {
    val preRegion = postBoundaryExchangeRegionUtil(preContent)
    val postRegion = postBoundaryExchangeRegionUtil(postContent)
    return "[patch-watcher] document-mutation op=$operation file=$file patch_id=${patchId ?: "-"} " +
        "transport=$transport pre=${documentMutationContentHashUtil(preContent)} " +
        "post=${documentMutationContentHashUtil(postContent)} mod_stamp=$modStamp idle=$idleReached " +
        "pre_user_region_len=${preRegion.length} post_user_region_len=${postRegion.length} " +
        "user_region_changed=${preRegion != postRegion}"
}

private fun collectReHeadingsUtil(content: String): Set<String> {
    val codeRanges = findCodeBlockRangesUtil(content)
    val headings = linkedSetOf<String>()
    var offset = 0

    for (line in content.split("\n")) {
        val inCode = codeRanges.any { range -> offset >= range.first && offset < range.second }
        if (!inCode) {
            val trimmed = line.trimStart()
            val hashCount = trimmed.takeWhile { it == '#' }.length
            val afterHash = trimmed.drop(hashCount)
            if (hashCount in 1..6 && afterHash.startsWith(' ') && afterHash.trimStart().startsWith("Re:")) {
                headings.add(line.trimStart().trimEnd().removeSuffix(" (HEAD)"))
            }
        }
        offset += line.length + 1
    }

    return headings
}

private fun annotateReHeadingsWithHeadUtil(content: String, baseline: Set<String>): String {
    val codeRanges = findCodeBlockRangesUtil(content)
    val lines = content.split("\n").toMutableList()
    val reIndices = mutableListOf<Int>()
    var offset = 0

    for ((idx, line) in lines.withIndex()) {
        val inCode = codeRanges.any { range -> offset >= range.first && offset < range.second }
        offset += line.length + 1
        if (inCode) continue

        val trimmed = line.trimStart()
        val hashCount = trimmed.takeWhile { it == '#' }.length
        val afterHash = trimmed.drop(hashCount)
        if (hashCount !in 1..6 || !afterHash.startsWith(' ') || !afterHash.trimStart().startsWith("Re:")) {
            continue
        }

        lines[idx] = line.trimEnd().removeSuffix(" (HEAD)")
        reIndices.add(idx)
    }

    val markIndices = reIndices.filter { idx -> !baseline.contains(lines[idx].trimStart().trimEnd()) }
    val finalIndices = if (markIndices.isNotEmpty()) markIndices else reIndices.takeLast(1)
    for (idx in finalIndices) {
        lines[idx] = lines[idx] + " (HEAD)"
    }

    return lines.joinToString("\n")
}

/**
 * Parse IPC patch JSON using Gson (bundled with IntelliJ Platform).
 * Gson handles all JSON escape sequences correctly, eliminating the
 * class of bugs from hand-rolled parsing (e.g., \\n unescape order).
 */
fun parsePatchJson(json: String): IpcPatch? {
    try {
        val root = com.google.gson.JsonParser.parseString(json).asJsonObject

        val file = root.get("file")?.asString ?: return null
        val unmatched = root.get("unmatched")?.asString ?: ""
        val frontmatter = root.get("frontmatter")?.asString
        val fullContent = root.get("fullContent")?.asString

        val patches = mutableListOf<ComponentPatch>()
        val patchesArray = root.getAsJsonArray("patches") ?: return null
        for (elem in patchesArray) {
            val obj = elem.asJsonObject
            val component = obj.get("component")?.asString ?: continue
            val content = obj.get("content")?.asString ?: continue
            val boundaryId = obj.get("boundary_id")?.asString
            val ensureBoundary = obj.get("ensure_boundary")?.asBoolean ?: false
            patches.add(ComponentPatch(component, content, boundaryId, ensureBoundary))
        }

        val repositionBoundary = root.get("reposition_boundary")?.asBoolean ?: false
        val repositionBoundaryId = root.get("reposition_boundary_id")?.asString
        val preserveHead = root.get("preserve_head")?.asBoolean ?: false
        val normalizePrefixLines = root.getAsJsonArray("normalize_prefix_lines")
            ?.mapNotNull { it.asString } ?: emptyList()
        val patchId = root.get("patch_id")?.asString
        val expectedContentHash = root.get("expected_content_hash")?.asString
        val expectedContentLen = root.get("expected_content_len")?.asInt
        val queueAuto = root.get("queue_auto")?.let { if (it.isJsonNull) null else it.asBoolean }
        val cycleId = root.get("cycle_id")?.let { if (it.isJsonNull) null else it.asString }
        val baselineHash = root.get("baseline_hash")?.let { if (it.isJsonNull) null else it.asString }
        return IpcPatch(
            file,
            patches,
            unmatched,
            frontmatter,
            fullContent,
            repositionBoundary,
            repositionBoundaryId,
            preserveHead,
            normalizePrefixLines,
            patchId,
            expectedContentHash,
            expectedContentLen,
            queueAuto,
            cycleId,
            baselineHash,
        )
    } catch (e: Exception) {
        return null
    }
}

/**
 * Extract a string field from a JSON object string using Gson.
 * Used by handleSocketMessage for lightweight field extraction.
 */
private fun extractStringField(json: String, field: String): String? {
    return try {
        com.google.gson.JsonParser.parseString(json).asJsonObject.get(field)?.asString
    } catch (e: Exception) {
        null
    }
}

/**
 * Add `❯ ` prefix to specific lines within the exchange component's user region.
 *
 * Uses line-by-line scanning with [String.trimEnd] comparison so trailing whitespace
 * differences between the binary's disk-side payload and IntelliJ's editor buffer
 * (which strips trailing spaces on save) do not silently lose the prefix.
 *
 * Only the region before the LAST boundary marker is normalised; the agent region is left
 * unchanged.  Lines already starting with `❯ ` are left unchanged (idempotent).
 */
internal fun normalizeExchangePrefixesUtil(doc: String, lines: List<String>): String {
    if (lines.isEmpty()) return doc
    val openTag = Regex("""<!-- agent:exchange(\s[^>]*)? -->""")
    val closeTag = "<!-- /agent:exchange -->"
    val boundaryTag = Regex("""<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->""")

    val openMatch = openTag.find(doc) ?: return doc
    val closeIdx = doc.indexOf(closeTag, openMatch.range.last)
    if (closeIdx < 0) return doc

    val beforeExchange = doc.substring(0, openMatch.range.last + 1)
    val exchangeContent = doc.substring(openMatch.range.last + 1, closeIdx)
    val afterExchange = doc.substring(closeIdx)

    // Only normalize the user-input region (before the LAST boundary marker).
    // Must use the last boundary — historical cycles each leave a marker, so stopping
    // at the first one misclassifies later user-input lines as agent region.
    val boundaryMatch = boundaryTag.findAll(exchangeContent).lastOrNull()
    val userRegionEnd = boundaryMatch?.range?.first ?: exchangeContent.length
    val userRegion = exchangeContent.substring(0, userRegionEnd)
    val agentRegion = exchangeContent.substring(userRegionEnd)

    // Build normalized target set once — trimEnd() absorbs trailing-whitespace divergence.
    val targetLines = lines.filter { it.isNotBlank() }.map { it.trimEnd() }.toSet()

    var inResponseBlock = false
    val normalizedUserRegion = userRegion.split("\n").joinToString("\n") { docLine ->
        val trimmed = docLine.trim()
        when {
            boundaryTag.matches(trimmed) -> {
                inResponseBlock = false
                return@joinToString docLine
            }
            isExchangeResponseHeadingForPrefixRepair(trimmed) -> {
                inResponseBlock = true
                return@joinToString docLine
            }
        }
        val normalized = docLine.trimEnd()
        val isTarget = normalized in targetLines
        if (inResponseBlock) {
            if (startsPromptRunAfterResponseForPrefixRepair(trimmed, isTarget)) {
                inResponseBlock = false
            } else {
                return@joinToString docLine
            }
        }
        when {
            normalized.startsWith("❯ ") -> docLine   // already prefixed — idempotent
            isTarget -> "❯ $docLine" // match — add prefix
            else -> docLine
        }
    }

    return beforeExchange + normalizedUserRegion + agentRegion + afterExchange
}

private fun isExchangeResponseHeadingForPrefixRepair(trimmed: String): Boolean =
    trimmed == "## Assistant" ||
        trimmed.startsWith("### Re:") ||
        trimmed.startsWith("#### Re:") ||
        trimmed.startsWith("##### Re:") ||
        trimmed.startsWith("###### Re:")

private fun startsPromptRunAfterResponseForPrefixRepair(trimmed: String, isTarget: Boolean): Boolean {
    val alreadyPrefixed = trimmed.startsWith("❯ ")
    val unprefixed = if (alreadyPrefixed) trimmed.removePrefix("❯ ").trimStart() else trimmed
    if (lineLooksLikePlainResponseAfterPromptForPrefixRepair(unprefixed)) return false
    return lineLooksLikeFreshPromptAfterResponseForPrefixRepair(unprefixed) ||
        ((alreadyPrefixed || isTarget) && !lineLooksLikePlainResponseAfterPromptForPrefixRepair(unprefixed))
}

private fun lineLooksLikeFreshPromptAfterResponseForPrefixRepair(trimmed: String): Boolean {
    val lower = trimmed.removePrefix("❯").trim().lowercase()
    return trimmed.startsWith("❯") ||
        trimmed.endsWith("?") ||
        lower == "go" ||
        lower == "continue" ||
        lower.startsWith("do #") ||
        lower.startsWith("do [#") ||
        lower.startsWith("fix #") ||
        lower.startsWith("run ") ||
        lower.startsWith("rerun ") ||
        lower.startsWith("build ") ||
        lower.startsWith("test ") ||
        lower.startsWith("commit ") ||
        lower.startsWith("push ") ||
        lower.startsWith("verify ") ||
        lower.startsWith("investigate ")
}

private fun lineLooksLikePlainResponseAfterPromptForPrefixRepair(trimmed: String): Boolean {
    if (trimmed.isBlank()) return false
    if (lineHasKnownResponseLabelForPrefixRepair(trimmed)) return true
    if (lineLooksLikeFreshPromptAfterResponseForPrefixRepair(trimmed)) return false
    if (
        trimmed.startsWith("- ") ||
        trimmed.startsWith("* ") ||
        trimmed.startsWith("Plan:") ||
        trimmed.startsWith("Verification") ||
        trimmed.startsWith("What changed:") ||
        trimmed.startsWith("Follow-up:") ||
        trimmed.startsWith("Commit / push:") ||
        trimmed.startsWith("Backlog:") ||
        trimmed.startsWith("`#")
    ) {
        return true
    }
    val lower = trimmed.lowercase()
    return lower.startsWith("i updated ") ||
        lower.startsWith("i fixed ") ||
        lower.startsWith("i added ") ||
        lower.startsWith("i implemented ") ||
        lower.startsWith("i left ") ||
        lower.startsWith("updated ") ||
        lower.startsWith("fixed ") ||
        lower.startsWith("added ") ||
        lower.startsWith("implemented ")
}

private fun lineHasKnownResponseLabelForPrefixRepair(line: String): Boolean {
    val normalized = normalizeResponseLabelCandidateForPrefixRepair(line) ?: return false
    return normalized.startsWith("Plan:") ||
        normalized.startsWith("Verification:") ||
        normalized.startsWith("What changed:") ||
        normalized.startsWith("Follow-up:") ||
        normalized.startsWith("Commit / push:") ||
        normalized.startsWith("Backlog:")
}

private fun normalizeResponseLabelCandidateForPrefixRepair(line: String): String? {
    var trimmed = line.trim()
    if (trimmed.isBlank()) return null

    if (trimmed.startsWith("❯")) {
        trimmed = trimmed.removePrefix("❯").trimStart()
    }

    val listMarker = Regex("""^([-*+]|\d+[.)])\s+""").find(trimmed)
    if (listMarker != null) {
        trimmed = trimmed.substring(listMarker.range.last + 1).trimStart()
    }

    trimmed = stripMarkdownEmphasisPairForPrefixRepair(trimmed).trimStart()
    return trimmed.ifBlank { null }
}

private fun stripMarkdownEmphasisPairForPrefixRepair(text: String): String {
    for (marker in listOf("***", "___", "**", "__", "*", "_")) {
        if (!text.startsWith(marker)) continue
        val rest = text.removePrefix(marker)
        val end = rest.indexOf(marker)
        if (end < 0) continue
        val label = rest.substring(0, end)
        val tail = rest.substring(end + marker.length)
        if (label.isNotBlank() && (label.trimEnd().endsWith(":") || tail.trimStart().startsWith(":"))) {
            return label + tail
        }
    }
    return text
}

/**
 * Extract a boolean field from a JSON object string using Gson.
 * Returns false if the field is absent or not a boolean.
 */
internal fun extractBooleanField(json: String, field: String): Boolean {
    return try {
        com.google.gson.JsonParser.parseString(json).asJsonObject.get(field)?.asBoolean ?: false
    } catch (e: Exception) {
        false
    }
}
