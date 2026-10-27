package com.github.btakita.agentdoc.shell

import com.intellij.openapi.Disposable
import com.intellij.openapi.util.Disposer
import com.intellij.util.messages.MessageBus
import com.intellij.util.messages.Topic
import java.lang.reflect.InvocationHandler
import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Method
import java.lang.reflect.Proxy
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArrayList

/**
 * GH #339 R1, programmatic listeners Tier A (editors/jetbrains/SPEC.md "Who owns IntelliJ
 * registrations and callbacks"): the platform holds one shell forwarder per topic, and the
 * forwarder delivers each event to the handlers an implementation generation staged.
 *
 * One hub belongs to one owner: the application's hub to [ShellGenerationHost], a project's hub
 * to that project's `ProjectPluginLifecycleService`. A forwarder subscribes once, under the hub
 * owner, the first time a handler is staged for its key. Each handler is removed when its own
 * `parent` disposable is disposed, which is exactly when the handler's own connection or listener
 * registration was released before R1. Disposal still goes through `Disposer.register(parent, …)`,
 * so a disposed parent refuses a new handler the way `MessageBus.connect(parent)` did.
 */
class ShellListenerHub internal constructor(
    private val owner: Disposable,
    private val label: String,
    private val bus: () -> MessageBus,
) {
    private val fanOuts = ConcurrentHashMap<Any, ShellFanOut<*>>()

    /** Deliver [topic] on this hub's message bus to [handler] until [parent] is disposed. */
    fun <L : Any> subscribe(topic: Topic<L>, handler: L, parent: Disposable) {
        fanOut(topic, topic.listenerClass, topic.displayName) { forwarder ->
            bus().connect(owner).subscribe(topic, forwarder)
        }.add(handler, parent)
    }

    /**
     * Deliver one listener interface registered through an `add…Listener(listener, disposable)`
     * platform API (the editor event multicaster, the editor factory), keyed by [key].
     */
    fun <L : Any> addListener(
        key: String,
        listenerClass: Class<L>,
        handler: L,
        parent: Disposable,
        register: (forwarder: L, owner: Disposable) -> Unit,
    ) {
        fanOut(key, listenerClass, key) { forwarder -> register(forwarder, owner) }.add(handler, parent)
    }

    /** Handlers currently staged under [key] (a [Topic] or an [addListener] key); for tests. */
    internal fun handlerCount(key: Any): Int = fanOuts[key]?.size() ?: 0

    /** Forwarders this hub has handed the platform; for tests. */
    internal fun forwarderCount(): Int = fanOuts.size

    @Suppress("UNCHECKED_CAST")
    private fun <L : Any> fanOut(
        key: Any,
        listenerClass: Class<L>,
        name: String,
        connect: (L) -> Unit,
    ): ShellFanOut<L> {
        fanOuts[key]?.let { return it as ShellFanOut<L> }
        return synchronized(fanOuts) {
            (fanOuts[key] as ShellFanOut<L>?) ?: ShellFanOut(listenerClass, "$label $name").also { created ->
                connect(created.forwarder)
                fanOuts[key] = created
            }
        }
    }
}

/**
 * The shell object the platform holds for one listener interface: a JDK proxy defined by the
 * shell's own classloader whose every interface method, including default methods added by a
 * later platform, is delivered to each staged handler in staging order. Every handler sees the
 * event even when an earlier one throws; the first failure is rethrown afterwards (later ones are
 * suppressed into it), so the platform reports it as it reported a direct subscriber's failure.
 */
internal class ShellFanOut<L : Any>(private val listenerClass: Class<L>, private val name: String) : InvocationHandler {
    private class Entry(val handler: Any)

    private val entries = CopyOnWriteArrayList<Entry>()

    val forwarder: L =
        listenerClass.cast(
            Proxy.newProxyInstance(ShellFanOut::class.java.classLoader, arrayOf(listenerClass), this),
        )

    fun add(handler: L, parent: Disposable) {
        val entry = Entry(handler)
        // Register the removal first: a disposed parent throws here, before the handler is live.
        Disposer.register(parent, Disposable { entries.remove(entry) })
        entries.add(entry)
    }

    fun size(): Int = entries.size

    override fun invoke(proxy: Any, method: Method, args: Array<out Any?>?): Any? {
        if (method.declaringClass == Any::class.java) {
            return when (method.name) {
                "equals" -> proxy === args?.firstOrNull()
                "hashCode" -> System.identityHashCode(proxy)
                else -> "agent-doc shell forwarder ($name, ${listenerClass.simpleName})"
            }
        }
        var failure: Throwable? = null
        for (entry in entries) {
            try {
                method.invoke(entry.handler, *(args ?: EMPTY))
            } catch (thrown: InvocationTargetException) {
                val cause = thrown.targetException ?: thrown
                if (failure == null) failure = cause else failure.addSuppressed(cause)
            }
        }
        failure?.let { throw it }
        return defaultReturn(method.returnType)
    }

    private companion object {
        val EMPTY = emptyArray<Any?>()

        fun defaultReturn(type: Class<*>): Any? = when (type) {
            java.lang.Boolean.TYPE -> false
            java.lang.Integer.TYPE -> 0
            java.lang.Long.TYPE -> 0L
            else -> null
        }
    }
}
