package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class NativeCallWorkerTest {
    @Test
    fun `queued timeout retains the native generation for a later call`() {
        val workerThreads = ConcurrentHashMap.newKeySet<Thread>()
        val executor = newNativeGenerationExecutor(workerThreads, workerCount = 1)
        val blockerStarted = CountDownLatch(1)
        val releaseBlocker = CountDownLatch(1)
        val poisoned = AtomicBoolean(false)

        try {
            executor.execute {
                blockerStarted.countDown()
                releaseBlocker.await(2, TimeUnit.SECONDS)
            }
            assertTrue(blockerStarted.await(1, TimeUnit.SECONDS))

            try {
                callOnNativeWorker(
                    executor = executor,
                    workerThreads = workerThreads,
                    timeoutMs = 25L,
                    onRunningTimeout = {
                        poisoned.set(true)
                        error("queued work must not poison the generation")
                    },
                ) {
                    "never-runs"
                }
                fail("queued call should time out")
            } catch (_: NativeCallQueueTimeoutException) {
                // Expected: the retained generation remains usable.
            }
            assertFalse(poisoned.get())

            releaseBlocker.countDown()
            assertEquals(
                "still-live",
                callOnNativeWorker(
                    executor = executor,
                    workerThreads = workerThreads,
                    timeoutMs = 1_000L,
                    onRunningTimeout = { error("later call unexpectedly wedged") },
                ) {
                    "still-live"
                },
            )
        } finally {
            releaseBlocker.countDown()
            executor.shutdownNow()
            assertTrue(executor.awaitTermination(2, TimeUnit.SECONDS))
        }
    }

    @Test
    fun `bounded native pool lets an unrelated call bypass a blocked document`() {
        val workerThreads = ConcurrentHashMap.newKeySet<Thread>()
        val executor = newNativeGenerationExecutor(workerThreads, workerCount = 2)
        val firstStarted = CountDownLatch(1)
        val releaseFirst = CountDownLatch(1)
        val firstFinished = CountDownLatch(1)

        try {
            val caller = Thread {
                callOnNativeWorker(
                    executor = executor,
                    workerThreads = workerThreads,
                    timeoutMs = 2_000L,
                    onRunningTimeout = { error("blocked test call exceeded its lease") },
                ) {
                    firstStarted.countDown()
                    releaseFirst.await(1, TimeUnit.SECONDS)
                }
                firstFinished.countDown()
            }
            caller.start()
            assertTrue(firstStarted.await(1, TimeUnit.SECONDS))

            val value = callOnNativeWorker(
                executor = executor,
                workerThreads = workerThreads,
                timeoutMs = 500L,
                onRunningTimeout = { error("unrelated native call was head-of-line blocked") },
            ) {
                "monsterrodholders-ready"
            }
            assertEquals("monsterrodholders-ready", value)
            assertFalse(firstFinished.await(25, TimeUnit.MILLISECONDS))

            releaseFirst.countDown()
            caller.join(1_000L)
            assertTrue(firstFinished.await(1, TimeUnit.SECONDS))
        } finally {
            releaseFirst.countDown()
            executor.shutdownNow()
            assertTrue(executor.awaitTermination(2, TimeUnit.SECONDS))
        }
    }
}
