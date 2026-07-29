package com.github.btakita.agentdoc

import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadFactory
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/**
 * One serialized worker lane per document replica.
 *
 * Replica operations are FIFO within a document, but an expensive attach or
 * normalization for one document must not delay unrelated open documents.
 * Scheduled executors retain their queue after an idle thread exits, so the
 * registry can preserve document affinity without retaining one OS thread for
 * every document ever opened by the project.
 */
internal class DocumentReplicaWorkers(
    private val idleThreadTimeoutMs: Long = DEFAULT_IDLE_THREAD_TIMEOUT_MS,
) {
    private val threadSequence = AtomicInteger(0)
    private val workers = ConcurrentHashMap<String, ScheduledThreadPoolExecutor>()

    fun forDocument(filePath: String): ScheduledExecutorService =
        workers.computeIfAbsent(filePath) { path ->
            ScheduledThreadPoolExecutor(1, threadFactory(path)).apply {
                removeOnCancelPolicy = true
                setKeepAliveTime(idleThreadTimeoutMs, TimeUnit.MILLISECONDS)
                allowCoreThreadTimeOut(true)
            }
        }

    fun shutdownNow() {
        workers.values.forEach(ScheduledThreadPoolExecutor::shutdownNow)
    }

    fun awaitTermination(timeoutMs: Long): Boolean {
        val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMs)
        for (worker in workers.values) {
            val remaining = deadline - System.nanoTime()
            if (remaining <= 0L) return worker.isTerminated
            try {
                if (!worker.awaitTermination(remaining, TimeUnit.NANOSECONDS)) return false
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                return false
            }
        }
        return true
    }

    internal fun laneCount(): Int = workers.size

    private fun threadFactory(filePath: String): ThreadFactory {
        val pathKey = filePath.hashCode().toUInt().toString(16)
        return ThreadFactory { runnable ->
            Thread(
                runnable,
                "agent-doc-crdt-replica-$pathKey-${threadSequence.incrementAndGet()}",
            ).apply {
                isDaemon = true
            }
        }
    }

    companion object {
        private const val DEFAULT_IDLE_THREAD_TIMEOUT_MS = 30_000L
    }
}
