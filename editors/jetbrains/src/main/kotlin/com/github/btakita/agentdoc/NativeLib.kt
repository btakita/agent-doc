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
     * Parse components from a document.
     * Returns JSON array of component objects.
     */
    fun agent_doc_parse_components(doc: String): FfiComponentList.ByValue

    /** Collect editor-facing visual token ranges as JSON. Caller must free result. */
    fun agent_doc_visual_tokens_json(doc: String): Pointer?

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
     * Checks `.agent-doc/claimed-patches/<patch_id>`. Sentinels are durable for
     * the patch id so repeated watcher passes all skip locally closed patches.
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
 * Safe wrapper around FFI calls with automatic memory management.
 */
object NativePatching {
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(NativePatching::class.java)

    data class VisualToken(
        val kind: String,
        val start: Int,
        val end: Int,
    )

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
}
