package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CrdtReplicaReregisterCoalescingTest {
    @Test
    fun `ack recovery replaces a replica once per bounded interval`() {
        assertTrue(projectionRecoveryReregisterDueUtil(null, nowMs = 10_000, minIntervalMs = 5_000))
        assertFalse(
            projectionRecoveryReregisterDueUtil(
                lastStartedMs = 10_000,
                nowMs = 10_500,
                minIntervalMs = 5_000,
            ),
        )
        assertTrue(
            projectionRecoveryReregisterDueUtil(
                lastStartedMs = 10_000,
                nowMs = 15_000,
                minIntervalMs = 5_000,
            ),
        )
    }

    @Test
    fun `clock rollback permits recovery instead of suppressing forever`() {
        assertTrue(
            projectionRecoveryReregisterDueUtil(
                lastStartedMs = 10_000,
                nowMs = 9_000,
                minIntervalMs = 5_000,
            ),
        )
    }

    @Test
    fun `every native replica incarnation receives a plugin lifetime generation`() {
        val filePath = "/work/tasks/fpe.md"
        val first = EditorIdentity.nextReplicaConnectionIdentity(filePath)
        val second = EditorIdentity.nextReplicaConnectionIdentity(filePath)
        val prefix = "${EditorIdentity.id}:$filePath:refresh-"

        assertNotEquals(
            "a replacement manager must not reuse the retiring manager's client identity",
            first,
            second,
        )
        assertTrue(first.startsWith(prefix))
        assertTrue(second.startsWith(prefix))
        assertTrue(first.removePrefix(prefix).all(Char::isDigit))
        assertTrue(second.removePrefix(prefix).all(Char::isDigit))
    }
}
