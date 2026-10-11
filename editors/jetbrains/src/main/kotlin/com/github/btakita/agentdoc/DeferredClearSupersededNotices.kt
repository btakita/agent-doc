package com.github.btakita.agentdoc

import java.util.concurrent.ConcurrentHashMap

/**
 * `#clearsupersedenotice`: a queued Clear Session Context the supervisor dropped because a newer
 * prompt started on the pane before the turn it was queued behind ended. Carried by the controller's
 * document-turn authority stream as `deferred_clear_superseded`.
 */
internal data class DeferredClearSuperseded(
    val clearEpoch: Long,
    val pane: String,
    val reason: String,
)

/** One ready authority frame's view of the dropped-clear field (`null` notice = none projected). */
internal data class DeferredClearSupersededObservation(val notice: DeferredClearSuperseded?)

/**
 * Edge detector for the dropped-clear notice: notify once per dropped `clear_epoch`.
 *
 * The first ready frame observed for a document only seeds the baseline, so a drop that happened
 * before the editor was watching (reopened tab, plugin restart) is not re-announced. A frame
 * without the field never resets the baseline, so a reconnect cannot re-raise the same drop.
 * Pure and platform-free so the policy is unit-testable.
 */
internal class DeferredClearSupersededNotices {
    private val lastEpoch = ConcurrentHashMap<String, Long>()

    /** Returns true when [notice] is a drop this document has not announced yet. */
    fun observe(filePath: String, notice: DeferredClearSuperseded?): Boolean {
        var raise = false
        lastEpoch.compute(filePath) { _, seen ->
            when {
                seen == null -> notice?.clearEpoch ?: NONE
                notice != null && notice.clearEpoch != seen -> {
                    raise = true
                    notice.clearEpoch
                }
                else -> seen
            }
        }
        return raise
    }

    /** Drops the baseline for a closed document; reopening re-seeds without notifying. */
    fun forget(filePath: String) {
        lastEpoch.remove(filePath)
    }

    fun clearAll() {
        lastEpoch.clear()
    }

    private companion object {
        const val NONE = -1L
    }
}
