package com.github.btakita.agentdoc

import com.sun.jna.Callback
import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.Structure
import java.io.File

/**
 * JNA bindings to libagent_doc shared library.
 *
 * Replaces duplicated Kotlin logic for component patching, frontmatter merge,
 * and code block detection with FFI calls to the canonical Rust implementation.
 */
interface AgentDocLib : Library {

    /** Result of [agent_doc_apply_patch]. */
    @Structure.FieldOrder("text", "error")
    class FfiPatchResult : Structure() {
        @JvmField var text: Pointer? = null
        @JvmField var error: Pointer? = null
    }

    /** Result of [agent_doc_parse_components]. */
    @Structure.FieldOrder("json", "count")
    class FfiComponentList : Structure() {
        @JvmField var json: Pointer? = null
        @JvmField var count: Long = 0
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
    ): FfiPatchResult

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
    ): FfiPatchResult

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
    ): FfiPatchResult

    /**
     * Merge YAML key/value pairs into a document's frontmatter.
     */
    fun agent_doc_merge_frontmatter(
        doc: String,
        yaml_fields: String,
    ): FfiPatchResult

    /**
     * Reposition boundary marker to end of exchange component.
     * Removes all stale boundaries and inserts a single fresh 8-char one.
     */
    fun agent_doc_reposition_boundary_to_end(doc: String): FfiPatchResult

    /**
     * Parse components from a document.
     * Returns JSON array of component objects.
     */
    fun agent_doc_parse_components(doc: String): FfiComponentList

    /** Record a document change event for debounce tracking. */
    fun agent_doc_document_changed(file_path: String)

    /** Non-blocking idle check. Returns true if no document_changed event within debounce_ms. */
    fun agent_doc_is_idle(file_path: String, debounce_ms: Long): Boolean

    /** Block until document is idle for debounce_ms, or timeout_ms expires. Returns true if idle. */
    fun agent_doc_await_idle(file_path: String, debounce_ms: Long, timeout_ms: Long): Boolean

    /** Check if the document has been tracked (at least one document_changed call). */
    fun agent_doc_is_tracked(file_path: String): Boolean

    /** Return the number of files tracked in the debounce state. */
    fun agent_doc_tracked_count(): Int

    /** Set response status for a file. Values: "generating", "writing", "routing", "idle". */
    fun agent_doc_set_status(file_path: String, status: String)

    /** Get response status for a file. Returns "generating", "writing", "routing", or "idle". Caller must free result. */
    fun agent_doc_get_status(file_path: String): Pointer?

    /** Check if any operation is in progress. Returns true if status != "idle". */
    fun agent_doc_is_busy(file_path: String): Boolean

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
     * Checks `.agent-doc/claimed-patches/<patch_id>`. Deletes sentinel if found (one-time use).
     *
     * @param project_root  path to the project root containing `.agent-doc/`
     * @param patch_id      UUID from the patch payload
     * @return true if sentinel exists (patch already applied by CLI disk write), false otherwise
     */
    fun agent_doc_is_claimed_by_force_disk(project_root: String, patch_id: String): Boolean

    /** Callback interface for socket IPC messages. */
    interface IpcMessageCallback : Callback {
        /** Called with each JSON message. Return true if handled, false on error. */
        fun invoke(message: Pointer): Boolean
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
        private var instance: AgentDocLib? = null
        private var loadError: String? = null

        /**
         * Get the loaded library instance, or null if unavailable.
         * Logs the error once on first failure.
         */
        fun get(): AgentDocLib? {
            if (instance != null) return instance
            if (loadError != null) return null

            try {
                val libPath = resolveLibPath()
                if (libPath == null) {
                    loadError = "agent-doc binary not found; FFI unavailable"
                    LOG.warn(loadError!!)
                    return null
                }
                instance = Native.load(libPath, AgentDocLib::class.java)
                LOG.info("[native] loaded libagent_doc from $libPath (is_tracked + await_idle available)")
            } catch (e: Exception) {
                loadError = "Failed to load libagent_doc: ${e.message}"
                LOG.warn(loadError!!)
            }
            return instance
        }

        /**
         * Resolve the path to libagent_doc shared library.
         * Strategy: run `agent-doc lib-path` to get the canonical location.
         */
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
 * Safe wrapper around FFI calls with automatic memory management.
 */
object NativePatching {
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(NativePatching::class.java)

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
    fun repositionBoundaryToEnd(doc: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result = lib.agent_doc_reposition_boundary_to_end(doc)
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
}
