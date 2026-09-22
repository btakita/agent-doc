package com.github.btakita.agentdoc

import java.util.concurrent.ConcurrentHashMap

/**
 * Tracks the live "agent input required" notification per document so it can be retracted.
 *
 * `#jbinputrequiredstale`: the input-required balloon is raised sticky (`isImportant`), so a
 * fire-and-forget `notify` outlives the condition that justified it. Once the operator answers the
 * prompt — or the turn ends, or the session dies — the actor leaves `WaitingInput`, but the balloon
 * stays on screen claiming input is still required, and the next `false -> true` edge stacks
 * another one beside it. Retaining the handle makes the notification a projection of the current
 * state rather than a log of past edges: at most one per document, expired as soon as the document
 * stops requiring input, closes, or the refresher is disposed.
 *
 * Generic over the handle type so the lifecycle is unit-testable without the IntelliJ platform.
 */
internal class InputRequiredNotifications<T : Any>(private val expire: (T) -> Unit) {
    private val live = ConcurrentHashMap<String, T>()

    /**
     * Raises a notification for [filePath] unless one is already live for it.
     *
     * [create] is invoked at most once per outstanding notification. Returns true when a new
     * notification was created and retained, false when an existing one already covers [filePath].
     */
    fun raise(filePath: String, create: () -> T): Boolean {
        var raised = false
        live.compute(filePath) { _, existing ->
            existing ?: create().also { raised = true }
        }
        return raised
    }

    /** Expires the live notification for [filePath], if any. No-op when none is outstanding. */
    fun clear(filePath: String) {
        live.remove(filePath)?.let(expire)
    }

    /** Expires every outstanding notification. */
    fun clearAll() {
        val outstanding = live.keys.toList()
        outstanding.forEach { live.remove(it)?.let(expire) }
    }

    /** Test seam: whether a notification is currently outstanding for [filePath]. */
    fun isLive(filePath: String): Boolean = live.containsKey(filePath)
}
