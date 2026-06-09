package com.github.btakita.agentdoc

import com.sun.jna.Callback
import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.Structure
import java.io.File

internal fun libMtimeChanged(path: String, storedMtime: Long): Boolean {
    val currentMtime = File(path).lastModified()
    return currentMtime != storedMtime && currentMtime != 0L
}

/**
 * JNA bindings to libagent_doc shared library.
 *
 * Replaces duplicated Kotlin logic for component patching, frontmatter merge,
 * and code block detection with FFI calls to the canonical Rust implementation.
 */
interface AgentDocLib : Library {

    /**
     * Result of [agent_doc_apply_patch].
     *
     * Rust returns this struct by value (`#[repr(C)]`). The binding therefore
     * uses [ByValue] so JNA reads the struct's fields directly from the
     * return registers (SysV ABI) instead of dereferencing them as a pointer.
     * See editors/jetbrains/docs/jna-by-value.md (or VERSIONS.md 0.2.59).
     */
    @Structure.FieldOrder("text", "error")
    open class FfiPatchResult : Structure() {
        @JvmField var text: Pointer? = null
        @JvmField var error: Pointer? = null
        class ByValue : FfiPatchResult(), Structure.ByValue
    }

    /** Result of [agent_doc_parse_components]. Returned by value — see [FfiPatchResult]. */
    @Structure.FieldOrder("json", "count")
    open class FfiComponentList : Structure() {
        @JvmField var json: Pointer? = null
        @JvmField var count: Long = 0
        class ByValue : FfiComponentList(), Structure.ByValue
    }

    /** Result of [agent_doc_resolve_project_path]. Returned by value — see [FfiPatchResult]. */
    @Structure.FieldOrder("project_root", "relative_path")
    open class FfiProjectPath : Structure() {
        @JvmField var project_root: Pointer? = null
        @JvmField var relative_path: Pointer? = null
        class ByValue : FfiProjectPath(), Structure.ByValue
    }

    /** JSON result returned by controller-backed admin/editor wrappers. */
    @Structure.FieldOrder("json", "error")
    open class FfiJsonResult : Structure() {
        @JvmField var json: Pointer? = null
        @JvmField var error: Pointer? = null
        class ByValue : FfiJsonResult(), Structure.ByValue
    }

    /**
     * Apply a patch to a document component.
     * Mode: "replace", "append", or "prepend".
     */
    fun agent_doc_apply_patch(
        doc: String,
        component_name: String,
        content: String,
        mode: String,
    ): FfiPatchResult.ByValue

    /**
     * Apply a patch with cursor-aware ordering for append mode.
     * When mode is "append" and caretOffset >= 0, inserts content before the caret.
     * Pass caretOffset = -1 for normal behavior.
     */
    fun agent_doc_apply_patch_with_caret(
        doc: String,
        component_name: String,
        content: String,
        mode: String,
        caret_offset: Int,
    ): FfiPatchResult.ByValue

    /**
     * Apply a patch using a boundary marker for insertion point.
     * When mode is "append" and boundary_id is found in the component,
     * inserts content at the boundary marker position.
     */
    fun agent_doc_apply_patch_with_boundary(
        doc: String,
        component_name: String,
        content: String,
        mode: String,
        boundary_id: String,
    ): FfiPatchResult.ByValue

    /**
     * Merge YAML key/value pairs into a document's frontmatter.
     */
    fun agent_doc_merge_frontmatter(
        doc: String,
        yaml_fields: String,
    ): FfiPatchResult.ByValue

    /**
     * Converge the `agent:queue` opening-tag `auto` attribute.
     * `want_auto` is a C int (nonzero = ensure `auto`, zero = strip `auto`); a
     * content patch cannot change an opening-tag attribute, so this is the
     * convergence seam for #adoc-queue-ipc-buffer-divergence.
     */
    fun agent_doc_converge_queue_auto(
        doc: String,
        want_auto: Int,
    ): FfiPatchResult.ByValue

    /**
     * Reposition boundary marker to end of exchange component.
     * Removes all stale boundaries and inserts a single fresh 8-char one.
     * Strips transient (HEAD) markers.
     */
    fun agent_doc_reposition_boundary_to_end(doc: String): FfiPatchResult.ByValue

    /**
     * Reposition boundary marker to end of exchange component using an explicit ID.
     * Strips transient (HEAD) markers.
     */
    fun agent_doc_reposition_boundary_to_end_with_id(
        doc: String,
        boundary_id: String,
    ): FfiPatchResult.ByValue

    /**
     * Reposition boundary marker to end of exchange component, preserving (HEAD) markers.
     */
    fun agent_doc_reposition_boundary_to_end_preserve_head(doc: String): FfiPatchResult.ByValue

    /**
     * Reposition boundary marker to end of exchange component using an explicit ID,
     * preserving (HEAD) markers.
     */
    fun agent_doc_reposition_boundary_to_end_preserve_head_with_id(
        doc: String,
        boundary_id: String,
    ): FfiPatchResult.ByValue

    /**
     * Normalize/fail-close template structure before editor-visible IPC writes.
     */
    fun agent_doc_normalize_template_structure(doc: String): FfiPatchResult.ByValue

    /**
     * Apply node-keyed IPC patches through the shared Rust document model.
     */
    fun agent_doc_apply_node_patches(doc: String, node_patches_json: String): FfiPatchResult.ByValue

    /** Controller-backed `admin inspect --json` wrapper. */
    fun agent_doc_admin_inspect_json(
        project_root: String?,
        document_path: String?,
        session_id: String?,
        pane_id: String?,
    ): FfiJsonResult.ByValue

    /** Controller-backed `admin queue pause|resume|drain --json` wrapper. */
    fun agent_doc_admin_queue_control_json(
        project_root: String?,
        document_path: String?,
        action: String,
        observed_generation: Long,
        reason: String?,
        item_id: String?,
    ): FfiJsonResult.ByValue

    /** Controller-backed `admin reap --json` wrapper. */
    fun agent_doc_admin_reap_json(
        project_root: String?,
        document_path: String?,
        session_id: String?,
        pane_id: String?,
        observed_generation: Long,
        reason: String,
    ): FfiJsonResult.ByValue

    /** Controller-backed `admin handoff --json` wrapper. */
    fun agent_doc_admin_handoff_json(
        project_root: String?,
        document_path: String,
        to_pane: String,
        observed_generation: Long,
        reason: String,
    ): FfiJsonResult.ByValue

    /** Controller-backed `admin repair-projection --json` wrapper. */
    fun agent_doc_admin_repair_projection_json(
        project_root: String?,
        document_path: String?,
        projection: String,
        observed_generation: Long,
        reason: String?,
    ): FfiJsonResult.ByValue

    /**
     * Parse components from a document.
     * Returns JSON array of component objects.
     */
    fun agent_doc_parse_components(doc: String): FfiComponentList.ByValue

    /** Collect editor-facing visual token ranges as JSON. Caller must free result. */
    fun agent_doc_visual_tokens_json(doc: String): Pointer?

    /** Record a document change event for debounce tracking. */
    fun agent_doc_document_changed(file_path: String)

    /** Record a document change event plus the editor-visible buffer digest. */
    fun agent_doc_document_changed_digest(file_path: String, content_len: Long, content_hash: String)

    /**
     * Record a document change plus the editor's FULL visible buffer content (#pcp6).
     * Lets the CLI confirm the editor buffer equals on-disk content (no unsaved edit
     * ahead of disk). Text stays local to the project `.agent-doc/` state dir.
     */
    fun agent_doc_document_changed_digest_content(file_path: String, content: String)

    /** Non-blocking idle check. Returns true if no document_changed event within debounce_ms. */
    fun agent_doc_is_idle(file_path: String, debounce_ms: Long): Boolean

    /** Block until document is idle for debounce_ms, or timeout_ms expires. Returns true if idle. */
    fun agent_doc_await_idle(file_path: String, debounce_ms: Long, timeout_ms: Long): Boolean

    /** Check if the document has been tracked (at least one document_changed call). */
    fun agent_doc_is_tracked(file_path: String): Boolean

    /** Return the number of files tracked in the debounce state. */
    fun agent_doc_tracked_count(): Int

    /**
     * Explicit run-cancel reclaim (#cancel-orphans-preflight-cycle): abandon an
     * orphaned empty `preflight_started` cycle (no response capture) so the next
     * Run Agent Doc starts fresh immediately. Returns 1 if abandoned, 0 if
     * nothing reclaimed (no open cycle / protected), -1 on error. Fail-safe in
     * the binary: a cycle with real work is left intact.
     */
    fun agent_doc_cancel_preflight_cycle(file_path: String): Int

    /** Try to acquire the sync lock. Returns true if acquired. */
    fun agent_doc_sync_try_lock(): Boolean

    /** Release the sync lock. */
    fun agent_doc_sync_unlock()

    /** Bump sync debounce generation. Returns new generation number. */
    fun agent_doc_sync_bump_generation(): Long

    /** Check if generation is still current (no newer events). */
    fun agent_doc_sync_check_generation(gen: Long): Boolean

    /**
     * Start the IPC socket listener on a background thread.
     * The callback receives each JSON message (read-only, do NOT free) and returns
     * true if handled, false on error. The listener generates ack responses internally.
     */
    fun agent_doc_start_ipc_listener(project_root: String, callback: IpcMessageCallback): Boolean

    /**
     * V2 of [agent_doc_start_ipc_listener] with extended ack-result encoding.
     *
     * The callback returns one of:
     * - `0` → ack `{"type":"ack","status":"error"}` (apply failed)
     * - `1` → ack `{"type":"ack","status":"ok"}` (apply succeeded)
     * - `2` → ack `{"type":"ack","status":"error","reason":"already_applied"}`
     *
     * Plugins prefer v2 so the binary can recognise `already_applied` and skip
     * the file-IPC fallback that would otherwise stack a duplicate response
     * heading on top of the live buffer. Falls back to v1 on older binaries.
     *
     * Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
     * `#ipcpluginalready`.
     */
    fun agent_doc_start_ipc_listener_v2(project_root: String, callback: IpcMessageCallbackV2): Boolean

    /** Stop the IPC socket listener by removing the socket file. */
    fun agent_doc_stop_ipc_listener(project_root: String)

    /**
     * Write the final applied document content to the ack-content sidecar file.
     * Sidecar path: `<project_root>/.agent-doc/ack-content/<patch_id>.md`
     * Call this after applying a patch so the CLI binary can use it as snapshot
     * content without the 200ms sleep + re-read heuristic.
     *
     * @param project_root  path to the project root containing `.agent-doc/`
     * @param patch_id      UUID from the patch payload (identifies the sidecar file)
     * @param content       the final document content after all patches applied
     * @return true if written successfully, false on error
     */
    fun agent_doc_write_ack_content(project_root: String, patch_id: String, content: String): Boolean

    /**
     * Check if --force-disk claimed this patch by writing a sentinel file.
     * Checks `.agent-doc/claimed-patches/<patch_id>`. Sentinels are durable for
     * the patch id so repeated watcher passes all skip locally closed patches.
     *
     * @param project_root  path to the project root containing `.agent-doc/`
     * @param patch_id      UUID from the patch payload
     * @return true if sentinel exists (patch already applied by CLI disk write), false otherwise
     */
    fun agent_doc_is_claimed_by_force_disk(project_root: String, patch_id: String): Boolean

    /**
     * True when the current disk file matches committed HEAD and HEAD already
     * contains the incoming response patch content. Used to no-op stale editor
     * replays after a JetBrains File Cache Conflict accept.
     */
    fun agent_doc_patch_content_already_committed(file_path: String, content: String): Boolean

    /** Callback interface for socket IPC messages. */
    interface IpcMessageCallback : Callback {
        /** Called with each JSON message. Return true if handled, false on error. */
        fun invoke(message: Pointer): Boolean
    }

    /**
     * V2 callback interface for socket IPC messages with already-applied signal.
     * Return one of: 0=error, 1=ok, 2=already_applied. See
     * [agent_doc_start_ipc_listener_v2] for the wire-level ack mapping.
     */
    interface IpcMessageCallbackV2 : Callback {
        fun invoke(message: Pointer): Int
    }

    /**
     * Text-based CRDT 3-way merge. All three params are plain text (not CRDT state bytes).
     * Returns merged text (conflict-free); falls back to `ours` on error.
     * Caller must free result with [agent_doc_free_string].
     */
    fun agent_doc_merge_crdt(base: String, ours: String, theirs: String): Pointer?

    /** Get the library version (e.g. "0.26.1"). Caller must free result. */
    fun agent_doc_version(): Pointer?

    /**
     * Resolve a file's agent-doc project root and relative path.
     *
     * Walks up from [filePath]'s parent looking for the nearest ancestor directory
     * that contains `.agent-doc/`. Used by editor plugins to detect when a file
     * inside a submodule belongs to its own agent-doc project (with its own tmux
     * session / snapshots) rather than the enclosing workspace.
     *
     * Returns a struct whose `project_root` / `relative_path` pointers are null
     * when no ancestor contains `.agent-doc/`. Non-null pointers must be freed
     * with [agent_doc_free_string].
     */
    fun agent_doc_resolve_project_path(filePath: String): FfiProjectPath.ByValue

    /**
     * Commit the document at [filePath] to git.
     * Call after successfully applying a patch as a defense-in-depth guarantee
     * that the agent response is tracked even if the shell-side --commit was skipped.
     *
     * @param filePath  absolute path to the document file
     * @return true on success, false on failure
     */
    fun agent_doc_commit(filePath: String): Boolean

    /** Free a string returned by any agent_doc_* function. */
    fun agent_doc_free_string(ptr: Pointer?)

    companion object {
        @Volatile private var instance: AgentDocLib? = null
        @Volatile private var loadError: String? = null
        @Volatile private var loadedPath: String? = null
        @Volatile private var loadedMtime: Long = 0L
        @Volatile private var currentLockFile: File? = null
        private var shutdownHookRegistered = false

        fun get(): AgentDocLib? {
            val current = instance
            val path = loadedPath

            if (current != null && path != null) {
                if (libMtimeChanged(path, loadedMtime)) {
                    LOG.info("[native] libagent_doc mtime changed, reloading from $path")
                    return reload(path)
                }
                return current
            }

            if (loadError != null) return null

            val libPath = resolveLibPath() ?: run {
                loadError = "agent-doc binary not found; FFI unavailable"
                LOG.warn(loadError!!)
                return null
            }

            return loadFrom(libPath)
        }

        @Synchronized
        private fun reload(path: String): AgentDocLib? {
            val currentMtime = File(path).lastModified()
            if (currentMtime == loadedMtime || currentMtime == 0L) return instance
            return try {
                removePidLock()
                val newLib = Native.load(path, AgentDocLib::class.java)
                instance = newLib
                loadedMtime = currentMtime
                loadError = null
                writePidLock(path)
                verifyVersion(newLib, path)
                newLib
            } catch (e: Exception) {
                LOG.warn("[native] reload failed, keeping previous instance: ${e.message}")
                instance
            }
        }

        @Synchronized
        private fun loadFrom(path: String): AgentDocLib? {
            return try {
                val newLib = Native.load(path, AgentDocLib::class.java)
                instance = newLib
                loadedPath = path
                loadedMtime = File(path).lastModified()
                writePidLock(path)
                registerShutdownHook()
                verifyVersion(newLib, path)
                newLib
            } catch (e: Exception) {
                loadError = "Failed to load libagent_doc: ${e.message}"
                LOG.warn(loadError!!)
                null
            }
        }

        private fun verifyVersion(lib: AgentDocLib, path: String) {
            try {
                val ptr = lib.agent_doc_version()
                if (ptr != null) {
                    val version = ptr.getString(0)
                    lib.agent_doc_free_string(ptr)
                    LOG.info("[native] loaded libagent_doc v$version from $path")
                } else {
                    LOG.warn("[native] agent_doc_version() returned null — possible ABI mismatch at $path")
                }
            } catch (e: Exception) {
                LOG.warn("[native] agent_doc_version() failed — ABI mismatch at $path: ${e.message}")
            }
        }

        private fun writePidLock(libPath: String) {
            try {
                val resolved = File(libPath).canonicalFile
                val pid = ProcessHandle.current().pid()
                val lockFile = File("${resolved.absolutePath}.pid.$pid")
                lockFile.createNewFile()
                currentLockFile = lockFile
                LOG.debug("[native] wrote pid lock: ${lockFile.name}")
            } catch (e: Exception) {
                LOG.debug("[native] failed to write pid lock: ${e.message}")
            }
        }

        internal fun removePidLock() {
            val lock = currentLockFile ?: return
            try {
                if (lock.delete()) {
                    LOG.debug("[native] removed pid lock: ${lock.name}")
                }
            } catch (e: Exception) {
                LOG.debug("[native] failed to remove pid lock: ${e.message}")
            }
            currentLockFile = null
        }

        private fun registerShutdownHook() {
            if (shutdownHookRegistered) return
            shutdownHookRegistered = true
            Runtime.getRuntime().addShutdownHook(Thread {
                removePidLock()
            })
        }

        private fun resolveLibPath(): String? {
            try {
                val process = ProcessBuilder("agent-doc", "lib-path")
                    .redirectErrorStream(false)
                    .start()
                val path = process.inputStream.bufferedReader().readLine()?.trim()
                val exitCode = process.waitFor()
                if (exitCode == 0 && path != null && File(path).exists()) {
                    return path
                }
            } catch (e: Exception) {
                LOG.debug("[native] agent-doc lib-path failed: ${e.message}")
            }
            return null
        }

        private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(AgentDocLib::class.java)
    }
}

/**
 * Safe wrappers for controller-backed admin FFI calls.
 *
 * These return the same JSON payloads as the CLI `agent-doc admin ... --json`
 * commands. Editors parse/display the payload; ownership and generation checks
 * stay inside the Rust controller.
 */
object NativeAdminControls {
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(NativeAdminControls::class.java)
    private const val NO_GENERATION: Long = -1

    fun inspect(
        projectRoot: String? = null,
        documentPath: String? = null,
        sessionId: String? = null,
        paneId: String? = null,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = try {
            lib.agent_doc_admin_inspect_json(projectRoot, documentPath, sessionId, paneId)
        } catch (e: Throwable) {
            LOG.warn("[native] admin_inspect unavailable: ${e.message}")
            return null
        }
        return decodeJsonResult(lib, result, "admin_inspect")
    }

    fun queueControl(
        action: String,
        projectRoot: String? = null,
        documentPath: String? = null,
        observedGeneration: Long? = null,
        reason: String? = null,
        itemId: String? = null,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = try {
            lib.agent_doc_admin_queue_control_json(
                projectRoot,
                documentPath,
                action,
                observedGeneration ?: NO_GENERATION,
                reason,
                itemId,
            )
        } catch (e: Throwable) {
            LOG.warn("[native] admin_queue_control unavailable: ${e.message}")
            return null
        }
        return decodeJsonResult(lib, result, "admin_queue_control")
    }

    fun reap(
        observedGeneration: Long,
        reason: String,
        projectRoot: String? = null,
        documentPath: String? = null,
        sessionId: String? = null,
        paneId: String? = null,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = try {
            lib.agent_doc_admin_reap_json(
                projectRoot,
                documentPath,
                sessionId,
                paneId,
                observedGeneration,
                reason,
            )
        } catch (e: Throwable) {
            LOG.warn("[native] admin_reap unavailable: ${e.message}")
            return null
        }
        return decodeJsonResult(lib, result, "admin_reap")
    }

    fun handoff(
        documentPath: String,
        toPane: String,
        observedGeneration: Long,
        reason: String,
        projectRoot: String? = null,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = try {
            lib.agent_doc_admin_handoff_json(
                projectRoot,
                documentPath,
                toPane,
                observedGeneration,
                reason,
            )
        } catch (e: Throwable) {
            LOG.warn("[native] admin_handoff unavailable: ${e.message}")
            return null
        }
        return decodeJsonResult(lib, result, "admin_handoff")
    }

    fun repairProjection(
        projection: String = "all",
        projectRoot: String? = null,
        documentPath: String? = null,
        observedGeneration: Long? = null,
        reason: String? = null,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = try {
            lib.agent_doc_admin_repair_projection_json(
                projectRoot,
                documentPath,
                projection,
                observedGeneration ?: NO_GENERATION,
                reason,
            )
        } catch (e: Throwable) {
            LOG.warn("[native] admin_repair_projection unavailable: ${e.message}")
            return null
        }
        return decodeJsonResult(lib, result, "admin_repair_projection")
    }

    private fun decodeJsonResult(
        lib: AgentDocLib,
        result: AgentDocLib.FfiJsonResult.ByValue,
        label: String,
    ): String? {
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] $label error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.json == null) return null
            return result.json!!.getString(0)
        } finally {
            lib.agent_doc_free_string(result.json)
        }
    }
}

/**
 * Safe wrapper around FFI calls with automatic memory management.
 */
object NativePatching {
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(NativePatching::class.java)
    @Volatile private var nodePatchesAvailable: Boolean? = null

    data class VisualToken(
        val kind: String,
        val start: Int,
        val end: Int,
    )

    fun isAvailable(): Boolean = AgentDocLib.get() != null

    fun canApplyNodePatches(): Boolean {
        nodePatchesAvailable?.let { return it }
        val lib = AgentDocLib.get() ?: return false
        return try {
            val result = lib.agent_doc_apply_node_patches("", "[]")
            try {
                if (result.error != null) {
                    val error = result.error!!.getString(0)
                    LOG.debug("[native] apply_node_patches probe rejected: $error")
                    false
                } else {
                    result.text != null
                }
            } finally {
                lib.agent_doc_free_string(result.text)
                lib.agent_doc_free_string(result.error)
            }
        } catch (e: UnsatisfiedLinkError) {
            LOG.debug("[native] apply_node_patches unavailable: ${e.message}")
            false
        } catch (e: Throwable) {
            LOG.debug("[native] apply_node_patches probe failed: ${e.message}")
            false
        }.also { nodePatchesAvailable = it }
    }

    fun patchContentAlreadyCommitted(filePath: String, content: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_patch_content_already_committed(filePath, content)
        } catch (e: Throwable) {
            LOG.debug("[native] patch_content_already_committed unavailable: ${e.message}")
            false
        }
    }

    /**
     * Apply node-keyed IPC patches using the native library.
     * Returns the patched document, or null if FFI is unavailable/errors.
     */
    fun applyNodePatches(doc: String, nodePatchesJson: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = try {
            lib.agent_doc_apply_node_patches(doc, nodePatchesJson)
        } catch (e: Throwable) {
            LOG.warn("[native] apply_node_patches unavailable: ${e.message}")
            return null
        }
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] apply_node_patches error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            return result.text!!.getString(0)
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Apply a component patch using the native library.
     * Returns the patched document, or null if FFI is unavailable/errors.
     */
    fun applyComponentPatch(doc: String, component: String, content: String, mode: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_apply_patch(doc, component, content, mode)
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] apply_patch error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            val text = result.text!!.getString(0)
            return text
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Apply a component patch with cursor-aware ordering.
     * When mode is "append" and caretOffset >= 0, inserts before the caret.
     * Returns the patched document, or null if FFI is unavailable/errors.
     */
    fun applyComponentPatchWithCaret(doc: String, component: String, content: String, mode: String, caretOffset: Int): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_apply_patch_with_caret(doc, component, content, mode, caretOffset)
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] apply_patch_with_caret error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            val text = result.text!!.getString(0)
            return text
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Apply a component patch using a boundary marker for insertion point.
     * Returns the patched document, or null if FFI is unavailable/errors.
     */
    fun applyComponentPatchWithBoundary(doc: String, component: String, content: String, mode: String, boundaryId: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_apply_patch_with_boundary(doc, component, content, mode, boundaryId)
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] apply_patch_with_boundary error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            val text = result.text!!.getString(0)
            return text
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Reposition boundary marker to end of exchange component via FFI.
     * Removes all stale boundaries, inserts a single fresh 8-char one.
     * Returns the cleaned document, or null if FFI is unavailable/errors.
     */
    fun repositionBoundaryToEnd(doc: String, boundaryId: String? = null): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = if (boundaryId.isNullOrBlank()) {
            lib.agent_doc_reposition_boundary_to_end(doc)
        } else {
            lib.agent_doc_reposition_boundary_to_end_with_id(doc, boundaryId)
        }
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] reposition_boundary error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            val text = result.text!!.getString(0)
            return text
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Reposition boundary marker to end of exchange component via FFI,
     * preserving transient (HEAD) markers on Re: headings.
     * Returns the repositioned document, or null if FFI is unavailable/errors.
     */
    fun repositionBoundaryToEndPreserveHead(doc: String, boundaryId: String? = null): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = if (boundaryId.isNullOrBlank()) {
            lib.agent_doc_reposition_boundary_to_end_preserve_head(doc)
        } else {
            lib.agent_doc_reposition_boundary_to_end_preserve_head_with_id(doc, boundaryId)
        }
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] reposition_boundary_preserve_head error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            val text = result.text!!.getString(0)
            return text
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Normalize/fail-close template structure using the shared Rust guard.
     * Returns null when FFI is unavailable or when the guard rejects the document.
     */
    fun normalizeTemplateStructure(doc: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_normalize_template_structure(doc)
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] normalize_template_structure error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            return result.text!!.getString(0)
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Non-blocking idle check via FFI debounce tracker.
     * Returns true if the user hasn't typed within debounceMs, or if FFI is unavailable.
     */
    fun isIdle(filePath: String, debounceMs: Long): Boolean {
        val lib = AgentDocLib.get() ?: return true // No FFI — assume idle (don't block)
        return lib.agent_doc_is_idle(filePath, debounceMs)
    }

    /**
     * CRDT 3-way merge from plain text.
     * Returns the conflict-free merged text, or null if FFI is unavailable.
     * Never returns null on merge conflict — CRDT is always conflict-free.
     */
    fun mergeCrdt(base: String, ours: String, theirs: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val ptr = lib.agent_doc_merge_crdt(base, ours, theirs)
        try {
            return ptr?.getString(0)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /**
     * Resolve the nearest agent-doc project root for an absolute file path.
     *
     * Returns `(projectRoot, relativePath)` where `projectRoot` is the nearest
     * ancestor directory containing `.agent-doc/`, and `relativePath` is
     * [absPath] relative to that root. Returns `null` when no ancestor has
     * `.agent-doc/` or FFI is unavailable — callers must fall back to the
     * workspace `basePath`.
     */
    fun resolveProjectPath(absPath: String): Pair<String, String>? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_resolve_project_path(absPath)
        try {
            val rootPtr = result.project_root ?: return null
            val relPtr = result.relative_path ?: return null
            return Pair(rootPtr.getString(0), relPtr.getString(0))
        } finally {
            lib.agent_doc_free_string(result.project_root)
            lib.agent_doc_free_string(result.relative_path)
        }
    }

    /**
     * Collect visual token ranges for agent-doc-specific markdown structures.
     */
    fun visualTokens(doc: String): List<VisualToken> {
        val lib = AgentDocLib.get() ?: return emptyList()
        val ptr = lib.agent_doc_visual_tokens_json(doc) ?: return emptyList()
        try {
            val raw = ptr.getString(0)
            val root = com.google.gson.JsonParser.parseString(raw).asJsonArray
            return root.mapNotNull { element ->
                val obj = element.asJsonObject
                val kind = obj.get("kind")?.asString ?: return@mapNotNull null
                val start = obj.get("start")?.asInt ?: return@mapNotNull null
                val end = obj.get("end")?.asInt ?: return@mapNotNull null
                VisualToken(kind, start, end)
            }
        } catch (e: Exception) {
            LOG.warn("[native] visual_tokens_json error: ${e.message}")
            return emptyList()
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /**
     * Merge frontmatter fields using the native library.
     * Returns the updated document, or null if FFI is unavailable/errors.
     */
    fun mergeFrontmatter(doc: String, yamlFields: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_merge_frontmatter(doc, yamlFields)
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] merge_frontmatter error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            val text = result.text!!.getString(0)
            return text
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }

    /**
     * Converge the `agent:queue` opening-tag `auto` attribute to [wantAuto] using
     * the native library. Returns the updated document (unchanged if there is no
     * queue component or the tag already matches), or null if FFI is
     * unavailable/errors. See #adoc-queue-ipc-buffer-divergence.
     */
    fun convergeQueueAuto(doc: String, wantAuto: Boolean): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_converge_queue_auto(doc, if (wantAuto) 1 else 0)
        try {
            if (result.error != null) {
                val error = result.error!!.getString(0)
                LOG.warn("[native] converge_queue_auto error: $error")
                lib.agent_doc_free_string(result.error)
                return null
            }
            if (result.text == null) return null
            return result.text!!.getString(0)
        } finally {
            lib.agent_doc_free_string(result.text)
        }
    }
}
