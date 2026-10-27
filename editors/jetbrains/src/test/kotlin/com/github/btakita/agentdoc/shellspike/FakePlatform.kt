package com.github.btakita.agentdoc.shellspike

import java.util.function.Consumer

/**
 * Stand-in for the IntelliJ objects that retain whatever a plugin hands them: a message bus that
 * never forgets a subscriber (a `MessageBus.connect()` without a disposable), the event queue
 * holding a not-yet-run `invokeLater` runnable, and the IDE error pool (`MessagePool`) holding a
 * reported throwable. Retention here is strong and unbounded on purpose, so a collectable old
 * generation proves that nothing the platform keeps can reach it.
 *
 * [current] models a static platform accessor such as `ApplicationManager.getApplication()`: an
 * implementation generation can reach it through parent delegation, exactly as real
 * implementation code could reach IntelliJ APIs. Only discipline (and the leak check) stops a
 * generation from registering itself directly.
 */
class FakePlatform {
    private val subscribers = LinkedHashMap<String, MutableList<Consumer<String>>>()
    private val eventQueue = ArrayDeque<Runnable>()
    val errorPool = mutableListOf<Throwable>()

    fun subscribe(topic: String, listener: Consumer<String>) {
        subscribers.getOrPut(topic) { mutableListOf() }.add(listener)
    }

    fun subscriberCount(topic: String): Int = subscribers[topic]?.size ?: 0

    fun publish(topic: String, payload: String) {
        subscribers[topic].orEmpty().toList().forEach { it.accept(payload) }
    }

    fun invokeLater(task: Runnable) {
        eventQueue.addLast(task)
    }

    fun queuedEvents(): Int = eventQueue.size

    /** Run queued events but keep them retained, as a slow queue or a stuck modality would. */
    fun runQueuedRetaining() {
        eventQueue.toList().forEach { it.run() }
    }

    fun reportError(error: Throwable) {
        errorPool += error
    }

    companion object {
        @JvmStatic
        @Volatile
        var current: FakePlatform? = null
    }
}
