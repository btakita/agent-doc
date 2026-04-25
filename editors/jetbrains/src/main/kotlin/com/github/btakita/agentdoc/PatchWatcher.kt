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
import com.intellij.util.Alarm
import java.io.File
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
    )

    /** Registered roots, keyed by absolute path. Written through [registerRoot]. */
    private val rootStates = java.util.concurrent.ConcurrentHashMap<String, RootState>()

    /** Dedup cache keyed by patch_id (globally unique). Shared across all roots. */
    private val appliedPatchIds = java.util.concurrent.ConcurrentHashMap<String, Long>()

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

        val callback = object : AgentDocLib.IpcMessageCallback {
            override fun invoke(message: Pointer): Boolean {
                return try {
                    val json = message.getString(0)
                    handleSocketMessage(json)
                } catch (e: Exception) {
                    LOG.warn("[socket] callback error", e)
                    false
                }
            }
        }
        state.ipcCallback = callback

        val started = lib.agent_doc_start_ipc_listener(state.root, callback)
        if (started) {
            LOG.info("[socket] IPC listener started via FFI for ${state.root}")
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
    private fun handleSocketMessage(json: String): Boolean {
        val type = extractStringField(json, "type") ?: return false

        return when (type) {
            "patch" -> {
                val patch = parsePatchJson(json) ?: return false
                if (isClaimedByForceDisk(patch.patchId, patch.file)) {
                    LOG.info("[socket] dedup: sentinel exists for patch_id ${patch.patchId} — skipping apply")
                    return true
                }
                if (isAlreadyApplied(patch.patchId)) {
                    LOG.info("[socket] dedup: patch_id ${patch.patchId} already applied — skipping")
                    return true
                }
                var applied = false
                ApplicationManager.getApplication().invokeAndWait {
                    // Re-check under EDT to avoid TOCTOU race with file watcher
                    if (isAlreadyApplied(patch.patchId)) {
                        LOG.info("[socket] dedup (EDT): patch_id ${patch.patchId} already applied — skipping")
                        return@invokeAndWait
                    }
                    applied = try {
                        applyPatch(patch)
                    } catch (e: Exception) {
                        LOG.warn("[socket] Failed to apply patch", e)
                        false
                    }
                    if (applied) {
                        recordApplied(patch.patchId)
                    }
                }
                if (applied) {
                    // Clean up any stale patch file for this document to prevent the file
                    // watcher from applying the same content again (double-apply prevention).
                    // Resolve the root by walking up from the doc path — this handles submodule
                    // roots whose patches live in `<submodule>/.agent-doc/patches/`, not the
                    // parent project's `.agent-doc/patches/`.
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
                applied
            }
            "reposition" -> {
                val file = extractStringField(json, "file") ?: return false
                val boundaryId = extractStringField(json, "boundary_id")
                val preserveHead = extractBooleanField(json, "preserve_head")
                repositionBoundaryViaDocument(file, boundaryId, preserveHead)
                true
            }
            "vcs_refresh" -> {
                refreshVcs()
                true
            }
            else -> {
                LOG.warn("[socket] Unknown message type: $type")
                false
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
            if (lib != null) {
                lib.agent_doc_await_idle(filePath, 500, 5000)
            }
            ApplicationManager.getApplication().invokeLater {
                val reposStart = System.nanoTime()
                val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeLater
                targetFile.refresh(false, false)
                val fdm = FileDocumentManager.getInstance()
                val document = fdm.getDocument(targetFile) ?: return@invokeLater
                if (!fdm.isDocumentUnsaved(document)) {
                    fdm.reloadFromDisk(document)
                }

                WriteCommandAction.runWriteCommandAction(project, "Agent Doc Reposition", null, {
                    val content = document.text
                    val result = if (preserveHead) {
                        NativePatching.repositionBoundaryToEndPreserveHead(content, boundaryId)
                            ?: repositionBoundaryToEnd(content, "exchange", boundaryId)
                    } else {
                        NativePatching.repositionBoundaryToEnd(content, boundaryId)
                            ?: repositionBoundaryToEnd(content, "exchange", boundaryId)
                    } ?: return@runWriteCommandAction
                    if (result != content && document.text == content) {
                        document.setText(result)
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

            // patch_id dedup: if socket IPC already applied this logical write, skip.
            if (isAlreadyApplied(patch.patchId)) {
                LOG.info("[patch-watcher] dedup: patch_id ${patch.patchId} already applied via socket — deleting: ${patchFile.name}")
                patchFile.delete()
                return
            }

            ApplicationManager.getApplication().invokeLater {
                if (isClaimedByForceDisk(patch.patchId, patch.file) || isPatchAlreadyApplied(patch, patchFile)) {
                    LOG.info("[patch-watcher] dedup (inner): skipping apply for ${patchFile.name}")
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

        // Full content replacement — only for append-mode documents without component patches.
        // When component patches are present, use the patch path instead: it applies
        // normalize_prefix_lines + patches correctly without clobbering the response.
        // Sending fullContent alongside patches caused the plugin to bypass patches and
        // replace the document with a version that may not include the new response,
        // leading to duplicates on the next cycle.
        if (!patch.fullContent.isNullOrEmpty() && patch.patches.isEmpty()) {
            if (patch.fullContent == content) {
                LOG.warn("Patch produced no changes for ${patch.file}")
                writeAckContent(patch.patchId, document.text, patch.file)
                return true
            }
            WriteCommandAction.runWriteCommandAction(project, "Agent Doc Patch", null, {
                document.setText(patch.fullContent)
                LOG.info("Patch applied (full content) to ${patch.file}")
            })
            FileDocumentManager.getInstance().saveDocument(document)
            writeAckContent(patch.patchId, document.text, patch.file)
            return true
        }

        // Component-based patching (template/stream-mode documents)
        var result = content

        // Apply frontmatter patch first (before component patches)
        if (!patch.frontmatter.isNullOrBlank()) {
            result = NativePatching.mergeFrontmatter(result, patch.frontmatter)
                ?: applyFrontmatterPatchKotlin(result, patch.frontmatter)
        }

        // Apply ❯  prefix normalization to user-typed lines before agent patches
        if (patch.normalizePrefixLines.isNotEmpty()) {
            result = normalizeExchangePrefixes(result, patch.normalizePrefixLines)
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
            result = NativePatching.repositionBoundaryToEnd(result, patch.repositionBoundaryId)
                ?: repositionBoundaryToEnd(result, "exchange", patch.repositionBoundaryId)
                ?: result
        }
        result = annotateExchangeHeadingsAgainstBaselineUtil(result, "exchange", content) ?: result

        if (result == content) {
            LOG.warn("Patch produced no changes for ${patch.file}")
            writeAckContent(patch.patchId, document.text, patch.file)
            return true
        }

        WriteCommandAction.runWriteCommandAction(project, "Agent Doc Patch", null, {
            document.setText(result)
            LOG.info("Patch applied to ${patch.file} (${result.length - content.length} chars changed)")
        })

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

    /**
     * Apply patches via VFS for files not open in the editor.
     * Reads content from disk, applies patches in memory, writes back through VFS.
     * This avoids the "externally modified" dialog since it goes through IntelliJ's VFS layer.
     */
    private fun applyPatchViaVfs(targetFile: com.intellij.openapi.vfs.VirtualFile, patch: IpcPatch): Boolean {
        try {
            val content = String(targetFile.contentsToByteArray(), targetFile.charset)

            // Full content replacement — only for append-mode documents without component patches.
            // Same guard as the Document path: skip fullContent when patches are present.
            if (!patch.fullContent.isNullOrEmpty() && patch.patches.isEmpty()) {
                if (patch.fullContent != content) {
                    ApplicationManager.getApplication().runWriteAction {
                        targetFile.setBinaryContent(patch.fullContent.toByteArray(targetFile.charset))
                    }
                    LOG.info("VFS patch applied (full content) to ${patch.file}")
                    writeAckContent(patch.patchId, patch.fullContent, patch.file)
                }
                return true
            }

            // Component-based patching
            var result = content

            if (!patch.frontmatter.isNullOrBlank()) {
                result = NativePatching.mergeFrontmatter(result, patch.frontmatter)
                    ?: applyFrontmatterPatchKotlin(result, patch.frontmatter)
            }

            // Apply ❯  prefix normalization to user-typed lines before agent patches
            if (patch.normalizePrefixLines.isNotEmpty()) {
                result = normalizeExchangePrefixes(result, patch.normalizePrefixLines)
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
                result = NativePatching.repositionBoundaryToEnd(result, patch.repositionBoundaryId)
                    ?: repositionBoundaryToEnd(result, "exchange", patch.repositionBoundaryId)
                    ?: result
            }
            result = annotateExchangeHeadingsAgainstBaselineUtil(result, "exchange", content) ?: result

            if (result != content) {
                ApplicationManager.getApplication().runWriteAction {
                    targetFile.setBinaryContent(result.toByteArray(targetFile.charset))
                }
                LOG.info("VFS patch applied to ${patch.file} (${result.length - content.length} chars changed)")
            } else {
                LOG.warn("VFS patch produced no changes for ${patch.file}")
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
     *
     * For each line in [lines], replaces the first occurrence of that exact
     * line text (without `❯ ` prefix) with `❯ <line>` within the region of
     * the exchange component that falls before the boundary marker. Lines
     * that already have the prefix or do not appear in the component are
     * left unchanged.
     */
    private fun normalizeExchangePrefixes(doc: String, lines: List<String>): String {
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
        var userRegion = exchangeContent.substring(0, userRegionEnd)
        val agentRegion = exchangeContent.substring(userRegionEnd)

        for (line in lines) {
            if (line.isBlank()) continue
            // Replace exact line (with newline boundaries) — idempotent if already prefixed
            userRegion = userRegion.replace("\n$line\n", "\n❯ $line\n")
            // Handle line at very start of user region
            if (userRegion.startsWith("$line\n")) {
                userRegion = "❯ $line\n" + userRegion.substring(line.length + 1)
            }
        }

        return beforeExchange + userRegion + agentRegion + afterExchange
    }

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

    private fun repositionBoundaryToEnd(doc: String, component: String, boundaryId: String? = null) =
        repositionBoundaryToEndUtil(doc, component, boundaryId)

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
    /** Lines whose plain text should be prefixed with `❯ ` in the exchange component. */
    val normalizePrefixLines: List<String> = emptyList(),
    /** UUID identifying this patch — used for ack-content sidecar and claimed-patches sentinel. */
    val patchId: String? = null,
)

data class ComponentPatch(
    val component: String,
    val content: String,
    val boundaryId: String? = null,
    val ensureBoundary: Boolean = false,
)

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

internal fun repositionBoundaryToEndUtil(doc: String, component: String, boundaryId: String? = null): String? {
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
    val cleanContent = stripTransientHeadMarkers(filteredLines.joinToString("\n"))

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
        val normalizePrefixLines = root.getAsJsonArray("normalize_prefix_lines")
            ?.mapNotNull { it.asString } ?: emptyList()
        val patchId = root.get("patch_id")?.asString
        return IpcPatch(
            file,
            patches,
            unmatched,
            frontmatter,
            fullContent,
            repositionBoundary,
            repositionBoundaryId,
            normalizePrefixLines,
            patchId,
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
