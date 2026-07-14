package com.github.btakita.agentdoc

import io.github.lazily.Context
import io.github.lazily.IngressOutcome
import io.github.lazily.KeyedRelay
import io.github.lazily.MergePolicy
import io.github.lazily.Overflow

/**
 * Thread-safe keyed ingress around lazily's single-threaded [KeyedRelay].
 *
 * Each key owns one coalesced hot head. Producers can therefore outrun a slow
 * consumer without growing a FIFO: the merge algebra decides what survives,
 * and [drainOne] admits at most one key to the consumer per scheduling turn.
 */
internal class KeyedCoalescingRelay<K, T : Any>(mergePolicy: MergePolicy<T>) {
    private val context = Context()
    private val relay = KeyedRelay<K, T>(context, 1L, Overflow.Conflate, mergePolicy)
    private val readyKeys = LinkedHashSet<K>()

    @Synchronized
    fun ingress(key: K, value: T): IngressOutcome {
        val outcome = relay.ingress(key, value)
        readyKeys.add(key)
        return outcome
    }

    @Synchronized
    fun drainOne(): Pair<K, T>? {
        val iterator = readyKeys.iterator()
        if (!iterator.hasNext()) return null
        val key = iterator.next()
        iterator.remove()
        val value = relay.drain(key) ?: return null
        return key to value
    }

    @Synchronized
    fun hasPending(): Boolean = readyKeys.isNotEmpty()

    @Synchronized
    fun pendingKeyCount(): Int = readyKeys.size

    @Synchronized
    fun clear() {
        for (key in readyKeys.toList()) relay.drain(key)
        readyKeys.clear()
    }
}
