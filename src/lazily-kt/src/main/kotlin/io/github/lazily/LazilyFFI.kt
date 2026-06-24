package io.github.lazily

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer

/**
 * JNA bindings to the lazily / agent-doc FFI surface.
 *
 * Wraps `agent_doc_state_projection` and `agent_doc_record_state_event`
 * from the agent-doc binary's `ffi.rs` (the `#lzffistate` FFI layer).
 * NOT a reactive-core port — the plugins consume authoritative projections
 * and report state events over this thin C-ABI channel.
 */
interface LazilyFFI : Library {
    companion object {
        fun load(libName: String = "agent_doc"): LazilyFFI {
            return Native.load(libName, LazilyFFI::class.java)
        }
    }

    /**
     * Read the binary's state projection for a document.
     *
     * Returns a NUL-terminated JSON string of `DocumentStateProjection`
     * (document/queue/closeout/transport/supervisor/route/proof slices),
     * or `"null"` when no events have been recorded.
     *
     * Caller MUST free the returned pointer with [agent_doc_free_string].
     */
    fun agent_doc_state_projection(documentHash: String): Pointer

    /**
     * Record a lazily state-backbone event from a plugin.
     *
     * `factJson` must be a JSON object deserializable as a `StateEvent`.
     * Returns 1 on success, 0 on failure.
     */
    fun agent_doc_record_state_event(documentHash: String, factJson: String): Int

    /** Free a string pointer returned by [agent_doc_state_projection]. */
    fun agent_doc_free_string(ptr: Pointer)
}
