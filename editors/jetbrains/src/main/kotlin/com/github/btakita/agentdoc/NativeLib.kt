package com.github.btakita.agentdoc

import com.sun.jna.Callback
import com.sun.jna.Library
import com.sun.jna.Pointer
import com.sun.jna.Structure
import java.io.File
import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Proxy
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.concurrent.CancellationException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

internal fun libMtimeChanged(path: String, storedMtime: Long): Boolean {
    val currentMtime = File(path).lastModified()
    return currentMtime != storedMtime && currentMtime != 0L
}

internal enum class NativeReloadTransition {
    KeepCurrent,
    PublishReplacement,
    RetainOldGeneration,
}

internal enum class NativeRetiredGenerationTransition {
    LoadReplacement,
    LoadReplacementRetainingInertMapping,
}

internal fun nativeRetiredGenerationTransition(
    oldGenerationUnmapped: Boolean
): NativeRetiredGenerationTransition =
    if (oldGenerationUnmapped) {
        NativeRetiredGenerationTransition.LoadReplacement
    } else {
        NativeRetiredGenerationTransition.LoadReplacementRetainingInertMapping
    }

internal fun nativeReloadTransition(
    loadedMtime: Long,
    targetMtime: Long,
    nativeQuiesced: Boolean,
    callsDrained: Boolean,
    replacementReady: Boolean = true,
): NativeReloadTransition =
    when {
        targetMtime == 0L || targetMtime == loadedMtime -> NativeReloadTransition.KeepCurrent
        !nativeQuiesced || !callsDrained || !replacementReady ->
            NativeReloadTransition.RetainOldGeneration
        else -> NativeReloadTransition.PublishReplacement
    }

internal enum class NativeCallLane {
    GenerationLifecycle,
    IsolatedCaller,
}

/**
 * Ordinary native work stays on the caller's already-scoped lane. A slow document, projection,
 * controller, or editor-surface call may delay that scope, but cannot poison the process-wide
 * native generation and disable unrelated reactive ingress such as editor-surface/tmux
 * synchronization.
 *
 * Only explicit generation lifecycle transitions use the bounded poisonable lease. Those
 * transitions are the operations that can actually prove a generation is no longer safe to retain.
 */
internal fun nativeCallLaneUtil(methodName: String): NativeCallLane =
    when (methodName) {
        "agent_doc_quiesce_for_reload",
        "agent_doc_resume_after_reload_failure",
        "agent_doc_start_ipc_listener",
        "agent_doc_start_ipc_listener_v2",
        "agent_doc_stop_ipc_listener" -> NativeCallLane.GenerationLifecycle
        else -> NativeCallLane.IsolatedCaller
    }

internal class NativeCallQueueTimeoutException(message: String) : IllegalStateException(message)

private enum class NativeCallExecutionState {
    Queued,
    Running,
    Finished,
    Cancelled,
}

/**
 * Run one call on a generation-owned worker without confusing queue delay with a wedged native
 * invocation. A queued timeout drops only that invocation; a call which actually started and
 * exceeded its lease invokes [onRunningTimeout], which poisons the generation.
 */
internal fun <T> callOnNativeWorker(
    executor: ExecutorService,
    workerThreads: Set<Thread>,
    timeoutMs: Long,
    onRunningTimeout: () -> Nothing,
    call: () -> T,
): T {
    if (workerThreads.contains(Thread.currentThread())) return call()
    val state = AtomicReference(NativeCallExecutionState.Queued)
    val future =
        executor.submit<T> {
            if (
                !state.compareAndSet(
                    NativeCallExecutionState.Queued,
                    NativeCallExecutionState.Running,
                )
            ) {
                throw CancellationException("native call cancelled before execution")
            }
            try {
                call()
            } finally {
                state.set(NativeCallExecutionState.Finished)
            }
        }
    return try {
        future.get(timeoutMs, TimeUnit.MILLISECONDS)
    } catch (error: ExecutionException) {
        throw error.cause ?: error
    } catch (_: TimeoutException) {
        if (
            state.compareAndSet(NativeCallExecutionState.Queued, NativeCallExecutionState.Cancelled)
        ) {
            future.cancel(false)
            throw NativeCallQueueTimeoutException(
                "native call waited ${timeoutMs}ms behind other operations; generation retained"
            )
        }
        if (state.get() == NativeCallExecutionState.Finished) {
            try {
                future.get()
            } catch (error: ExecutionException) {
                throw error.cause ?: error
            }
        } else {
            future.cancel(true)
            onRunningTimeout()
        }
    } catch (error: InterruptedException) {
        future.cancel(true)
        Thread.currentThread().interrupt()
        throw IllegalStateException("interrupted while waiting for the native generation", error)
    }
}

internal fun newNativeGenerationExecutor(
    workerThreads: MutableSet<Thread>,
    workerCount: Int,
): ExecutorService {
    val threadSequence = AtomicInteger(0)
    return Executors.newFixedThreadPool(workerCount) { runnable ->
        Thread(
                runnable,
                "agent-doc-native-generation-${threadSequence.incrementAndGet()}",
            )
            .also { thread ->
                thread.isDaemon = true
                workerThreads.add(thread)
            }
    }
}

internal sealed interface NativeReloadOutcome {
    data object AlreadyCurrent : NativeReloadOutcome

    data class Reloaded(val mtime: Long) : NativeReloadOutcome

    data class RetainedOld(val reason: String) : NativeReloadOutcome

    data class RestartRequired(val reason: String) : NativeReloadOutcome
}

/**
 * Copy the freshly-installed shared library to a unique, per-mtime path so `Native.load` (and the
 * underlying `dlopen`) actually maps the NEW native code.
 *
 * Loading the canonical install path in place does NOT pick up a new build: `dlopen` returns the
 * already-mapped handle for an unchanged path, so the live JVM keeps running stale native code
 * after `make install` / `agent-doc lib-install` until a full IDE restart. Copying to
 * `libagent_doc-<mtime>.<ext>` under [cacheRoot] gives each install a distinct inode, forcing a
 * real load and enabling hot-reload without restarting the IDE. Stale shadow copies are pruned only
 * after their native generation is closed; deleting a still-mapped file would leave a `(deleted)`
 * mapping and make generation ownership impossible to prove. Returns the shadow path, or null on
 * failure so the caller can fall back to the canonical path.
 */
internal fun nativeShadowCopyPath(canonicalPath: String, mtime: Long, cacheRoot: File): String? {
    return try {
        val src = File(canonicalPath)
        val ext = src.name.substringAfterLast('.', "so")
        cacheRoot.mkdirs()
        val dest = File(cacheRoot, "libagent_doc-$mtime.$ext")
        if (!dest.exists() || dest.length() != src.length()) {
            Files.copy(src.toPath(), dest.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
        dest.absolutePath
    } catch (e: Exception) {
        null
    }
}

internal fun nativePathIsMapped(path: String, procMaps: String): Boolean {
    val absolutePath = File(path).absolutePath
    return procMaps.lineSequence().any { line ->
        line.endsWith(" $absolutePath") || line.endsWith(" $absolutePath (deleted)")
    }
}

/**
 * JNA bindings to libagent_doc shared library.
 *
 * Replaces duplicated Kotlin logic for component patching, frontmatter merge, and code block
 * detection with FFI calls to the canonical Rust implementation.
 */
interface AgentDocLib : Library {

    /**
     * Result of [agent_doc_apply_patch].
     *
     * Rust returns this struct by value (`#[repr(C)]`). The binding therefore uses [ByValue] so JNA
     * reads the struct's fields directly from the return registers (SysV ABI) instead of
     * dereferencing them as a pointer. See editors/jetbrains/docs/jna-by-value.md (or VERSIONS.md
     * 0.2.59).
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

    /** Apply a patch to a document component. Mode: "replace", "append", or "prepend". */
    fun agent_doc_apply_patch(
        doc: String,
        component_name: String,
        content: String,
        mode: String,
    ): FfiPatchResult.ByValue

    /**
     * Apply a patch with cursor-aware ordering for append mode. When mode is "append" and
     * caretOffset >= 0, inserts content before the caret. Pass caretOffset = -1 for normal
     * behavior.
     */
    fun agent_doc_apply_patch_with_caret(
        doc: String,
        component_name: String,
        content: String,
        mode: String,
        caret_offset: Int,
    ): FfiPatchResult.ByValue

    /**
     * Apply a patch using a boundary marker for insertion point. When mode is "append" and
     * boundary_id is found in the component, inserts content at the boundary marker position.
     */
    fun agent_doc_apply_patch_with_boundary(
        doc: String,
        component_name: String,
        content: String,
        mode: String,
        boundary_id: String,
    ): FfiPatchResult.ByValue

    /** Merge YAML key/value pairs into a document's frontmatter. */
    fun agent_doc_merge_frontmatter(
        doc: String,
        yaml_fields: String,
    ): FfiPatchResult.ByValue

    /**
     * Converge the `agent:queue` opening-tag `auto` attribute. `want_auto` is a C int (nonzero =
     * ensure `auto`, zero = strip `auto`); a content patch cannot change an opening-tag attribute,
     * so this is the convergence seam for #adoc-queue-ipc-buffer-divergence.
     */
    fun agent_doc_converge_queue_auto(
        doc: String,
        want_auto: Int,
    ): FfiPatchResult.ByValue

    /**
     * Reposition boundary marker to end of exchange component. Removes all stale boundaries and
     * inserts a single fresh 8-char one. Strips transient (HEAD) markers.
     */
    fun agent_doc_reposition_boundary_to_end(doc: String): FfiPatchResult.ByValue

    /**
     * Reposition boundary marker to end of exchange component using an explicit ID. Strips
     * transient (HEAD) markers.
     */
    fun agent_doc_reposition_boundary_to_end_with_id(
        doc: String,
        boundary_id: String,
    ): FfiPatchResult.ByValue

    /** Reposition boundary marker to end of exchange component, preserving (HEAD) markers. */
    fun agent_doc_reposition_boundary_to_end_preserve_head(doc: String): FfiPatchResult.ByValue

    /**
     * Reposition boundary marker to end of exchange component using an explicit ID, preserving
     * (HEAD) markers.
     */
    fun agent_doc_reposition_boundary_to_end_preserve_head_with_id(
        doc: String,
        boundary_id: String,
    ): FfiPatchResult.ByValue

    /** Normalize/fail-close template structure before editor-visible IPC writes. */
    fun agent_doc_normalize_template_structure(doc: String): FfiPatchResult.ByValue

    /** Apply node-keyed IPC patches through the shared Rust document model. */
    fun agent_doc_apply_node_patches(doc: String, node_patches_json: String): FfiPatchResult.ByValue

    /** Controller-backed `admin inspect --json` wrapper. */
    fun agent_doc_admin_inspect_json(
        project_root: String?,
        document_path: String?,
        session_id: String?,
        pane_id: String?,
    ): FfiJsonResult.ByValue

    /** Project Controller-owned tmux focus projection. */
    fun agent_doc_tmux_focus_state_json(project_root: String?): FfiJsonResult.ByValue

    /** Project Controller-owned document pane focus. */
    fun agent_doc_focus_document_pane_json(
        project_root: String?,
        document_path: String,
    ): FfiJsonResult.ByValue

    /** Project Controller-owned tmux layout sync. */
    fun agent_doc_sync_tmux_layout_json(
        project_root: String?,
        columns_json: String,
        window: String?,
        focus: String?,
        no_autostart: Int,
        exact_visible: Int,
    ): FfiJsonResult.ByValue

    /**
     * Report what the editor looks like now and get the derived tmux intent back
     * (`#jbsurfaceswap`).
     *
     * `surface_json` is an `EditorSurface`: `{ "focused", "visible", "columns", "force_reconcile"
     * }`. The receipt is `{ "intent", "idle", "outcome", "error" }`. This replaces choosing between
     * [agent_doc_focus_document_pane_json] and [agent_doc_sync_tmux_layout_json] in the plugin.
     */
    fun agent_doc_editor_surface_observe_json(
        project_root: String,
        surface_json: String,
    ): FfiJsonResult.ByValue

    /** Validate and enqueue an observation without waiting for tmux work. */
    fun agent_doc_editor_surface_enqueue_json(
        project_root: String,
        surface_json: String,
    ): Int

    /** Latest native/controller authority Source for one open document. */
    fun agent_doc_document_authority_json(
        project_root: String,
        document_path: String,
    ): FfiJsonResult.ByValue

    /** Selected editor document joined with its authority by the native Computed. */
    fun agent_doc_current_document_authority_json(project_root: String): FfiJsonResult.ByValue

    /**
     * Forget a project root's editor-surface graph — the editor closed the project. Returns `1`
     * when a surface was forgotten, `0` when none was registered, `-1` on a bad argument.
     */
    fun agent_doc_editor_surface_forget(project_root: String): Int

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

    /** Parse components from a document. Returns JSON array of component objects. */
    fun agent_doc_parse_components(doc: String): FfiComponentList.ByValue

    /** Collect editor-facing visual token ranges as JSON. Caller must free result. */
    fun agent_doc_visual_tokens_json(doc: String): Pointer?

    /**
     * #falsetyping-guard: per-editor full-content report carrying replica-churn provenance.
     * `no_unsaved_operator_edits` is 1 when the buffer holds no unsaved local operator edits ahead
     * of disk (any divergence is a `remoteCrdtApply`), letting the CLI re-merge on replica churn
     * instead of failing the visible-write guard closed. 0 keeps operator text authoritative.
     */
    fun agent_doc_lazily_current_observed_v1(
        file_path: String,
        content: String,
        editor_id: String,
        editor_kind: String,
        editor_version: String,
        capabilities_csv: String,
        no_unsaved_operator_edits: Int,
    )

    /** Publish this editor instance's reliable-sync close for a document. */
    fun agent_doc_document_closed_for_editor(file_path: String, editor_id: String)

    /**
     * Explicit run-cancel reclaim (#cancel-orphans-preflight-cycle): abandon an orphaned empty
     * `preflight_started` cycle (no response capture) so the next Run Agent Doc starts fresh
     * immediately. Returns 1 if abandoned, 0 if nothing reclaimed (no open cycle / protected), -1
     * on error. Fail-safe in the binary: a cycle with real work is left intact.
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
     * Start the IPC socket listener on a background thread. The callback receives each JSON message
     * (read-only, do NOT free) and returns true if handled, false on error. The listener generates
     * receipt responses internally.
     */
    fun agent_doc_start_ipc_listener(project_root: String, callback: IpcMessageCallback): Boolean

    /**
     * V2 of [agent_doc_start_ipc_listener] with extended receipt-result encoding.
     *
     * The callback returns one of:
     * - `0` → receipt `{"type":"receipt","status":"rejected"}` (apply failed)
     * - `1` → receipt `{"type":"receipt","status":"applied"}` (apply succeeded)
     * - `2` → receipt `{"type":"receipt","status":"applied","reason":"already_applied"}`
     *
     * Plugins prefer v2 so the binary can recognise `already_applied` and skip the file-IPC
     * fallback that would otherwise stack a duplicate response heading on top of the live buffer.
     * Falls back to v1 on older binaries.
     *
     * Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md `#ipcpluginalready`.
     */
    fun agent_doc_start_ipc_listener_v2(
        project_root: String,
        callback: IpcMessageCallbackV2,
    ): Boolean

    /** Stop the IPC socket listener by removing the socket file. */
    fun agent_doc_stop_ipc_listener(project_root: String): Int

    /**
     * Stop and join every native listener/connection thread, then drop any remaining cdylib-hosted
     * replicas. Returns 1 only at a safe unload point.
     */
    fun agent_doc_quiesce_for_reload(timeout_ms: Long): Int

    /** Re-enable a quiesced generation when loading its replacement failed. */
    fun agent_doc_resume_after_reload_failure()

    /**
     * Capability-bearing editor content endpoint for a successfully applied patch. It requires
     * lazily receipt support and writes the derived content projection plus the matching Lazily
     * delivery receipt in one ABI call.
     */
    fun agent_doc_editor_content_applied_for_editor_v1(
        project_root: String,
        patch_id: String,
        file_path: String,
        content: String,
        editor_id: String,
        editor_kind: String,
        editor_version: String,
        capabilities_csv: String,
    ): Boolean

    /** Record one editor-surface outcome through the shared Rust ops-log schema. */
    fun agent_doc_record_editor_surface_event(
        project_root: String,
        source: String,
        file_path: String,
        surface: String,
        action: String,
        agent_command: String,
        patch_id: String?,
        status: String,
    ): Boolean

    /**
     * True when the current disk file matches committed HEAD and HEAD already contains the incoming
     * response patch content. Used to no-op stale editor replays after a JetBrains File Cache
     * Conflict accept.
     */
    fun agent_doc_patch_content_already_committed(file_path: String, content: String): Boolean

    /** Callback interface for socket IPC messages. */
    interface IpcMessageCallback : Callback {
        /** Called with each JSON message. Return true if handled, false on error. */
        fun invoke(message: Pointer): Boolean
    }

    /**
     * V2 callback interface for socket IPC messages with already-applied signal. Return one of:
     * 0=error, 1=ok, 2=already_applied. See [agent_doc_start_ipc_listener_v2] for the wire-level
     * receipt mapping.
     */
    interface IpcMessageCallbackV2 : Callback {
        fun invoke(message: Pointer): Int
    }

    /**
     * Text-based CRDT 3-way merge. All three params are plain text (not CRDT state bytes). Returns
     * merged text (conflict-free); falls back to `ours` on error. Caller must free result with
     * [agent_doc_free_string].
     */
    fun agent_doc_merge_crdt(base: String, ours: String, theirs: String): Pointer?

    /** Get the library version (e.g. "0.26.1"). Caller must free result. */
    fun agent_doc_version(): Pointer?

    /**
     * Read the Rust-owned document state projection JSON for a document hash. Caller must free the
     * returned pointer with [agent_doc_free_string].
     */
    fun agent_doc_state_projection(documentHash: String): Pointer?

    /**
     * Recover a durable deferred write into a re-registering editor buffer. Caller must free a
     * non-null result with [agent_doc_free_string].
     */
    fun agent_doc_deferred_write_reconnect_content(
        filePath: String,
        editorContent: String,
    ): Pointer?

    fun agent_doc_deferred_write_post_register_content(
        filePath: String,
        editorContent: String,
    ): Pointer?

    fun agent_doc_deferred_write_post_register_project(filePath: String, editorContent: String): Int

    fun agent_doc_deferred_write_reconnect_propagated(filePath: String, editorContent: String): Int

    /**
     * Read the Project Controller→plugin turn-state projection JSON for a document path:
     * `{"state":"idle|awaiting_response|persisting","turn_in_flight":bool,"transition_authority":"project_controller","realtime_steering":{...}}`.
     * The plugin observes this to render turn-in-flight UI and to decide whether a forwarded
     * operator prompt starts a fresh turn or would collide with an in-flight response (the
     * double-append guard). Defaults to the idle projection when no cycle is open.
     * Shared-Foundation parity with the VS Code frontend (`specs/14-realtime-workflow.md` § Editor
     * Parity Requirement). Caller must free the returned pointer with [agent_doc_free_string].
     */
    fun agent_doc_turn_projection(filePath: String): Pointer?

    /**
     * Borrowed static capability token for lossless-tree CRDT frame exchange. Do not free the
     * returned pointer.
     */
    fun agent_doc_lossless_tree_capability(): Pointer?

    /**
     * Project document text into a durable lossless-tree JSON projection. Caller must free the
     * returned pointer with [agent_doc_free_string].
     */
    fun agent_doc_lossless_tree_project(docText: String): Pointer?

    /**
     * Render a durable lossless-tree JSON projection back to document text. Caller must free the
     * returned pointer with [agent_doc_free_string].
     */
    fun agent_doc_lossless_tree_render(projectionJson: String): Pointer?

    /**
     * Returns 1 when [projectionJson] still describes [visibleText], 0 when stale or unvalidatable.
     */
    fun agent_doc_lossless_tree_projection_current(projectionJson: String, visibleText: String): Int

    /**
     * Subscribe to lazily-spec snapshot/delta messages for a document (`#lazilystatesync2` /
     * `#lazilystatesync3`).
     *
     * - `lastEpoch == 0` → cold read returning a full `{"type":"snapshot",...}` graph image.
     * - `0 < lastEpoch < current` → warm `{"type":"delta",...}` the caller applies verbatim to
     *   converge.
     * - `lastEpoch >= current` → no-op delta (caller is current).
     *
     * Returns a NUL-terminated JSON string (or `"null"` on failure). Caller must free the returned
     * pointer with [agent_doc_free_string].
     */
    fun agent_doc_state_subscribe(documentHash: String, lastEpoch: Long): Pointer?

    /** Record a typed state-backbone event. Returns 1 on success, 0 on any error. */
    fun agent_doc_record_state_event(documentHash: String, eventJson: String): Int

    /** Record a lazily transport receipt for a successfully applied editor patch. */
    fun agent_doc_editor_patch_applied(
        filePath: String,
        patchId: String,
        actorGeneration: Long,
    ): Int

    /** Record a lazily transport receipt for a rejected editor patch. */
    fun agent_doc_editor_patch_rejected(
        filePath: String,
        patchId: String,
        actorGeneration: Long,
        reason: String,
    ): Int

    /**
     * Resolve a file's agent-doc project root and relative path.
     *
     * Walks up from [filePath]'s parent looking for the nearest ancestor directory that contains
     * `.agent-doc/`. Used by editor plugins to detect when a file inside a submodule belongs to its
     * own agent-doc project (with its own tmux session / snapshots) rather than the enclosing
     * workspace.
     *
     * Returns a struct whose `project_root` / `relative_path` pointers are null when no ancestor
     * contains `.agent-doc/`. Non-null pointers must be freed with [agent_doc_free_string].
     */
    fun agent_doc_resolve_project_path(filePath: String): FfiProjectPath.ByValue

    /**
     * Canonical path-based document id (`document_id_for_path`) — the reliable-sync `document_hash`
     * for a file (sidecar-retirement Phase 3C). Returns a NUL-terminated string to free with
     * [agent_doc_free_string], or null on a non-UTF-8 path.
     */
    fun agent_doc_document_id_for_path(filePath: String): Pointer?

    /**
     * Whether `filePath` is an agent-doc **session document** (frontmatter/opt-in classified).
     * Reliable-sync liveness must only report session documents so the plane's open-set matches the
     * sidecar `open_agent_docs` scope — a plain source file opened as an editor tab must not enter
     * the plane. Returns 1 for a session document, 0 otherwise (including unreadable paths).
     */
    fun agent_doc_is_session_document(filePath: String): Int

    /**
     * Enqueue a JSON batch of externally-tagged `LivenessOp`s
     * (`[{"Open":{"document_hash":..,"pid":..,"tag":..}}, ..]`) into a document's durable
     * reliable-sync push outbox (`#lzsync` Phase 3C). No-op unless the controller dual-run flag is
     * on. Returns 0 on success, -1 on error.
     */
    fun agent_doc_reliable_sync_liveness_enqueue(
        projectRoot: String,
        documentHash: String,
        opsJson: String,
    ): Int

    /**
     * Flush a document's durable reliable-sync push outbox to the controller. Returns the ack
     * cursor (>= 0) on success, -1 on error.
     */
    fun agent_doc_reliable_sync_liveness_flush(projectRoot: String, documentHash: String): Long

    /** Durably enqueue + flush one incremental `Vec<TextOp>` JSON delta. */
    fun agent_doc_reliable_sync_document_op_push(
        projectRoot: String,
        filePath: String,
        deltaJson: String,
    ): Int

    /** Bounded genuine-reattach recovery carrying editor text, never op history. */
    fun agent_doc_reliable_sync_text_adopt_push(
        projectRoot: String,
        filePath: String,
        text: String,
    ): Int

    /** Retry a retained document-op suffix without enqueueing a new frame. */
    fun agent_doc_reliable_sync_document_op_flush(projectRoot: String, filePath: String): Int

    /**
     * `#ctrlkillreregister` Tier 3 — which of THIS editor's registrations the controller currently
     * holds no replica for.
     *
     * The editor is the only process that can create its own replica, so it asks about itself and
     * repairs, instead of being pushed a rebuild request. A push has to reach the editor — the
     * failure behind `reload-lib reached 1/4 endpoints` — while this is driven by a process that is
     * provably alive, because it just called. It is therefore correct whichever side restarted.
     *
     * [heldJson] is a JSON array of document hashes this editor already has a replica for. Returns
     * a JSON array of `EditorRegistration` objects to rebuild (`[]` when up to date), or null when
     * the ABI is unavailable or the controller could not be reached. Caller must free a non-null
     * result with [agent_doc_free_string].
     */
    fun agent_doc_peer_replicas_missing(
        projectRoot: String,
        pid: Long,
        heldJson: String,
    ): Pointer?

    /**
     * Commit the document at [filePath] to git. Call after successfully applying a patch as a
     * defense-in-depth guarantee that the agent response is tracked even if the shell-side --commit
     * was skipped.
     *
     * @param filePath absolute path to the document file
     * @return true on success, false on failure
     */
    fun agent_doc_commit(filePath: String): Boolean

    /**
     * Record one real editor operation for CRDT-based op replay (`#qnodemerge4wire`).
     * `offset`/`deleteLen` are UTF-8 BYTE units — the reporter converts the editor's UTF-16
     * offset/length first. `opKind` is `"insert"` (with `insertText`, `deleteLen=0`) or `"delete"`
     * (with `insertText=null`, `deleteLen=byteLen`). A replacement is reported as a delete then an
     * insert at the same offset. `baseHash` comes from [agent_doc_document_base_hash]. Returns 1 on
     * success, 0 on any error (bad input/offset) so the reporter can ignore and fall back to the
     * diff-guess.
     */
    fun agent_doc_record_editor_op(
        filePath: String,
        baseHash: String,
        opKind: String,
        offset: Long,
        insertText: String?,
        deleteLen: Long,
    ): Int

    /**
     * Record one ordered editor-op burst as a single bounded state-ledger transaction. [opsJson]
     * uses the Rust EditorOp JSON shape.
     */
    fun agent_doc_record_editor_ops_json(
        filePath: String,
        baseHash: String,
        opsJson: String,
    ): Int

    /**
     * End the current operator-op epoch before a remote/agent projection changes the editor
     * frontier. This prevents later local edits from being appended to operations captured against
     * the pre-projection merge base.
     */
    fun agent_doc_clear_editor_op_epoch(filePath: String): Int

    /**
     * Compute the base hash captured ops must be stamped with so the write-time merge accepts them
     * (`#qnodemerge4wire`) — the SHA256 hex of the resolved CRDT merge base text. Returns a string
     * pointer (null on error → skip op capture this edit). Caller must free with
     * [agent_doc_free_string].
     */
    fun agent_doc_document_base_hash(filePath: String): Pointer?

    // --- CRDT editor-as-replica FFI node (`#crdtauth5`, plan phase 3/5) -------
    //
    // The cdylib hosts the per-editor yrs replica; the plugin stays THIN — it
    // forwards a local `Document` delta into the replica and applies remote
    // updates back, with NO CRDT logic in Kotlin (the yrs replica, state-vector
    // logic, and op encode/decode all live once in Rust). yrs update / state-
    // vector buffers cross the boundary as raw byte arrays.

    /**
     * Open the per-editor yrs replica for [replica_id]. Pass a stable, unique client-id (mint one
     * from a stable editor-process identity so two IDEs never collide). When [init_state] is
     * non-null/non-empty it bootstraps the replica from that encoded state (e.g. the canonical
     * bootstrap returned by the Project Controller `replica_register` ack). Returns 0 on success,
     * negative on error.
     */
    fun agent_doc_replica_open(replica_id: Long, init_state: ByteArray?, init_len: Long): Int

    /**
     * Apply a LOCAL edit to the replica (delete [delete_len] chars at [offset], then insert
     * [insert]). Offsets/lengths are yrs char units. Returns 0 on success, negative on error.
     */
    fun agent_doc_replica_apply_local(
        replica_id: Long,
        offset: Int,
        delete_len: Int,
        insert: String,
    ): Int

    /** The replica's converged text. Caller must free with [agent_doc_free_string]. */
    fun agent_doc_replica_text(replica_id: Long): Pointer?

    /**
     * The replica's encoded state vector (the compact causal summary to announce to a peer). Writes
     * the length into [out_len]. Caller must free the returned buffer with [agent_doc_free_state].
     */
    fun agent_doc_replica_state_vector(
        replica_id: Long,
        out_len: com.sun.jna.ptr.LongByReference,
    ): Pointer?

    /**
     * The incremental update carrying exactly the ops [their_sv] is missing — the delta to fan out
     * to a peer. Caller must free with [agent_doc_free_state].
     */
    fun agent_doc_replica_diff(
        replica_id: Long,
        their_sv: ByteArray?,
        their_sv_len: Long,
        out_len: com.sun.jna.ptr.LongByReference,
    ): Pointer?

    /**
     * Apply a remote update (idempotent + causal-buffered by yrs). Returns 0 on success, negative
     * on error.
     */
    fun agent_doc_replica_apply_update(replica_id: Long, update: ByteArray, update_len: Long): Int

    /**
     * The full encoded state — the bootstrap snapshot for a peer's first contact or a durable
     * projection. Caller must free with [agent_doc_free_state].
     */
    fun agent_doc_replica_encode_state(
        replica_id: Long,
        out_len: com.sun.jna.ptr.LongByReference,
    ): Pointer?

    /** Close the replica, freeing its yrs doc. Returns 0 on success. */
    fun agent_doc_replica_close(replica_id: Long): Int

    /** Free a byte buffer returned by a replica state/diff/encode call. */
    fun agent_doc_free_state(ptr: Pointer?, len: Long)

    /** Free a string returned by any agent_doc_* function. */
    fun agent_doc_free_string(ptr: Pointer?)

    companion object {
        private class LoadedGeneration
        private constructor(
            val proxy: AgentDocLib,
            private val delegate: AgentDocLib,
            private val handler: Library.Handler,
            private val executor: ExecutorService,
            private val workerThreads: Set<Thread>,
            val loadTarget: String,
        ) {
            private val callMonitor = java.lang.Object()
            private val activeCalls = AtomicInteger(0)
            @Volatile private var acceptingCalls = true

            fun stopAcceptingAndAwait(timeoutMs: Long): Boolean {
                val deadline = System.nanoTime() + timeoutMs * 1_000_000L
                synchronized(callMonitor) {
                    acceptingCalls = false
                    while (activeCalls.get() != 0) {
                        val remainingNanos = deadline - System.nanoTime()
                        if (remainingNanos <= 0L) return false
                        val waitMillis = (remainingNanos / 1_000_000L).coerceAtLeast(1L)
                        callMonitor.wait(waitMillis)
                    }
                    return true
                }
            }

            fun resumeCalls() {
                synchronized(callMonitor) {
                    acceptingCalls = true
                    callMonitor.notifyAll()
                }
            }

            fun resumeNativeAfterFailure(): Boolean {
                return try {
                    callOnWorker("agent_doc_resume_after_reload_failure") {
                        delegate.agent_doc_resume_after_reload_failure()
                    }
                    true
                } catch (error: Throwable) {
                    LOG.warn(
                        "[native] failed to resume retained native generation: ${error.message}"
                    )
                    false
                }
            }

            fun retireAndClose(timeoutMs: Long): Boolean {
                executor.shutdown()
                val terminated =
                    try {
                        executor.awaitTermination(timeoutMs, TimeUnit.MILLISECONDS)
                    } catch (_: InterruptedException) {
                        Thread.currentThread().interrupt()
                        false
                    }
                if (!terminated) return false
                handler.nativeLibrary.close()
                return true
            }

            fun requireReloadAbi() {
                listOf(
                        "agent_doc_version",
                        "agent_doc_quiesce_for_reload",
                        "agent_doc_resume_after_reload_failure",
                        "agent_doc_start_ipc_listener_v2",
                        "agent_doc_stop_ipc_listener",
                        "agent_doc_reliable_sync_liveness_enqueue",
                        "agent_doc_reliable_sync_liveness_flush",
                        "agent_doc_reliable_sync_document_op_push",
                        "agent_doc_deferred_write_post_register_project",
                    )
                    .forEach { symbol ->
                        handler.nativeLibrary.getFunction(symbol)
                    }
            }

            private fun <T> callOnWorker(methodName: String, call: () -> T): T {
                if (workerThreads.contains(Thread.currentThread())) return call()
                check(!SwingUtilities.isEventDispatchThread()) {
                    "agent-doc native calls are forbidden on the IDEA event-dispatch thread"
                }
                if (nativeCallLaneUtil(methodName) == NativeCallLane.IsolatedCaller) {
                    val started = System.nanoTime()
                    return try {
                        call()
                    } finally {
                        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)
                        if (elapsedMs > NATIVE_CALL_TIMEOUT_MS) {
                            LOG.warn(
                                "[native] isolated call $methodName ran for ${elapsedMs}ms on its caller lane; " +
                                    "native generation retained and unrelated reactive ingress remains available"
                            )
                        }
                    }
                }
                return callOnNativeWorker(
                    executor = executor,
                    workerThreads = workerThreads,
                    timeoutMs = NATIVE_CALL_TIMEOUT_MS,
                    onRunningTimeout = {
                        synchronized(callMonitor) {
                            acceptingCalls = false
                            callMonitor.notifyAll()
                        }
                        executor.shutdownNow()
                        val reason =
                            "native generation lifecycle call $methodName ran longer than ${NATIVE_CALL_TIMEOUT_MS}ms; " +
                                "disabled the wedged generation to keep the IDE responsive"
                        poisonGeneration(this, reason)
                        throw IllegalStateException(reason)
                    },
                    call = call,
                )
            }

            companion object {
                fun load(path: String): LoadedGeneration {
                    val handler =
                        Library.Handler(path, AgentDocLib::class.java, emptyMap<String, Any>())
                    val delegate =
                        Proxy.newProxyInstance(
                            AgentDocLib::class.java.classLoader,
                            arrayOf(AgentDocLib::class.java),
                            handler,
                        ) as AgentDocLib
                    val workerThreads = ConcurrentHashMap.newKeySet<Thread>()
                    val executor =
                        newNativeGenerationExecutor(workerThreads, NATIVE_GENERATION_WORKER_COUNT)
                    lateinit var generation: LoadedGeneration
                    val guarded =
                        Proxy.newProxyInstance(
                            AgentDocLib::class.java.classLoader,
                            arrayOf(AgentDocLib::class.java),
                        ) { _, method, args ->
                            synchronized(generation.callMonitor) {
                                if (!generation.acceptingCalls) {
                                    throw IllegalStateException("native generation is quiescing")
                                }
                                generation.activeCalls.incrementAndGet()
                            }
                            try {
                                generation.callOnWorker(method.name) {
                                    try {
                                        method.invoke(delegate, *(args ?: emptyArray()))
                                    } catch (error: InvocationTargetException) {
                                        throw error.targetException
                                    }
                                }
                            } catch (error: InvocationTargetException) {
                                throw error.targetException
                            } finally {
                                synchronized(generation.callMonitor) {
                                    if (generation.activeCalls.decrementAndGet() == 0) {
                                        generation.callMonitor.notifyAll()
                                    }
                                }
                            }
                        } as AgentDocLib
                    generation =
                        LoadedGeneration(
                            guarded,
                            delegate,
                            handler,
                            executor,
                            workerThreads,
                            path,
                        )
                    return generation
                }
            }
        }

        @Volatile private var instance: AgentDocLib? = null
        @Volatile private var loadedGeneration: LoadedGeneration? = null
        @Volatile private var loadError: String? = null
        @Volatile private var loadedPath: String? = null
        @Volatile private var loadedMtime: Long = 0L
        @Volatile private var failedReloadMtime: Long = 0L
        @Volatile private var currentLockFile: File? = null
        private var shutdownHookRegistered = false
        private const val NATIVE_QUIESCE_TIMEOUT_MS = 7_000L
        private const val NATIVE_CALL_TIMEOUT_MS = 10_000L
        private const val NATIVE_GENERATION_WORKER_COUNT = 4

        @Synchronized
        fun get(): AgentDocLib? {
            val current = instance
            val path = loadedPath

            if (current != null && path != null) {
                val currentMtime = File(path).lastModified()
                if (currentMtime != failedReloadMtime && libMtimeChanged(path, loadedMtime)) {
                    NativeReloadCoordinator.requestReload("mtime")
                }
                return current
            }

            if (loadError != null) return null

            val libPath =
                resolveLibPath()
                    ?: run {
                        loadError = "agent-doc binary not found; FFI unavailable"
                        LOG.warn(loadError!!)
                        return null
                    }

            return loadFrom(libPath)
        }

        @Synchronized
        internal fun hotReload(libVersion: String? = null): NativeReloadOutcome {
            val path =
                loadedPath
                    ?: resolveLibPath()
                    ?: return NativeReloadOutcome.RetainedOld("libagent_doc path is unavailable")
            val old = loadedGeneration
            if (old == null) {
                return if (loadFrom(path) != null) {
                    NativeReloadOutcome.Reloaded(loadedMtime)
                } else {
                    NativeReloadOutcome.RetainedOld(loadError ?: "initial native load failed")
                }
            }
            val targetMtime = File(path).lastModified()
            if (targetMtime != 0L && targetMtime == failedReloadMtime) {
                return NativeReloadOutcome.RetainedOld(
                    "replacement mtime $targetMtime already failed validation"
                )
            }
            if (
                nativeReloadTransition(loadedMtime, targetMtime, true, true) ==
                    NativeReloadTransition.KeepCurrent
            ) {
                return NativeReloadOutcome.AlreadyCurrent
            }

            val nativeQuiesced =
                try {
                    old.proxy.agent_doc_quiesce_for_reload(NATIVE_QUIESCE_TIMEOUT_MS) == 1
                } catch (error: Throwable) {
                    LOG.warn("[native] generation quiesce failed: ${error.message}")
                    false
                }
            if (!nativeQuiesced) {
                return NativeReloadOutcome.RetainedOld("native listener/replica quiesce timed out")
            }
            val callsDrained = old.stopAcceptingAndAwait(NATIVE_QUIESCE_TIMEOUT_MS)
            if (!callsDrained) {
                old.resumeNativeAfterFailure()
                old.resumeCalls()
                return NativeReloadOutcome.RetainedOld("native calls did not drain")
            }

            val loadTarget = shadowCopyForLoad(path, targetMtime)
            val oldMtime = loadedMtime
            if (!old.retireAndClose(NATIVE_QUIESCE_TIMEOUT_MS)) {
                return markRestartRequired(
                    "old native generation worker did not terminate after calls drained"
                )
            }
            loadedGeneration = null
            instance = null
            when (nativeRetiredGenerationTransition(nativeGenerationIsUnmapped(old.loadTarget))) {
                NativeRetiredGenerationTransition.LoadReplacement -> Unit
                NativeRetiredGenerationTransition.LoadReplacementRetainingInertMapping -> {
                    // glibc may retain a Rust cdylib mapping after dlclose because
                    // another JVM thread once acquired Rust TLS. That is not a live
                    // generation: native quiesce stopped its listeners/replicas,
                    // the guarded calls drained, its owner pool terminated, and
                    // JNA closed the handle. The replacement has a distinct shadow
                    // path/inode, so publish it and retain this inert mapping until
                    // process exit instead of reopening stale code.
                    LOG.info(
                        "[native] retired generation remains inertly mapped after dlclose; " +
                            "continuing distinct-inode replacement handoff"
                    )
                }
            }

            val replacement =
                try {
                    loadValidatedGeneration(loadTarget, path)
                } catch (replacementError: Throwable) {
                    failedReloadMtime = targetMtime
                    val restored =
                        try {
                            loadValidatedGeneration(old.loadTarget, path)
                        } catch (restoreError: Throwable) {
                            LOG.warn(
                                "[native] replacement and old-generation restore both failed",
                                restoreError,
                            )
                            return markRestartRequired(
                                "replacement load failed (${replacementError.message}); " +
                                    "old generation restore failed (${restoreError.message})"
                            )
                        }
                    if (!restored.resumeNativeAfterFailure()) {
                        return markRestartRequired(
                            "replacement load failed (${replacementError.message}); " +
                                "restored generation did not resume"
                        )
                    }
                    publishGeneration(restored, path, oldMtime)
                    LOG.warn(
                        "[native] replacement load failed; restored prior generation: ${replacementError.message}"
                    )
                    return NativeReloadOutcome.RetainedOld(
                        "replacement load failed; restored prior generation: ${replacementError.message}"
                    )
                }
            publishGeneration(replacement, path, targetMtime)
            failedReloadMtime = 0L
            pruneRetiredShadow(old.loadTarget, replacement.loadTarget)
            LOG.info(
                "[native] hot-reloaded libagent_doc v${libVersion ?: "?"} from $path " +
                    "after quiesce/close handoff"
            )
            return NativeReloadOutcome.Reloaded(targetMtime)
        }

        @Synchronized
        private fun loadFrom(path: String): AgentDocLib? {
            return try {
                loadedPath = path
                loadedMtime = File(path).lastModified()
                val loadTarget = shadowCopyForLoad(path, loadedMtime)
                val generation = loadValidatedGeneration(loadTarget, path)
                publishGeneration(generation, path, loadedMtime)
                registerShutdownHook()
                generation.proxy
            } catch (error: Throwable) {
                loadError = "Failed to load libagent_doc: ${error.message}"
                LOG.warn(loadError!!)
                null
            }
        }

        private fun loadValidatedGeneration(
            loadTarget: String,
            canonicalPath: String,
        ): LoadedGeneration {
            val generation = LoadedGeneration.load(loadTarget)
            try {
                generation.requireReloadAbi()
                verifyVersion(generation.proxy, canonicalPath)
                return generation
            } catch (error: Throwable) {
                try {
                    generation.retireAndClose(NATIVE_QUIESCE_TIMEOUT_MS)
                } catch (_: Throwable) {}
                throw error
            }
        }

        private fun publishGeneration(generation: LoadedGeneration, path: String, mtime: Long) {
            loadedPath = path
            loadedMtime = mtime
            loadedGeneration = generation
            instance = generation.proxy
            loadError = null
            removePidLock()
            writePidLock(path)
        }

        private fun markRestartRequired(reason: String): NativeReloadOutcome.RestartRequired {
            loadedGeneration = null
            instance = null
            loadError = "IDE restart required: $reason"
            removePidLock()
            LOG.warn("[native] $loadError")
            return NativeReloadOutcome.RestartRequired(reason)
        }

        @Synchronized
        private fun poisonGeneration(generation: LoadedGeneration, reason: String) {
            if (loadedGeneration !== generation) return
            loadedGeneration = null
            instance = null
            loadError = "IDE restart required: $reason"
            removePidLock()
            LOG.warn("[native] $loadError")
        }

        private fun nativeGenerationIsUnmapped(path: String): Boolean {
            if (!System.getProperty("os.name").lowercase().contains("linux")) {
                return false
            }
            val maps =
                try {
                    File("/proc/self/maps").readText()
                } catch (error: Exception) {
                    LOG.warn("[native] cannot inspect /proc/self/maps after dlclose", error)
                    return false
                }
            return !nativePathIsMapped(path, maps)
        }

        private fun verifyVersion(lib: AgentDocLib, path: String) {
            val ptr =
                lib.agent_doc_version()
                    ?: throw IllegalStateException("agent_doc_version() returned null at $path")
            try {
                val version = ptr.getString(0)
                require(version.isNotBlank()) {
                    "agent_doc_version() returned an empty version at $path"
                }
                LOG.info("[native] loaded libagent_doc v$version from $path")
            } finally {
                lib.agent_doc_free_string(ptr)
            }
        }

        /**
         * Resolve the load target for [canonicalPath]. The per-process shadow copy avoids mutating
         * a mapped library during installation and gives each active generation a distinct handle.
         * Falls back to the canonical path in place if the copy fails.
         */
        private fun shadowCopyForLoad(canonicalPath: String, mtime: Long): String {
            val cacheRoot = nativeCacheRoot()
            val shadow = nativeShadowCopyPath(canonicalPath, mtime, cacheRoot)
            if (shadow == null) {
                LOG.warn(
                    "[native] shadow copy failed; loading canonical path in place (may keep stale native code until restart)"
                )
                return canonicalPath
            }
            return shadow
        }

        private fun pruneRetiredShadow(retiredPath: String, activePath: String) {
            if (retiredPath == activePath) return
            val retired = File(retiredPath)
            if (!retired.name.startsWith("libagent_doc-")) return
            if (System.getProperty("os.name").lowercase().contains("linux")) {
                val maps =
                    try {
                        File("/proc/self/maps").readText()
                    } catch (error: Exception) {
                        LOG.debug(
                            "[native] cannot prove retired shadow is unmapped: ${error.message}"
                        )
                        return
                    }
                if (nativePathIsMapped(retiredPath, maps)) {
                    LOG.debug(
                        "[native] retaining inert mapped shadow until process exit: $retiredPath"
                    )
                    return
                }
            } else {
                // Rust TLS destructors can defer dlclose on Unix-like hosts. If
                // the host has no authoritative mapping view, retain the file
                // rather than turning a still-mapped generation into `(deleted)`.
                return
            }
            try {
                if (retired.exists() && !retired.delete()) {
                    LOG.debug("[native] retired shadow remains on disk: $retiredPath")
                }
            } catch (error: Exception) {
                LOG.debug("[native] failed to prune retired shadow: ${error.message}")
            }
        }

        private fun nativeCacheRoot(): File {
            val pid = ProcessHandle.current().pid()
            return File(System.getProperty("java.io.tmpdir"), "agent-doc-native-$pid")
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
            Runtime.getRuntime()
                .addShutdownHook(
                    Thread {
                        removePidLock()
                        try {
                            nativeCacheRoot().deleteRecursively()
                        } catch (_: Exception) {}
                    }
                )
        }

        private fun resolveLibPath(): String? {
            try {
                val process =
                    ProcessBuilder("agent-doc", "lib-path").redirectErrorStream(false).start()
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

        private val LOG =
            com.intellij.openapi.diagnostic.Logger.getInstance(AgentDocLib::class.java)
    }
}

/**
 * Safe wrappers for controller-backed admin FFI calls.
 *
 * These return the same JSON payloads as the CLI `agent-doc admin ... --json` commands. Editors
 * parse/display the payload; ownership and generation checks stay inside the Rust controller.
 */
object NativeAdminControls {
    private val LOG =
        com.intellij.openapi.diagnostic.Logger.getInstance(NativeAdminControls::class.java)
    private const val NO_GENERATION: Long = -1

    fun inspect(
        projectRoot: String? = null,
        documentPath: String? = null,
        sessionId: String? = null,
        paneId: String? = null,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
                lib.agent_doc_admin_inspect_json(projectRoot, documentPath, sessionId, paneId)
            } catch (e: Throwable) {
                LOG.warn("[native] admin_inspect unavailable: ${e.message}")
                return null
            }
        return decodeJsonResult(lib, result, "admin_inspect")
    }

    fun tmuxFocusState(projectRoot: String? = null): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
                lib.agent_doc_tmux_focus_state_json(projectRoot)
            } catch (e: Throwable) {
                LOG.warn("[native] tmux_focus_state unavailable: ${e.message}")
                return null
            }
        return decodeJsonResult(lib, result, "tmux_focus_state")
    }

    fun focusDocumentPane(
        projectRoot: String? = null,
        documentPath: String,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
                lib.agent_doc_focus_document_pane_json(projectRoot, documentPath)
            } catch (e: Throwable) {
                LOG.warn("[native] focus_document_pane unavailable: ${e.message}")
                return null
            }
        return decodeJsonResult(lib, result, "focus_document_pane")
    }

    fun syncTmuxLayout(
        projectRoot: String? = null,
        columnsJson: String,
        window: String? = null,
        focus: String? = null,
        noAutostart: Boolean = false,
        exactVisible: Boolean = false,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
                lib.agent_doc_sync_tmux_layout_json(
                    projectRoot,
                    columnsJson,
                    window,
                    focus,
                    if (noAutostart) 1 else 0,
                    if (exactVisible) 1 else 0,
                )
            } catch (e: Throwable) {
                LOG.warn("[native] sync_tmux_layout unavailable: ${e.message}")
                return null
            }
        return decodeJsonResult(lib, result, "sync_tmux_layout")
    }

    /**
     * Report one editor-surface observation (`#jbsurfaceswap`).
     *
     * Returns the receipt JSON, or `null` when the native library is unavailable or the observation
     * was rejected. Callers report; they do not plan.
     */
    fun editorSurfaceObserve(
        projectRoot: String,
        surfaceJson: String,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
                lib.agent_doc_editor_surface_observe_json(projectRoot, surfaceJson)
            } catch (e: Throwable) {
                LOG.warn("[native] editor_surface_observe unavailable: ${e.message}")
                return null
            }
        return decodeJsonResult(lib, result, "editor_surface_observe")
    }

    fun editorSurfaceEnqueue(
        projectRoot: String,
        surfaceJson: String,
    ): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_editor_surface_enqueue_json(projectRoot, surfaceJson) == 1
        } catch (e: Throwable) {
            LOG.warn("[native] editor_surface_enqueue unavailable: ${e.message}")
            false
        }
    }

    fun documentAuthority(
        projectRoot: String,
        documentPath: String,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
                lib.agent_doc_document_authority_json(projectRoot, documentPath)
            } catch (e: Throwable) {
                LOG.warn("[native] document_authority unavailable: ${e.message}")
                return null
            }
        return decodeJsonResult(lib, result, "document_authority")
    }

    fun currentDocumentAuthority(projectRoot: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
                lib.agent_doc_current_document_authority_json(projectRoot)
            } catch (e: Throwable) {
                LOG.warn("[native] current_document_authority unavailable: ${e.message}")
                return null
            }
        return decodeJsonResult(lib, result, "current_document_authority")
    }

    /** Release a project root's editor-surface graph. */
    fun editorSurfaceForget(projectRoot: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_editor_surface_forget(projectRoot) == 1
        } catch (e: Throwable) {
            LOG.warn("[native] editor_surface_forget unavailable: ${e.message}")
            false
        }
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
        val result =
            try {
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
        val result =
            try {
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
        val result =
            try {
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
        val result =
            try {
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

/** Safe wrapper around FFI calls with automatic memory management. */
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
        nodePatchesAvailable?.let {
            return it
        }
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
            }
            .also { nodePatchesAvailable = it }
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
     * Apply node-keyed IPC patches using the native library. Returns the patched document, or null
     * if FFI is unavailable/errors.
     */
    fun applyNodePatches(doc: String, nodePatchesJson: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            try {
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
     * Apply a component patch using the native library. Returns the patched document, or null if
     * FFI is unavailable/errors.
     */
    fun applyComponentPatch(
        doc: String,
        component: String,
        content: String,
        mode: String,
    ): String? {
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
     * Apply a component patch with cursor-aware ordering. When mode is "append" and caretOffset >=
     * 0, inserts before the caret. Returns the patched document, or null if FFI is
     * unavailable/errors.
     */
    fun applyComponentPatchWithCaret(
        doc: String,
        component: String,
        content: String,
        mode: String,
        caretOffset: Int,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            lib.agent_doc_apply_patch_with_caret(doc, component, content, mode, caretOffset)
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
     * Apply a component patch using a boundary marker for insertion point. Returns the patched
     * document, or null if FFI is unavailable/errors.
     */
    fun applyComponentPatchWithBoundary(
        doc: String,
        component: String,
        content: String,
        mode: String,
        boundaryId: String,
    ): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            lib.agent_doc_apply_patch_with_boundary(doc, component, content, mode, boundaryId)
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
     * Reposition boundary marker to end of exchange component via FFI. Removes all stale
     * boundaries, inserts a single fresh 8-char one. Returns the cleaned document, or null if FFI
     * is unavailable/errors.
     */
    fun repositionBoundaryToEnd(doc: String, boundaryId: String? = null): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            if (boundaryId.isNullOrBlank()) {
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
     * Reposition boundary marker to end of exchange component via FFI, preserving transient (HEAD)
     * markers on Re: headings. Returns the repositioned document, or null if FFI is
     * unavailable/errors.
     */
    fun repositionBoundaryToEndPreserveHead(doc: String, boundaryId: String? = null): String? {
        val lib = AgentDocLib.get() ?: return null
        val result =
            if (boundaryId.isNullOrBlank()) {
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
     * Normalize/fail-close template structure using the shared Rust guard. Returns null when FFI is
     * unavailable or when the guard rejects the document.
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
     * CRDT 3-way merge from plain text. Returns the conflict-free merged text, or null if FFI is
     * unavailable. Never returns null on merge conflict — CRDT is always conflict-free.
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
     * Resolve a Lazily-retained deferred write for a re-registering editor. Returns null when there
     * is no pending target or recovery is unavailable.
     */
    fun deferredWriteReconnectContent(filePath: String, editorContent: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val ptr = lib.agent_doc_deferred_write_reconnect_content(filePath, editorContent)
        try {
            return ptr?.getString(0)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /**
     * Replay a deferred semantic write only after the exact editor cut has become the registered
     * CRDT baseline.
     */
    fun deferredWritePostRegisterContent(filePath: String, editorContent: String): String? {
        val lib = AgentDocLib.get() ?: return null
        val ptr =
            try {
                lib.agent_doc_deferred_write_post_register_content(filePath, editorContent)
            } catch (e: UnsatisfiedLinkError) {
                LOG.debug("[native] deferred-write post-register replay unavailable: ${e.message}")
                return null
            }
        try {
            return ptr?.getString(0)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /**
     * Project a retained post-registration semantic intent into the controller-owned CRDT
     * authority. This is transport-only: the returned status never carries editor bytes for Kotlin
     * to apply directly.
     */
    fun projectDeferredWritePostRegister(filePath: String, editorContent: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return try {
            lib.agent_doc_deferred_write_post_register_project(filePath, editorContent) == 1
        } catch (e: UnsatisfiedLinkError) {
            LOG.debug("[native] deferred-write post-register projection unavailable: ${e.message}")
            false
        }
    }

    fun deferredWriteReconnectPropagated(filePath: String, editorContent: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        return lib.agent_doc_deferred_write_reconnect_propagated(filePath, editorContent) == 1
    }

    /**
     * Resolve the nearest agent-doc project root for an absolute file path.
     *
     * Returns `(projectRoot, relativePath)` where `projectRoot` is the nearest ancestor directory
     * containing `.agent-doc/`, and `relativePath` is [absPath] relative to that root. Returns
     * `null` when no ancestor has `.agent-doc/` or FFI is unavailable — callers must fall back to
     * the workspace `basePath`.
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

    /** Collect visual token ranges for agent-doc-specific markdown structures. */
    fun visualTokensOrNull(doc: String): List<VisualToken>? {
        val lib = AgentDocLib.get() ?: return null
        val ptr = lib.agent_doc_visual_tokens_json(doc) ?: return null
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
            return null
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    fun visualTokens(doc: String): List<VisualToken> = visualTokensOrNull(doc).orEmpty()

    /**
     * Merge frontmatter fields using the native library. Returns the updated document, or null if
     * FFI is unavailable/errors.
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
     * Converge the `agent:queue` opening-tag `auto` attribute to [wantAuto] using the native
     * library. Returns the updated document (unchanged if there is no queue component or the tag
     * already matches), or null if FFI is unavailable/errors.
     * See #adoc-queue-ipc-buffer-divergence.
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
