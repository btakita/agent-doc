package com.github.btakita.agentdoc

import com.sun.jna.Pointer
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vcs.changes.VcsDirtyScopeManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.util.Alarm
import java.io.File
import java.time.Instant
import java.lang.reflect.Field
import java.lang.reflect.Method
import java.security.MessageDigest

private enum class CrdtReplicaEventReason(val token: String) {
    RequestFullState("request_full_state"),
    Fanout("fanout"),
    ResponseCellAdd("response_cell_add"),
    CpcWrite("cpc_write"),
    Rebootstrap("rebootstrap"),
    AckReplay("ack_replay"),
    AckRecoveryForceRefresh("ack_recovery_force_refresh");

    companion object {
        fun fromToken(token: String?): CrdtReplicaEventReason? =
            entries.firstOrNull { it.token == token }
    }
}

/** Cross-language editor intent names; mirrored by Rust and VS Code. */
private enum class EditorIntent(val token: String) {
    ApplyCanonical("apply_canonical"),
    Reposition("reposition"),
    SaveDocument("save_document"),
    RefreshContent("refresh_content"),
    ObserveLazilyCurrent("observe_lazily_current"),
    DeliverCrdtRemote("deliver_crdt_remote"),
    RefreshVcs("refresh_vcs"),
    ReloadLibrary("reload_library"),
}

/**
 * Hosts PID-scoped endpoints for IntelliJ's registered Lazily replicas.
 * Document mutations arrive as typed messages and publish typed receipts;
 * normal realtime paths use minimal range edits so undo remains local.
 *
 * **Multi-root:** a single watcher tracks every nested `.agent-doc/` project
 * under `project.basePath` (scanned at startup) plus any additional roots
 * discovered at runtime via [registerRoot] (called by actions when they
 * resolve a submodule root via FFI). Each root has its own FFI socket listener
 * and shares the applied-delivery dedup cache.
 */
class PatchWatcher(private val project: Project) : Disposable {
    private val operatorTextAuthorityCapability = "operator_text_authority_v1"
    private val lazilyTransportReceiptsCapability = "lazily_transport_receipts_v1"
    private val editorCapabilities = listOf(
        operatorTextAuthorityCapability,
        lazilyTransportReceiptsCapability,
    ).joinToString(",")

    private data class RootState(
        val root: String,
        @Volatile var ipcCallback: AgentDocLib.IpcMessageCallback? = null,
        @Volatile var ipcCallbackV2: AgentDocLib.IpcMessageCallbackV2? = null,
    )

    /** Registered roots, keyed by absolute path. Written through [registerRoot]. */
    private val rootStates = java.util.concurrent.ConcurrentHashMap<String, RootState>()

    /** Dedup cache keyed by patch_id (globally unique). Shared across all roots. */
    private val appliedPatchIds = java.util.concurrent.ConcurrentHashMap<String, Long>()

    /** Boundary reposition requests delayed after a stale proof/apply attempt. */
    private val scheduledRepositionRetries = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()

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

    @Volatile private var lastApplyBlockedForFileCacheConflict = false

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

    private fun currentContentForProjection(filePath: String): String? {
        // VFS lookup + synchronous refresh must stay OUTSIDE a read action: a
        // synchronous `refresh(false)` cannot run while holding the read lock.
        var targetFile = LocalFileSystem.getInstance().findFileByPath(filePath)
        if (targetFile == null) {
            LocalFileSystem.getInstance().refresh(false)
            targetFile = LocalFileSystem.getInstance().findFileByPath(filePath)
        }
        val file = targetFile
        if (file == null) {
            LOG.warn("[content-projection] already_applied target file not found: $filePath")
            return null
        }
        // #patchwatcher-readaccess: `getDocument()` / `document.text` touch the
        // IntelliJ document model, which requires a read action. This method runs
        // on the socket callback thread, so read the model inside a read action
        // instead of tripping `softAssertReadAccess`.
        return ApplicationManager.getApplication().runReadAction<String?> {
            val document = FileDocumentManager.getInstance().getDocument(file)
            if (document != null) {
                document.text
            } else {
                try {
                    String(file.contentsToByteArray(), file.charset)
                } catch (e: Exception) {
                    LOG.warn(
                        "[content-projection] failed to read already_applied VFS content for $filePath",
                        e,
                    )
                    null
                }
            }
        }
    }

    private fun writeAlreadyAppliedContentProjection(patch: IpcPatch, source: String): Boolean {
        val content = currentContentForProjection(patch.file) ?: return false
        val ok = writeEditorContentProjection(patch.patchId, content, patch.file)
        if (ok) {
            LOG.info("[content-projection] already_applied source=$source patch_id ${patch.patchId} content_len=${content.length}")
        }
        return ok
    }

    private fun patchWatcherPluginVersion(): String =
        javaClass.`package`?.implementationVersion ?: "dev"

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
     * Register a root directory and its FFI socket listener.
     * Idempotent: calling with an already-registered root is a no-op.
     *
     * Called at startup for [project.basePath] + nested discoveries, and at runtime
     * by action code when a submodule root is resolved via FFI.
     */
    fun registerRoot(root: String) {
        if (!running) return
        if (rootStates.containsKey(root)) return
        val state = RootState(root)
        if (rootStates.putIfAbsent(root, state) != null) return // race: another caller won
        startSocketListenerViaFfi(state)
        LOG.info("[lazily-endpoint] registered root: $root")
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

    private fun recordDocumentActivity(filePath: String, reason: String) {
        TurnStateBannerRefresher.getInstance(project).requestRefresh(filePath, reason)
        CrdtReplicaManager.requestRemoteDrain(project, filePath, reason)
    }

    /**
     * Handle a socket IPC message from the FFI listener.
     * Called on the FFI listener thread — dispatches to EDT for Document API operations.
     * Returns true if handled, false on error.
     */
    /**
     * Handle a socket IPC message and return the v2 receipt outcome.
     *
     * Return values match the FFI v2 contract:
     * - 0 → receipt `{"status":"rejected"}` (apply failed)
     * - 1 → receipt `{"status":"applied"}` (apply succeeded with content change)
     * - 2 → receipt `{"status":"applied","reason":"already_applied"}` (patch text
     *   already present in live buffer; binary skips file-IPC fallback so a
     *   duplicate response heading cannot land)
     *
     * Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
     * `#ipcpluginalready`.
     */
    private fun handleSocketMessageV2(json: String): Int {
        val type = extractStringField(json, "type") ?: return 0

        return when (type) {
            EditorIntent.ApplyCanonical.token -> {
                val patch = parsePatchJson(json) ?: return 0
                if (!patch.targetsThisEditor()) {
                    LOG.info("[socket] patch_id ${patch.patchId} targets editor_id ${patch.editorId}; this editor is ${EditorIdentity.id}")
                    return APPLY_FAILED
                }
                if (isAlreadyApplied(patch.patchId)) {
                    LOG.info("[socket] dedup: patch_id ${patch.patchId} already applied — emitting already_applied")
                    return if (writeAlreadyAppliedContentProjection(patch, "socket_precheck")) APPLY_ALREADY_APPLIED else APPLY_FAILED
                }
                val stateGeneration = StateProjectionBridge.recordEditorPatchQueued(patch.file, patch.patchId)
                if (!patch.fullContent.isNullOrEmpty()) {
                    LOG.warn("[socket] full-content IPC is disabled; rejecting patch_id ${patch.patchId} for ${patch.file}")
                    StateProjectionBridge.recordEditorRetryRequested(
                        patch.file,
                        patch.patchId,
                        stateGeneration,
                        "full_content_ipc_disabled",
                    )
                    return APPLY_FAILED
                }
                var applied = false
                var wasNoOp = false
                ApplicationManager.getApplication().invokeAndWait {
                    // Re-check under EDT to avoid TOCTOU race with file watcher
                    if (isAlreadyApplied(patch.patchId)) {
                        LOG.info("[socket] dedup (EDT): patch_id ${patch.patchId} already applied — emitting already_applied")
                        wasNoOp = writeAlreadyAppliedContentProjection(patch, "socket_edt_recheck")
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
                    StateProjectionBridge.recordEditorPatchApplied(
                        patch.file,
                        patch.patchId,
                        stateGeneration,
                    )
                } else {
                    val retryReason = if (lastApplyBlockedForFileCacheConflict) {
                        "file_cache_conflict_pending"
                    } else {
                        "socket_apply_failed"
                    }
                    StateProjectionBridge.recordEditorRetryRequested(
                        patch.file,
                        patch.patchId,
                        stateGeneration,
                        retryReason,
                    )
                }
                if (applied || wasNoOp) {
                    recordDocumentActivity(patch.file, "socket-patch")
                }
                when {
                    !applied && !wasNoOp -> APPLY_FAILED
                    wasNoOp -> APPLY_ALREADY_APPLIED
                    else -> APPLY_APPLIED
                }
            }
            EditorIntent.Reposition.token -> {
                val file = extractStringField(json, "file") ?: return APPLY_FAILED
                val editorId = extractStringField(json, "editor_id")
                if (!targetsThisEditorId(editorId)) {
                    LOG.info("[socket] reposition targets editor_id ${editorId ?: "-"}; this editor is ${EditorIdentity.id}")
                    return APPLY_FAILED
                }
                val boundaryId = extractStringField(json, "boundary_id")
                val preserveHead = extractBooleanField(json, "preserve_head")
                repositionBoundaryViaDocument(file, boundaryId, preserveHead)
                recordDocumentActivity(file, "socket-reposition")
                APPLY_APPLIED
            }
            EditorIntent.RefreshContent.token -> {
                val file = extractStringField(json, "file") ?: return APPLY_FAILED
                val content = extractStringField(json, "content") ?: return APPLY_FAILED
                val expectedHash = extractStringField(json, "expected_content_hash")
                val expectedLen = extractIntField(json, "expected_content_len")
                if (refreshContentViaDocument(file, content, expectedHash, expectedLen)) {
                    recordDocumentActivity(file, "socket-refresh-content")
                    APPLY_APPLIED
                } else {
                    APPLY_FAILED
                }
            }
            EditorIntent.ObserveLazilyCurrent.token -> {
                val file = extractStringField(json, "file") ?: return APPLY_FAILED
                if (TypingTracker.observeLazilyCurrentNow(file)) {
                    recordDocumentActivity(file, "socket-publish-current-document")
                    APPLY_APPLIED
                } else {
                    APPLY_FAILED
                }
            }
            EditorIntent.DeliverCrdtRemote.token -> {
                val file = extractStringField(json, "file") ?: return APPLY_FAILED
                val editorId = extractStringField(json, "editor_id")
                if (!targetsThisEditorId(editorId)) return APPLY_FAILED
                val reasonToken = extractStringField(json, "reason")
                when (CrdtReplicaEventReason.fromToken(reasonToken)) {
                    CrdtReplicaEventReason.RequestFullState ->
                        CrdtReplicaManager.requestTextAdopt(project, file)
                    else -> Unit
                }
                // #crdtpushdrain: every controller-published frontier drains urgently.
                // Only `request_full_state` (handled above by the text-adopt path) is
                // exempt. The urgent path falls back to the gated drain when it finds
                // no work, so the no-op backoff still governs speculative polling.
                if (shouldUrgentDrainForRemoteEventUtil(reasonToken)) {
                    CrdtReplicaManager.requestUrgentRemoteDrain(
                        project,
                        file,
                        "crdt-remote-${reasonToken ?: "event"}",
                    )
                } else {
                    CrdtReplicaManager.requestRemoteDrain(project, file, "crdt-remote")
                }
                recordDocumentActivity(file, "socket-crdt-remote")
                APPLY_APPLIED
            }
            EditorIntent.RefreshVcs.token -> {
                recordProjectSurfaceOps("vcs_refresh", "refresh_vcs", "commit_vcs_refresh", "triggered")
                refreshVcs()
                APPLY_APPLIED
            }
            EditorIntent.ReloadLibrary.token -> {
                val libVersion = extractStringField(json, "lib_version") ?: "?"
                LOG.info("[socket] reload_library received (lib_version=$libVersion); forcing cdylib reload")
                AgentDocLib.forceReload()
                CrdtReplicaManager.forceRefreshOpenDocumentReplicas(project, "reload-lib-$libVersion")
                APPLY_APPLIED
            }
            EditorIntent.SaveDocument.token -> {
                val file = extractStringField(json, "file") ?: return APPLY_FAILED
                val patchId = extractStringField(json, "patch_id")
                if (saveDocumentViaDocument(file, patchId)) {
                    recordDocumentActivity(file, "socket-save-document")
                    APPLY_APPLIED
                } else {
                    APPLY_FAILED
                }
            }
            else -> {
                LOG.warn("[socket] Unknown message type: $type")
                APPLY_FAILED
            }
        }
    }

    /**
     * Flush the editor-owned markdown buffer to disk and publish the exact saved
     * content as an editor-owned content projection. This is intentionally not a full-content apply:
     * the plugin does not replace the buffer, it only asks the editor platform to
     * save the open document that already owns the user's visible text.
     */
    private fun saveDocumentViaDocument(filePath: String, patchId: String?): Boolean {
        var savedContent: String? = null
        ApplicationManager.getApplication().invokeAndWait {
            val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath)
            if (targetFile == null) {
                LOG.warn("[socket] save_document rejected: target file not found for $filePath")
                recordEditorSurfaceOps(filePath, "vcs_refresh_save", "save_document", "save_document", patchId, "missing_file")
                return@invokeAndWait
            }
            val fdm = FileDocumentManager.getInstance()
            val document = fdm.getDocument(targetFile)
            if (document == null) {
                LOG.warn("[socket] save_document rejected: no document for $filePath")
                recordEditorSurfaceOps(filePath, "vcs_refresh_save", "save_document", "save_document", patchId, "missing_document")
                return@invokeAndWait
            }

            try {
                fdm.saveDocument(document)
                savedContent = document.text
                LOG.info("[socket] save_document flushed ${savedContent?.length ?: 0} chars for $filePath")
                recordEditorSurfaceOps(filePath, "vcs_refresh_save", "save_document", "save_document", patchId, "saved")
            } catch (e: Exception) {
                LOG.warn("[socket] save_document failed for $filePath", e)
                recordEditorSurfaceOps(filePath, "vcs_refresh_save", "save_document", "save_document", patchId, "failed")
            }
        }

        val content = savedContent ?: return false
        return writeEditorContentProjection(patchId, content, filePath)
    }

    /**
     * Replace a stale post-commit editor buffer with the committed HEAD content.
     *
     * This is intentionally narrower than generic full-content IPC: it is only
     * used after the binary has restored disk to HEAD for #pcwc, and it carries
     * the stale buffer hash/length that must match before the range edit runs.
     */
    private fun refreshContentViaDocument(
        filePath: String,
        content: String,
        expectedHash: String?,
        expectedLen: Int?,
    ): Boolean {
        var applied = false
        ApplicationManager.getApplication().invokeAndWait {
            val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeAndWait
            val fdm = FileDocumentManager.getInstance()
            val document = fdm.getDocument(targetFile) ?: return@invokeAndWait
            // #p2j4 / #jbcfdiag — this socket refresh_content path is the IPC reconcile
            // that exists specifically to AVOID a behind-editor disk write; it must not
            // itself arm the File Cache Conflict dialog by refreshing an unsaved buffer.
            // The range edit below replaces only the changed span in the proven-stale buffer.
            // See shouldRefreshVfsBeforeApplyUtil.
            if (shouldRefreshVfsBeforeApplyUtil(fdm.isDocumentUnsaved(document))) {
                targetFile.refresh(false, false)
            }
            val current = document.text
            if (current == content) {
                LOG.info("[socket] refresh_content no-op: editor already matches committed content for $filePath")
                applied = true
                return@invokeAndWait
            }

            val currentHash = contentHash(current)
            if (!refreshContentPreconditionUtil(current, content, expectedHash, expectedLen)) {
                if (expectedLen != null && current.length != expectedLen) {
                    LOG.warn(
                        "[socket] refresh_content rejected for $filePath: live length ${current.length} != expected stale length $expectedLen",
                    )
                } else if (expectedHash != null && currentHash != expectedHash) {
                    LOG.warn(
                        "[socket] refresh_content rejected for $filePath: live hash ${currentHash.take(12)} != expected stale hash ${expectedHash.take(12)}",
                    )
                } else {
                    LOG.warn("[socket] refresh_content rejected for $filePath: stale-buffer proof failed")
                }
                return@invokeAndWait
            }

            val proof = EditorApplyProof(current, document.modificationStamp)
            WriteCommandAction.runWriteCommandAction(project, "Agent Doc Refresh Content", null, {
                if (!editorApplyProofStillCurrentUtil(proof, document.text, document.modificationStamp)) {
                    LOG.warn("[socket] stale editor generation during refresh_content for $filePath; rejecting")
                    return@runWriteCommandAction
                }
                LOG.info(
                    documentMutationDiagnosticUtil(
                        "refreshContent.postcommit",
                        filePath,
                        expectedHash?.take(12),
                        "socket_refresh_content",
                        current,
                        content,
                        document.modificationStamp,
                        true,
                    ),
                )
                if (LOG.isDebugEnabled) {
                    LOG.debug(
                        "[patch-watcher] minimal-edit target (refresh_content) for $filePath (${content.length} chars):\n$content",
                    )
                }
                CrdtReplicaManager.withAgentAppliedEditorMutation(filePath) {
                    applyMinimalDocumentEditUtil(document, proof.content, content)
                }
                applied = true
            })
        }
        return applied
    }

    /**
     * Reposition boundary marker in a document via Document API.
     * Used by socket IPC "reposition" messages.
     *
     * Applies immediately on the EDT. The CPC/binary owns debounce and retry
     * scheduling before it sends editor IPC.
     */
    private fun repositionBoundaryViaDocument(filePath: String, boundaryId: String? = null, preserveHead: Boolean = false) {
        ApplicationManager.getApplication().invokeLater {
            val reposStart = System.nanoTime()
            val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeLater
            val fdm = FileDocumentManager.getInstance()
            val document = fdm.getDocument(targetFile) ?: return@invokeLater
            // #p2j4 / #jbcfdiag — only refresh the VFS when the buffer is clean;
            // a content-bearing refresh against an unsaved buffer arms the platform
            // File Cache Conflict dialog. See shouldRefreshVfsBeforeApplyUtil.
            if (shouldRefreshVfsBeforeApplyUtil(fdm.isDocumentUnsaved(document))) {
                targetFile.refresh(false, false)
            }
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
                    // Capture the exact target payload at debug level for
                    // IPC-corruption forensics.
                    if (LOG.isDebugEnabled) {
                        LOG.debug(
                            "[patch-watcher] minimal-edit target (repositionBoundary) for $filePath boundaryId=$boundaryId (${result.length} chars):\n$result"
                        )
                    }
                    CrdtReplicaManager.withAgentAppliedEditorMutation(filePath) {
                        applyMinimalDocumentEditUtil(document, content, result)
                    }
                } else if (result != content) {
                    LOG.warn("[patch-watcher] stale editor generation during reposition for $filePath")
                }
            })
            val reposMs = (System.nanoTime() - reposStart) / 1_000_000
            if (reposMs > 50) LOG.info("[perf] repositionBoundary: ${reposMs}ms $filePath")
        }
    }

    /**
     * Write the final document content to the derived content projection via FFI.
     * Called after every successful apply so the CLI binary can pair the
     * projection with lazily receipt state instead of a timing heuristic.
     * Keyed by patch_id (not file path) — all path logic lives in Rust.
     */
    private fun writeEditorContentProjection(patchId: String?, content: String, filePath: String? = null): Boolean {
        if (patchId == null) return true
        val root = filePath?.let { resolveRootFor(it) } ?: project.basePath ?: return false
        val lib = AgentDocLib.get() ?: run {
            LOG.warn("[content-projection] FFI unavailable, cannot write content projection for patch_id $patchId")
            return false
        }
        if (filePath != null) {
            try {
                if (lib.agent_doc_editor_content_applied_for_editor_v1(
                    root,
                    patchId,
                    filePath,
                    content,
                    EditorIdentity.id,
                    "jetbrains",
                    patchWatcherPluginVersion(),
                    editorCapabilities,
                )) {
                    recordEditorSurfaceOps(filePath, "content_projection", "editor_content_applied_for_editor_v1", "write_finalize_ipc", patchId, "ok")
                    return true
                }
                LOG.warn("[content-projection] FFI editor_content_applied_for_editor_v1 returned false for patch_id $patchId")
                recordEditorSurfaceOps(filePath, "content_projection", "editor_content_applied_for_editor_v1", "write_finalize_ipc", patchId, "failed")
                return false
            } catch (_: UnsatisfiedLinkError) {
                LOG.warn("[content-projection] incompatible agent-doc native library: missing agent_doc_editor_content_applied_for_editor_v1; reinstall the plugin/native library")
                recordEditorSurfaceOps(filePath, "content_projection", "editor_content_applied_for_editor_v1", "write_finalize_ipc", patchId, "missing_symbol")
                return false
            } catch (_: NoSuchMethodError) {
                LOG.warn("[content-projection] incompatible agent-doc native library: missing agent_doc_editor_content_applied_for_editor_v1; reinstall the plugin/native library")
                recordEditorSurfaceOps(filePath, "content_projection", "editor_content_applied_for_editor_v1", "write_finalize_ipc", patchId, "missing_symbol")
                return false
            }
        }
        LOG.warn("[content-projection] file path is required for lazily receipt-capable content publication")
        return false
    }

    private fun scheduleRepositionRetry(filePath: String, boundaryId: String?, preserveHead: Boolean) {
        val key = "$filePath|${boundaryId ?: ""}|$preserveHead"
        if (!scheduledRepositionRetries.add(key)) return
        com.intellij.util.concurrency.AppExecutorUtil.getAppExecutorService().submit {
            try {
                Thread.sleep(PATCH_RETRY_DELAY_MS)
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

    private fun nodePatchesJson(nodePatches: List<NodePatch>): String {
        return nodePatchesJsonStatic(nodePatches)
    }

    private fun applyPatch(patch: IpcPatch): Boolean {
        lastApplyWasNoOp = false
        lastApplyBlockedForFileCacheConflict = false

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

        // Reload document from disk if it was externally modified
        val fdm = FileDocumentManager.getInstance()
        val document = fdm.getDocument(targetFile)

        // #p2j4 / #jbcfdiag — refresh the VFS from disk only when the document is
        // saved (clean). A content-bearing refresh against an UNSAVED buffer is what
        // arms IntelliJ's memory↔disk "File Cache Conflict" dialog behind the editor;
        // for an unsaved buffer the apply path below reconciles via the Document API
        // (and the unsaved branch deliberately does NOT reload from disk anyway), so
        // the refresh adds no value and only triggers the dialog. See
        // shouldRefreshVfsBeforeApplyUtil.
        if (document == null || shouldRefreshVfsBeforeApplyUtil(fdm.isDocumentUnsaved(document))) {
            targetFile.refresh(false, false)
        }

        if (document == null) {
            // File not open in editor — apply patches via VFS (no Document API needed).
            // This avoids the "externally modified" dialog for background tabs.
            LOG.info("No document for ${patch.file}, applying via VFS")
            return applyPatchViaVfs(targetFile, patch)
        }

        if (hasPendingMemoryDiskConflict(targetFile)) {
            lastApplyBlockedForFileCacheConflict = true
            val proof = fileCacheConflictProof(patch, document, targetFile, fdm)
            recordFileCacheConflictOps(
                patch,
                "blocked",
                "editor_ipc_convergence",
                "apply_patch",
                "write_finalize_ipc",
                proof,
            )
            LOG.warn(
                "[patch-watcher] File Cache Conflict pending for ${patch.file}; rejecting patch without mutating document " +
                    "$proof $UI_OUTCOME_REAL_COMPONENT_CONFLICT"
            )
            refreshVisualHighlightersAfterFileCacheConflict(targetFile, "blocked")
            return false
        }

        if (fdm.isDocumentUnsaved(document)) {
            // Document has unsaved changes — don't reload (preserve user edits)
        } else {
            // Reload from disk to pick up boundary changes from agent-doc boundary
            fdm.reloadFromDisk(document)
        }

        // Compute the patched result OUTSIDE the write action to avoid
        // blocking the EDT for no-op patches. Only acquire the write lock
        // if the content actually changed.
        val content = document.text
        val proof = EditorApplyProof(content, document.modificationStamp)

        if (isPatchGenerationSuperseded(patch, content)) {
            LOG.info("[patch-watcher] generation fence: rejecting superseded live-buffer patch for ${patch.file}")
            return false
        }

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
            if (!writeEditorContentProjection(patch.patchId, diskContent ?: content, patch.file)) {
                return false
            }
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

        val nodePatchNativeAvailable = patch.nodePatches.isNotEmpty() && NativePatching.canApplyNodePatches()
        val nodePatchedComponents = patch.nodePatches.map { it.component }.toSet()
        if (nodePatchNativeAvailable) {
            result = NativePatching.applyNodePatches(result, nodePatchesJson(patch.nodePatches)) ?: run {
                LOG.warn("[patch-watcher] native node-patch apply rejected patch_id ${patch.patchId} for ${patch.file}")
                return false
            }
        }

        for (p in patch.patches) {
            if (nodePatchNativeAvailable && p.component in nodePatchedComponents) {
                LOG.info("[patch-watcher] skipping legacy component patch for node-patched component ${p.component}")
                continue
            }
            val effectiveBoundaryId = if (p.ensureBoundary && p.boundaryId == null) {
                findBoundaryInComponent(result, p.component)
            } else {
                p.boundaryId
            }
            result = applyComponentPatchNative(result, p.component, p.content, effectiveBoundaryId, p.op)
        }

        // Apply unmatched content to exchange or output component
        if (patch.unmatched.isNotBlank()) {
            val exchangeResult = applyComponentPatchNative(result, "exchange", patch.unmatched)
            result = if (exchangeResult != result) exchangeResult
                else applyComponentPatchNative(result, "output", patch.unmatched)
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
        // the prior boundary are included in the user region seen by the document cell.
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
            if (!writeEditorContentProjection(patch.patchId, document.text, patch.file)) {
                return false
            }
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
            // The diagnostic above logs only hashes +
            // lengths. To capture the exact corrupting payload for the IPC
            // duplication family, log the full target payload at debug level
            // (only when `#com.github.btakita.agentdoc` debug logging is on).
            if (LOG.isDebugEnabled) {
                LOG.debug(
                    "[patch-watcher] minimal-edit target (applyPatch.component) for ${patch.file} patchId=${patch.patchId} (${result.length} chars):\n$result"
                )
            }
            CrdtReplicaManager.withAgentAppliedEditorMutation(patch.file) {
                applyMinimalDocumentEditUtil(document, content, result)
            }
            wrote = true
            LOG.info("Patch applied to ${patch.file} (${result.length - content.length} chars changed)")
        })
        if (!wrote) {
            return false
        }

        if (!writeEditorContentProjection(patch.patchId, document.text, patch.file)) {
            return false
        }
        // Note: do NOT call agent_doc_commit here. The plugin committing within the IPC
        // window races with the skill's `agent-doc commit` call, causing the binary commit
        // to be a no-op (FFI already committed). The binary's git::commit handles boundary
        // markers and HEAD repositioning; the FFI commit skips all of that. The preflight
        // sweep (Fix 5) handles missed commits as a backstop for interrupted sessions.
        return true
    }

    private fun patchConflictKey(patch: IpcPatch): String =
        patch.patchId ?: patch.file

    private fun fileCacheConflictProof(
        patch: IpcPatch,
        document: Document,
        targetFile: VirtualFile,
        fdm: FileDocumentManager,
    ): String {
        return "conflict_key=${patchConflictKey(patch)} patch_id=${patch.patchId ?: "-"} " +
            "document_unsaved=${fdm.isDocumentUnsaved(document)} document_stamp=${document.modificationStamp} " +
            "file_stamp=${targetFile.modificationStamp}"
    }

    private fun recordFileCacheConflictOps(
        patch: IpcPatch,
        outcome: String,
        surface: String,
        action: String,
        agentCommand: String,
        proof: String,
    ) {
        val root = resolveRootFor(patch.file) ?: return
        val relativePath = File(patch.file).relativeToOrSelf(File(root)).path
        appendOpsLog(
            root,
            buildFileCacheConflictOpsLogLine(
                timestamp = Instant.ofEpochSecond(Instant.now().epochSecond).toString(),
                relativePath = relativePath,
                outcome = outcome,
                surface = surface,
                action = action,
                agentCommand = agentCommand,
                proof = proof,
            ),
            "[patch-watcher]",
        )
    }

    private fun recordEditorSurfaceOps(
        filePath: String,
        surface: String,
        action: String,
        agentCommand: String,
        patchId: String?,
        status: String,
    ) {
        val root = resolveRootFor(filePath) ?: return
        val relativePath = File(filePath).relativeToOrSelf(File(root)).path
        appendOpsLog(
            root,
            buildEditorSurfaceOpsLogLine(
                timestamp = Instant.ofEpochSecond(Instant.now().epochSecond).toString(),
                relativePath = relativePath,
                surface = surface,
                action = action,
                agentCommand = agentCommand,
                patchId = patchId,
                status = status,
            ),
            "[patch-watcher]",
        )
    }

    private fun recordProjectSurfaceOps(
        surface: String,
        action: String,
        agentCommand: String,
        status: String,
    ) {
        val root = project.basePath ?: return
        appendOpsLog(
            root,
            buildEditorSurfaceOpsLogLine(
                timestamp = Instant.ofEpochSecond(Instant.now().epochSecond).toString(),
                relativePath = ".",
                surface = surface,
                action = action,
                agentCommand = agentCommand,
                patchId = null,
                status = status,
            ),
            "[patch-watcher]",
        )
    }

    private fun appendOpsLog(root: String, line: String, logPrefix: String) {
        try {
            val agentDocDir = File(root, ".agent-doc")
            if (!agentDocDir.isDirectory) return
            val logsDir = File(agentDocDir, "logs")
            if (!logsDir.isDirectory && !logsDir.mkdirs()) {
                LOG.warn("$logPrefix failed to create ops.log directory at ${logsDir.path}")
                return
            }
            File(logsDir, "ops.log").appendText(line + System.lineSeparator())
        } catch (e: Exception) {
            LOG.warn("$logPrefix failed to append ops.log marker: ${e.message}", e)
        }
    }

    private fun refreshVisualHighlightersAfterFileCacheConflict(targetFile: VirtualFile, outcome: String) {
        try {
            VisualHighlighterManager.getInstance(project).refreshFile(targetFile)
            LOG.info("[patch-watcher] refreshed visual highlighters after File Cache Conflict $outcome for ${targetFile.path}")
        } catch (e: Exception) {
            LOG.warn("[patch-watcher] unable to refresh visual highlighters after File Cache Conflict $outcome for ${targetFile.path}", e)
        }
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
                LOG.warn("[patch-watcher] unable to inspect IntelliJ File Cache Conflict state; proceeding without conflict guard", e)
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
     * Closed-file VFS patch handling is read-only during realtime cutover.
     * It may accept a stale replay already present on disk/HEAD, but it must not
     * synthesize and write a whole-buffer replacement outside editor convergence.
     */
    private fun applyPatchViaVfs(targetFile: com.intellij.openapi.vfs.VirtualFile, patch: IpcPatch): Boolean {
        try {
            val content = String(targetFile.contentsToByteArray(), targetFile.charset)

            if (!patch.fullContent.isNullOrEmpty()) {
                LOG.warn("[patch-watcher] VFS full-content IPC is disabled; rejecting patch_id ${patch.patchId} for ${patch.file}")
                return false
            }

            if (patchReplayAlreadyPresentUtil(
                    patch,
                    listOf(content),
                ) { payload -> NativePatching.patchContentAlreadyCommitted(patch.file, payload) }
            ) {
                LOG.info("[patch-watcher] dedup: VFS response patch_id ${patch.patchId} already present in disk/committed content — skipping stale replay")
                if (!writeEditorContentProjection(patch.patchId, content, patch.file)) {
                    return false
                }
                lastApplyWasNoOp = true
                return true
            }

            LOG.warn("[patch-watcher] VFS whole-buffer patch apply is disabled; rejecting patch_id ${patch.patchId} for ${patch.file}")
            return false
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
    private fun applyComponentPatchNative(doc: String, component: String, content: String, boundaryId: String? = null, modeOverride: String? = null): String {
        val mode = componentPatchModeOverrideUtil(modeOverride) ?: extractComponentMode(doc, component)
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
                // Re-compute git gutter annotations without recursively refreshing
                // project content. Open agent-doc Documents may intentionally be
                // unsaved while CRDT/editor delivery converges; a workspace-wide
                // VFS refresh turns that safe transient state into IntelliJ's
                // memory-vs-disk File Cache Conflict dialog. Content-bearing apply
                // paths refresh only their clean target file immediately before
                // Document API mutation.
                VcsDirtyScopeManager.getInstance(project).markEverythingDirty()
                LOG.info("[vcs] Triggered VCS dirty-scope refresh after external commit without content VFS refresh (debounced)")
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
            try { lib?.agent_doc_stop_ipc_listener(state.root) } catch (_: Exception) {}
            state.ipcCallback = null
        }
        rootStates.clear()
    }

    companion object {
        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(PatchWatcher::class.java)
        private val instances = mutableMapOf<Project, PatchWatcher>()
        private const val PATCH_RETRY_DELAY_MS = 500L
        private const val UI_OUTCOME_REAL_COMPONENT_CONFLICT =
            "ui_outcome_contract=ui-outcome-v1 ui_outcome=real_component_conflict ui_outcome_class=blocked next_action=resolve_component_conflict"

        const val APPLY_FAILED = 0
        const val APPLY_APPLIED = 1
        const val APPLY_ALREADY_APPLIED = 2

        /** Compute SHA256 hex of content bytes — mirrors debounce::content_hash in the Rust binary. */
        fun contentHash(content: String): String {
            val digest = MessageDigest.getInstance("SHA-256")
            val bytes = digest.digest(content.toByteArray(Charsets.UTF_8))
            return bytes.joinToString("") { "%02x".format(it) }
        }

        fun generationFenceContentHash(content: String): String =
            contentHash(normalizeGenerationFenceContent(content))

        /**
         * Apply-side generation fence (`#late-ipc-patch-plugin-apply-fence`).
         *
         * A queued file patch tagged by the binary
         * ([queue_file_ipc_reposition_boundary] in write.rs) carries a generation
         * token: `cycle_id` (the cycle that produced it), `baseline_hash`
         * (SHA-256 of the live doc the patch targeted), and optionally
         * `baseline_normalized_hash` (the same baseline after transient
         * agent-doc markers are stripped). A LATE applier — this watcher,
         * possibly running minutes after the producing cycle committed — must
         * DROP a patch whose generation is already superseded instead of
         * re-applying it and re-materializing a duplicate `### Re:` block:
         *
         *  - **cycle already committed:** the document's persisted `cycle_state`
         *    advanced the SAME `cycle_id` this patch was produced in to
         *    `committed`. The cycle is closed; replaying its reposition can only
         *    duplicate (mirrors the write-side `try_ipc` `already_committed` guard).
         *  - **baseline drift:** the live doc no longer hashes to `baseline_hash`
         *    or, when provided, `baseline_normalized_hash`. A later cycle rewrote
         *    the doc, so the patch targets a stale baseline. Transient `(HEAD)` /
         *    boundary marker churn is tolerated by the normalized token.
         *
         * Fails OPEN: returns false on any IO/parse error or when the patch
         * carries no generation token, so a legitimate patch is never dropped
         * because state could not be read.
         */
        fun isPatchGenerationSuperseded(patch: IpcPatch, liveContent: String?): Boolean {
            // Baseline drift: live doc moved on from the baseline this patch targeted.
            if ((patch.baselineHash != null || patch.baselineNormalizedHash != null) && liveContent != null) {
                val rawMatches = patch.baselineHash != null && contentHash(liveContent) == patch.baselineHash
                val normalizedMatches = patch.baselineNormalizedHash != null &&
                    generationFenceContentHash(liveContent) == patch.baselineNormalizedHash
                if (!rawMatches && !normalizedMatches) {
                    if (patch.nodePatches.isNotEmpty() && nodePatchTargetsStillCurrent(patch, liveContent)) {
                        LOG.info(
                            "[patch-watcher] generation fence: full document drifted but node patch targets are still current for ${patch.file}",
                        )
                        return false
                    }
                    LOG.info(
                        "[patch-watcher] generation fence: baseline drift (live doc moved on from queued baseline) for ${patch.file}",
                    )
                    return true
                }
            }
            return false
        }

        fun nodePatchTargetsStillCurrent(patch: IpcPatch, liveContent: String): Boolean {
            if (patch.nodePatches.isEmpty()) return false
            val nodePatchedComponents = patch.nodePatches.map { it.component }.toSet()
            if (patch.patches.any { it.component !in nodePatchedComponents }) return false
            if (patch.unmatched.isNotBlank() || !patch.frontmatter.isNullOrBlank() || patch.queueAuto != null) {
                return false
            }
            if (patch.nodePatches.any { it.op != "insert" && it.expectedContent == null }) {
                return false
            }
            return NativePatching.applyNodePatches(liveContent, nodePatchesJsonStatic(patch.nodePatches)) != null
        }

        fun nodePatchesJsonStatic(nodePatches: List<NodePatch>): String {
            val array = com.google.gson.JsonArray()
            for (patch in nodePatches) {
                val obj = com.google.gson.JsonObject()
                obj.addProperty("component", patch.component)
                obj.addProperty("node_key", patch.nodeKey)
                obj.addProperty("op", patch.op)
                patch.content?.let { obj.addProperty("content", it) }
                patch.expectedContent?.let { obj.addProperty("expected_content", it) }
                patch.expectedContentHash?.let { obj.addProperty("expected_content_hash", it) }
                patch.before?.let { obj.addProperty("before", it) }
                patch.after?.let { obj.addProperty("after", it) }
                if (patch.order.isNotEmpty()) {
                    val order = com.google.gson.JsonArray()
                    patch.order.forEach { order.add(it) }
                    obj.add("order", order)
                }
                array.add(obj)
            }
            return array.toString()
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
    /** UUID identifying this delivery and its Lazily receipt. */
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
    /**
     * Generation token for editor-visible socket convergence. SHA-256 hex of the
     * targeted live document after transient agent-doc markers are stripped.
     */
    val baselineNormalizedHash: String? = null,
    /** Node-keyed mutation plan carried alongside legacy component patches. */
    val nodePatches: List<NodePatch> = emptyList(),
    /** Target editor id for per-editor broadcast patches. */
    val editorId: String? = null,
    /** Originating editor id for echo suppression. */
    val originEditorId: String? = null,
) {
    fun targetsThisEditor(): Boolean {
        return targetsThisEditorId(editorId)
    }
}

private fun targetsThisEditorId(editorId: String?): Boolean {
    return editorId == EditorIdentity.id
}

data class ComponentPatch(
    val component: String,
    val content: String,
    val boundaryId: String? = null,
    val ensureBoundary: Boolean = false,
    val op: String? = null,
    val nodeId: String? = null,
)

data class NodePatch(
    val component: String,
    val nodeKey: String,
    val op: String,
    val content: String? = null,
    val expectedContent: String? = null,
    val expectedContentHash: String? = null,
    val before: String? = null,
    val after: String? = null,
    val order: List<String> = emptyList(),
)

internal data class EditorApplyProof(
    val content: String,
    val modificationStamp: Long,
)

internal data class MinimalDocumentEdit(
    val start: Int,
    val end: Int,
    val replacement: String,
)

internal fun minimalDocumentEditUtil(before: String, after: String): MinimalDocumentEdit? {
    if (before == after) {
        return null
    }

    var prefixLen = 0
    val minLen = minOf(before.length, after.length)
    while (prefixLen < minLen && before[prefixLen] == after[prefixLen]) {
        prefixLen++
    }

    var suffixLen = 0
    while (
        suffixLen < before.length - prefixLen &&
        suffixLen < after.length - prefixLen &&
        before[before.length - 1 - suffixLen] == after[after.length - 1 - suffixLen]
    ) {
        suffixLen++
    }

    return MinimalDocumentEdit(
        start = prefixLen,
        end = before.length - suffixLen,
        replacement = after.substring(prefixLen, after.length - suffixLen),
    )
}

internal fun applyMinimalDocumentEditUtil(
    document: Document,
    before: String,
    after: String,
): Boolean {
    val edit = minimalDocumentEditUtil(before, after) ?: return false
    document.replaceString(edit.start, edit.end, edit.replacement)
    return document.text == after
}

/**
 * `#p2j4` / `#jbcfdiag` — decide whether a content-bearing `VirtualFile.refresh`
 * is safe to run before applying an agent write.
 *
 * IntelliJ's `FileDocumentManagerImpl` arms its memory↔disk "File Cache Conflict"
 * dialog *during* a VFS refresh: when the on-disk bytes diverge from an **unsaved**
 * in-memory `Document`, the platform's `contentsChanged` handler pops the dialog
 * synchronously. Every agent write path that reconciles the document through the
 * Document API is the authority for an unsaved buffer, so a bare
 * `refresh(false, false)` before that reconcile adds no value when the document is
 * unsaved — it only hands the platform a window to fire the dialog behind the
 * editor (the remaining trigger after IPC-first writes removed the Rust-side
 * behind-editor disk writes).
 *
 * Therefore: refresh the VFS from disk only when the document is **saved** (clean).
 * When the document is unsaved, skip the refresh — the in-memory buffer is the
 * source of truth and the apply path (or `mergeOrReload`) reconciles any disk
 * divergence through the Document API without arming the platform dialog. The
 * `--force-disk` operator escape hatch is a Rust-side flag and is unaffected.
 */
internal fun shouldRefreshVfsBeforeApplyUtil(documentUnsaved: Boolean): Boolean =
    !documentUnsaved

internal fun editorApplyProofStillCurrentUtil(
    proof: EditorApplyProof,
    currentContent: String,
    currentModificationStamp: Long,
): Boolean =
    proof.modificationStamp == currentModificationStamp && proof.content == currentContent

internal fun refreshContentPreconditionUtil(
    currentContent: String,
    targetContent: String,
    expectedHash: String?,
    expectedLen: Int?,
): Boolean {
    if (currentContent == targetContent) {
        return true
    }
    if (expectedLen != null && currentContent.length != expectedLen) {
        return false
    }
    if (expectedHash != null && PatchWatcher.contentHash(currentContent) != expectedHash) {
        return false
    }
    return true
}

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

internal fun componentPatchModeOverrideUtil(op: String?): String? =
    when (op?.trim()?.lowercase()) {
        "append", "prepend", "replace" -> op.trim().lowercase()
        else -> null
    }

internal fun findCodeBlockRangesUtil(doc: String): List<Pair<Int, Int>> {
    // Mirror the Rust `component::find_code_ranges` fence handling (#npe1 /
    // #codefencestrip): a real fenced code block is delimited by a matching
    // fence *character* (``` or ~~~) and *length* (>= the opener's run length).
    // The previous naive "every ``` line toggles a block, ~~~ ignored" pairing
    // desynced whenever the fence count went odd (a lone ``` in prose, a
    // backtick run inside a ~~~ block, or a ~~~-only block), which could let a
    // mis-paired range swallow the boundary / `<!-- /agent:exchange -->` region
    // and strip a trailing code fence at the exchange boundary.
    val ranges = mutableListOf<Pair<Int, Int>>()
    var inFence = false
    var fenceChar = '\u0000'
    var fenceLen = 0
    var fenceStart = 0
    var offset = 0

    for (segment in splitInclusiveNewline(doc)) {
        val lineStart = offset
        offset += segment.length
        val lineEnd = offset
        val trimmed = segment.trimEnd('\n', '\r').trimStart()
        val first = trimmed.firstOrNull()
        val runLen = if (first == null) 0 else trimmed.takeWhile { it == first }.length
        val opensOrCloses = (first == '`' || first == '~') && runLen >= 3

        if (!inFence) {
            if (opensOrCloses) {
                inFence = true
                fenceChar = first!!
                fenceLen = runLen
                fenceStart = lineStart
            }
        } else if (first == fenceChar && runLen >= fenceLen && trimmed.drop(runLen).trim().isEmpty()) {
            // Closing fence: include the full closing-fence line in the range.
            inFence = false
            ranges.add(Pair(fenceStart, lineEnd))
        }
    }

    // An unterminated fence runs to end of document (matches CommonMark: the
    // rest of the input is code).
    if (inFence) {
        ranges.add(Pair(fenceStart, doc.length))
    }

    return ranges
}

/** Split `doc` into line segments that each retain their trailing newline. */
private fun splitInclusiveNewline(doc: String): List<String> {
    val segments = mutableListOf<String>()
    var start = 0
    var i = 0
    while (i < doc.length) {
        if (doc[i] == '\n') {
            segments.add(doc.substring(start, i + 1))
            start = i + 1
        }
        i++
    }
    if (start < doc.length) {
        segments.add(doc.substring(start))
    }
    return segments
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

private fun normalizeGenerationFenceContent(content: String): String {
    val withoutBoundaries = rustStyleLines(content)
        .filterNot { it.trim().startsWith("<!-- agent:boundary:") }
        .joinToString("\n")
    val withoutHeadMarkers = stripTransientHeadMarkers(withoutBoundaries)
    val withoutGuardMarkers = stripGenerationFenceGuardMarkers(withoutHeadMarkers)
    val withoutPipeline = stripGenerationFencePipelineBlock(withoutGuardMarkers)
    return rustStyleLines(withoutPipeline).joinToString("\n")
}

private fun rustStyleLines(content: String): List<String> {
    val normalized = content.replace("\r\n", "\n").replace('\r', '\n')
    if (normalized.isEmpty()) return emptyList()
    val lines = normalized.split("\n")
    return if (normalized.endsWith("\n")) lines.dropLast(1) else lines
}

private fun stripGenerationFenceGuardMarkers(content: String): String {
    val markers = listOf("<!-- no-pending-capture -->", "<!-- no-pending-done-guard -->")
    val result = mutableListOf<String>()
    for (line in rustStyleLines(content)) {
        if (markers.any { marker -> line.trim() == marker }) {
            continue
        }
        if (markers.any { marker -> line.contains(marker) }) {
            var cleaned = line
            for (marker in markers) {
                cleaned = cleaned.replace(marker, "")
            }
            result.add(cleaned.trimEnd())
        } else {
            result.add(line)
        }
    }
    return result.joinToString("\n")
}

private fun stripGenerationFencePipelineBlock(content: String): String {
    val lines = content.split("\n")
    if (lines.firstOrNull()?.trimEnd() != "---") return content
    val closeIdx = lines.withIndex()
        .drop(1)
        .firstOrNull { it.value.trimEnd() == "---" }
        ?.index ?: return content

    val out = mutableListOf<String>()
    var skipping = false
    for ((index, line) in lines.withIndex()) {
        if (index == 0 || index >= closeIdx) {
            skipping = false
            out.add(line)
            continue
        }
        if (skipping) {
            if (line.startsWith(" ") || line.startsWith("\t")) {
                continue
            }
            skipping = false
        }
        if (line.trimStart().startsWith("agent_doc_pipeline:")) {
            skipping = true
            continue
        }
        out.add(line)
    }
    return out.joinToString("\n")
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

        if (line.endsWith(" (HEAD)")) {
            val stripped = line.removeSuffix(" (HEAD)")
            val withoutSuffix = stripped.trimEnd()
            if (Regex("""^\s*#{1,6}\s""").containsMatchIn(line) ||
                (trimmed.startsWith("**") && withoutSuffix.trimStart().endsWith("**"))
            ) {
                result.add(stripped)
                continue
            }
        }
        result.add(line)
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
 * user-prompt region; comparing it pre/post an editor-visible write reveals prompt
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
 * Structured diagnostic for an editor-visible mutation.
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

internal fun buildFileCacheConflictOpsLogLine(
    timestamp: String,
    relativePath: String,
    outcome: String,
    surface: String,
    action: String,
    agentCommand: String,
    proof: String,
): String {
    val doc = File(relativePath).nameWithoutExtension.ifBlank { "unknown" }
    return "[$timestamp] file_cache_conflict_detected source=jetbrains outcome=$outcome " +
        "surface=$surface action=$action agent_command=$agentCommand file=$relativePath $proof doc=$doc #cyh0"
}

internal fun buildEditorSurfaceOpsLogLine(
    timestamp: String,
    relativePath: String,
    surface: String,
    action: String,
    agentCommand: String,
    patchId: String?,
    status: String,
): String {
    val doc = File(relativePath).nameWithoutExtension.ifBlank { "project" }
    return "[$timestamp] editor_surface_event source=jetbrains surface=$surface action=$action " +
        "agent_command=$agentCommand status=$status file=$relativePath patch_id=${patchId ?: "-"} doc=$doc #cyh0"
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
        val editorId = root.get("editor_id")?.let { if (it.isJsonNull) null else it.asString }
        val originEditorId = root.get("origin_editor_id")?.let { if (it.isJsonNull) null else it.asString }
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
            val op = obj.get("op")?.asString
            val nodeId = obj.get("node_id")?.asString
            patches.add(ComponentPatch(component, content, boundaryId, ensureBoundary, op, nodeId))
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
        val baselineNormalizedHash = root.get("baseline_normalized_hash")?.let { if (it.isJsonNull) null else it.asString }
        val nodePatches = root.getAsJsonArray("node_patches")
            ?.mapNotNull { elem ->
                val obj = elem.asJsonObject
                val component = obj.get("component")?.asString ?: return@mapNotNull null
                val nodeKey = obj.get("node_key")?.asString ?: return@mapNotNull null
                val op = obj.get("op")?.asString ?: return@mapNotNull null
                val order = obj.getAsJsonArray("order")?.mapNotNull { it.asString } ?: emptyList()
                NodePatch(
                    component,
                    nodeKey,
                    op,
                    obj.get("content")?.let { if (it.isJsonNull) null else it.asString },
                    obj.get("expected_content")?.let { if (it.isJsonNull) null else it.asString },
                    obj.get("expected_content_hash")?.let { if (it.isJsonNull) null else it.asString },
                    obj.get("before")?.let { if (it.isJsonNull) null else it.asString },
                    obj.get("after")?.let { if (it.isJsonNull) null else it.asString },
                    order,
                )
            } ?: emptyList()
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
            baselineNormalizedHash,
            nodePatches,
            editorId,
            originEditorId,
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

private fun extractIntField(json: String, field: String): Int? {
    return try {
        com.google.gson.JsonParser.parseString(json).asJsonObject.get(field)?.asInt
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
