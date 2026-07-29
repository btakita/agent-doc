package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class DocumentReplicaWorkersTest {
    @Test
    fun `blocked document does not delay another replica and each lane stays fifo`() {
        val workers = DocumentReplicaWorkers(idleThreadTimeoutMs = 50L)
        val firstStarted = CountDownLatch(1)
        val releaseFirst = CountDownLatch(1)
        val secondDocumentFinished = CountDownLatch(1)
        val firstDocumentOrder = CopyOnWriteArrayList<Int>()

        try {
            workers.forDocument("/project/large.md").execute {
                firstDocumentOrder += 1
                firstStarted.countDown()
                assertTrue(releaseFirst.await(2, TimeUnit.SECONDS))
            }
            workers.forDocument("/project/large.md").execute {
                firstDocumentOrder += 2
            }

            assertTrue(firstStarted.await(1, TimeUnit.SECONDS))
            workers.forDocument("/project/independent.md").execute {
                secondDocumentFinished.countDown()
            }

            assertTrue(
                "an unrelated document lane must run while the large document is blocked",
                secondDocumentFinished.await(1, TimeUnit.SECONDS),
            )
            assertEquals(listOf(1), firstDocumentOrder.toList())

            releaseFirst.countDown()
            workers.forDocument("/project/large.md").submit<Unit> {}.get(1, TimeUnit.SECONDS)
            assertEquals(listOf(1, 2), firstDocumentOrder.toList())
            assertEquals(2, workers.laneCount())
        } finally {
            releaseFirst.countDown()
            workers.shutdownNow()
            assertTrue(workers.awaitTermination(2_000L))
        }
    }
}
