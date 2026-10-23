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

/**
 * Reconciles equivalent notifications observed on the project notification bus.
 *
 * A plugin generation owns its own [InputRequiredNotifications] map, but every generation loaded
 * in one IDE project publishes through the same IntelliJ notification topic. If an incompletely
 * retired generation still receives controller frames, each private map can legitimately decide
 * that it is raising the first notification for a document. Observing the shared bus gives the
 * current generation a classloader-neutral edge at which to expire the previous equivalent handle
 * and retain only the newest one.
 *
 * This reconciler deliberately does not expire retained handles when it is disposed: it observes
 * notifications owned by other plugin generations, and unloading one observer must not retract a
 * notification whose owning generation and input-required state are still live.
 */
internal class NotificationBusDeduplicator<T : Any, K : Any>(
    private val keyOf: (T) -> K?,
    private val expire: (T) -> Unit,
) {
    private val latest = ConcurrentHashMap<K, T>()

    /**
     * Observes [notification] and expires the preceding equivalent handle, if any.
     *
     * Returns true only when a distinct preceding handle was reconciled. Re-observing the same
     * handle (for example, during startup inventory after the bus listener saw it) is a no-op.
     */
    fun observe(notification: T): Boolean {
        val key = keyOf(notification) ?: return false
        var reconciled = false
        latest.compute(key) { _, previous ->
            if (previous != null && previous !== notification) {
                expire(previous)
                reconciled = true
            }
            notification
        }
        return reconciled
    }

    /** Forget observations without retracting notifications owned by their publishers. */
    fun forgetAll() {
        latest.clear()
    }
}
