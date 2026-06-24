package io.github.lazily

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Holds the raw JSON projection returned by [LazilyFFI.agent_doc_state_projection].
 *
 * `null` means no state events have been recorded for the document.
 * The JSON contains document/queue/closeout/transport/supervisor/route/proof
 * slices — consumers parse it with their preferred JSON library.
 */
data class StateProjection(val json: String?) {
    val isAvailable: Boolean get() = json != null
}

/**
 * Client for the lazily state-projection FFI channel.
 *
 * Provides a [StateFlow] of [StateProjection] for reactive UI binding
 * (JetBrains `StateFlow` → Compose/screen rendering), plus [recordStateEvent]
 * for feeding facts into the binary's state backbone.
 *
 * Usage:
 * ```
 * val client = StateProjectionClient(documentHash)
 * client.refresh()        // pull latest projection from binary
 * client.projection.value // StateFlow<StateProjection>
 * client.recordStateEvent("""{"type":"BaselineSaved",...}""")
 * ```
 */
class StateProjectionClient(
    private val documentHash: String,
    private val ffi: LazilyFFI = LazilyFFI.load(),
) {
    private val _projection = MutableStateFlow(StateProjection(null))
    val projection: StateFlow<StateProjection> = _projection.asStateFlow()

    /**
     * Pull the latest state projection from the binary.
     * Updates [projection]. Safe to call on any thread (FFI is thread-safe).
     */
    fun refresh() {
        val ptr = ffi.agent_doc_state_projection(documentHash)
        try {
            val json = ptr.getString(0, "UTF-8")
            _projection.value = StateProjection(
                if (json == "null") null else json
            )
        } finally {
            ffi.agent_doc_free_string(ptr)
        }
    }

    /**
     * Record a state event in the binary's backbone.
     *
     * @param factJson JSON object deserializable as a `StateEvent`
     *                 (internally tagged `{"type":"...",...}`).
     * @return `true` if accepted, `false` on parse/ledger failure.
     */
    fun recordStateEvent(factJson: String): Boolean {
        return ffi.agent_doc_record_state_event(documentHash, factJson) == 1
    }
}
