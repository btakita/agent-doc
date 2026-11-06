package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path

/**
 * `#divergentapplyreregister`: a delivery the native replica refuses as a
 * divergent CRDT history (FFI -4, `#replicapositiondrift`) re-registers the
 * replica from the buffer immediately, at most once per guard window.
 */
class DivergentHistoryReregisterTest {

    @Test
    fun `the FFI apply return code keeps the divergent-history refusal apart`() {
        assertEquals(ReplicaApplyOutcome.Applied, replicaApplyOutcomeUtil(0))
        assertEquals(ReplicaApplyOutcome.DivergentHistory, replicaApplyOutcomeUtil(-4))
        assertEquals(ReplicaApplyOutcome.Failed, replicaApplyOutcomeUtil(-2))
        assertEquals(ReplicaApplyOutcome.Failed, replicaApplyOutcomeUtil(-3))
        assertEquals(ReplicaApplyOutcome.Failed, replicaApplyOutcomeUtil(-99))
    }

    @Test
    fun `a node without outcome support reports every refusal as a plain failure`() {
        val node = OutcomeNode(ReplicaApplyOutcome.Applied)
        val legacy = object : ReplicaNode by node {
            override fun applyUpdate(clientId: Long, update: ByteArray): Boolean = false
            override fun applyUpdateOutcome(clientId: Long, update: ByteArray): ReplicaApplyOutcome =
                super.applyUpdateOutcome(clientId, update)
        }
        assertEquals(ReplicaApplyOutcome.Failed, legacy.applyUpdateOutcome(1L, byteArrayOf(1)))
    }

    @Test
    fun `a divergent refusal marks the replica and withholds its resume state`() {
        val node = OutcomeNode(ReplicaApplyOutcome.DivergentHistory)
        val forwarder = CrdtReplicaForwarder("plan.md", "intellij:1", node, AckTransport())
        assertTrue(forwarder.register())
        assertNotNull("an attached, healthy replica offers its resume state", forwarder.captureResumeState())

        assertNull(forwarder.applyRemoteUpdate("cross-history".toByteArray()))

        assertTrue(forwarder.historyDivergent)
        assertNull(
            "a divergent replica's frontier names retired op ids and must never seed a resume",
            forwarder.captureResumeState(),
        )
        assertEquals("nothing was applied", "base", forwarder.replicaText())
    }

    @Test
    fun `an ordinary apply failure does not mark the replica divergent`() {
        val node = OutcomeNode(ReplicaApplyOutcome.Failed)
        val forwarder = CrdtReplicaForwarder("plan.md", "intellij:1", node, AckTransport())
        assertTrue(forwarder.register())

        assertNull(forwarder.applyRemoteUpdate("bad".toByteArray()))

        assertFalse(forwarder.historyDivergent)
        assertNotNull(forwarder.captureResumeState())
    }

    @Test
    fun `an applied update converges and leaves the replica in its history`() {
        val node = OutcomeNode(ReplicaApplyOutcome.Applied)
        val forwarder = CrdtReplicaForwarder("plan.md", "intellij:1", node, AckTransport())
        assertTrue(forwarder.register())

        assertEquals("base+peer", forwarder.applyRemoteUpdate("+peer".toByteArray()))
        assertFalse(forwarder.historyDivergent)
    }

    @Test
    fun `the re-register loop guard allows one attempt per window`() {
        assertTrue(divergentHistoryReregisterDueUtil(nowMs = 100_000, lastAttemptMs = null))
        assertFalse(divergentHistoryReregisterDueUtil(nowMs = 100_000, lastAttemptMs = 100_000))
        assertFalse(
            divergentHistoryReregisterDueUtil(
                nowMs = 100_000 + DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS - 1,
                lastAttemptMs = 100_000,
            ),
        )
        assertTrue(
            divergentHistoryReregisterDueUtil(
                nowMs = 100_000 + DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS,
                lastAttemptMs = 100_000,
            ),
        )
    }

    @Test
    fun `a clock rollback permits the re-register instead of suppressing it`() {
        assertTrue(divergentHistoryReregisterDueUtil(nowMs = 90_000, lastAttemptMs = 100_000))
    }

    @Test
    fun `the remote drain re-registers a divergent replica before projecting it`() {
        val source =
            Files.readString(Path.of("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"))
        val drain =
            source
                .substringAfter("private fun drainRemoteUpdatesFor(")
                .substringBefore("private fun applyReplaceDelivery(")
        val divergentCheck = drain.indexOf("if (forwarder.historyDivergent)")
        assertTrue("the drain must check the divergence mark", divergentCheck >= 0)
        assertTrue(
            "the divergence check must precede the editor projection",
            divergentCheck < drain.indexOf("queueRemoteTextApply(\n"),
        )
        assertTrue(drain.contains("reregisterAfterDivergentHistory(filePath, forwarder, expectedText)"))

        val reregister =
            source
                .substringAfter("private fun reregisterAfterDivergentHistory(")
                .substringBefore("private fun shouldAttemptRegister(")
        assertTrue(reregister.contains("divergentHistoryReregisterDueUtil(now, lastAttemptMs)"))
        assertTrue(reregister.contains("bootstrapFromControllerCanonical = true"))
        assertTrue(reregister.contains("expectedEditorTextAtSwap = editorText"))
        assertTrue(
            "the guard window is stamped only after the cheap buffer fence",
            reregister.indexOf("editorBufferText(filePath) != editorText") <
                reregister.indexOf("divergentHistoryReregisterAtMs[filePath] = now"),
        )
    }

    /** A replica node whose remote apply returns a fixed outcome. */
    private class OutcomeNode(private val outcome: ReplicaApplyOutcome) : ReplicaNode {
        private val buffer = StringBuilder()

        override fun open(clientId: Long, initState: ByteArray?): Boolean {
            buffer.append("base")
            return true
        }

        override fun applyLocal(clientId: Long, offset: Int, deleteLen: Int, insert: String): Boolean = true

        override fun applyUpdate(clientId: Long, update: ByteArray): Boolean =
            applyUpdateOutcome(clientId, update) == ReplicaApplyOutcome.Applied

        override fun applyUpdateOutcome(clientId: Long, update: ByteArray): ReplicaApplyOutcome {
            if (outcome == ReplicaApplyOutcome.Applied) buffer.append(String(update))
            return outcome
        }

        override fun encodeState(): ByteArray? = buffer.toString().toByteArray()

        override fun stateVector(): ByteArray? = "sv:$buffer".toByteArray()

        override fun text(): String? = buffer.toString()

        override fun close(clientId: Long) = Unit
    }

    private class AckTransport : ReplicaTransport {
        override fun register(filePath: String, identity: String): ReplicaRegisterAck? =
            ReplicaRegisterAck(clientId = 7L, bootstrap = null, lineage = "lineage-test")

        override fun broadcastUpdate(filePath: String, identity: String, update: ByteArray) = Unit

        override fun pushDocumentOps(filePath: String, lineage: String?, deltaJson: String): Boolean = true

        override fun pullUpdates(filePath: String, identity: String): List<ReplicaRemoteUpdate> = emptyList()

        override fun deregister(filePath: String, identity: String) = Unit
    }
}
