package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CrdtReplicaReregisterCoalescingTest {
    @Test
    fun `ack recovery replaces a replica once per bounded interval`() {
        assertTrue(ackRecoveryReregisterDueUtil(null, nowMs = 10_000, minIntervalMs = 5_000))
        assertFalse(
            ackRecoveryReregisterDueUtil(
                lastStartedMs = 10_000,
                nowMs = 10_500,
                minIntervalMs = 5_000,
            ),
        )
        assertTrue(
            ackRecoveryReregisterDueUtil(
                lastStartedMs = 10_000,
                nowMs = 15_000,
                minIntervalMs = 5_000,
            ),
        )
    }

    @Test
    fun `clock rollback permits recovery instead of suppressing forever`() {
        assertTrue(
            ackRecoveryReregisterDueUtil(
                lastStartedMs = 10_000,
                nowMs = 9_000,
                minIntervalMs = 5_000,
            ),
        )
    }
}
