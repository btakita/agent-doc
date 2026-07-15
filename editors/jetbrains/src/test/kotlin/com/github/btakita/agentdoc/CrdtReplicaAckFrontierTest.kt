package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CrdtReplicaAckFrontierTest {
    @Test
    fun `one oldest ack carries proof for the newest matching visible frontier`() {
        val updates = listOf(
            update(generation = 3, expectedHash = "first"),
            update(generation = 4, expectedHash = "middle"),
            update(generation = 5, expectedHash = "visible"),
        )

        val plan = remoteAckReplayPlanUtil(updates, "visible")!!

        assertEquals(3L, plan.candidate.generation)
        assertEquals(5L, plan.acknowledgedThroughGeneration)
    }

    @Test
    fun `unpublished visible text retries only one oldest ack without claiming a prefix`() {
        val updates = listOf(
            update(generation = 11, expectedHash = "older"),
            update(generation = 12, expectedHash = "newer"),
        )

        val plan = remoteAckReplayPlanUtil(updates, "not-yet-published")!!

        assertEquals(11L, plan.candidate.generation)
        assertNull(plan.acknowledgedThroughGeneration)
    }

    private fun update(generation: Long, expectedHash: String) = ReplicaRemoteUpdate(
        patchId = "crdt:1:2:$generation",
        origin = 1L,
        target = 2L,
        generation = generation,
        expectedContentHash = expectedHash,
        update = byteArrayOf(generation.toByte()),
    )
}
