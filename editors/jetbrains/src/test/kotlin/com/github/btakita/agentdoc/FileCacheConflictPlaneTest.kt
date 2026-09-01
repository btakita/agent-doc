package com.github.btakita.agentdoc

import io.github.lazily.ThreadSafeContext
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FileCacheConflictPlaneTest {
    @Test
    fun `pending conflict blocks once without manufacturing retry edges`() {
        val plane = FileCacheConflictPlane(ThreadSafeContext())

        val first = plane.observe("payments-ledger.md", pending = true, diskWitness = 41)
        val repeated = plane.observe("payments-ledger.md", pending = true, diskWitness = 41)

        assertTrue(first.deferMutation)
        assertTrue(first.newlyPendingEdge)
        assertTrue(repeated.deferMutation)
        assertFalse(repeated.newlyPendingEdge)
    }

    @Test
    fun `cleared conflict rearms the next pending edge`() {
        val plane = FileCacheConflictPlane(ThreadSafeContext())
        plane.observe("payments-ledger.md", pending = true, diskWitness = 41)

        val cleared = plane.observe("payments-ledger.md", pending = false, diskWitness = 41)
        val next = plane.observe("payments-ledger.md", pending = true, diskWitness = 42)

        assertFalse(cleared.deferMutation)
        assertTrue(next.deferMutation)
        assertTrue(next.newlyPendingEdge)
    }
}
