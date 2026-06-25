package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Unit tests for the `#crdtauth5` editor-as-replica forwarding seam.
 *
 * These prove the THIN-plugin contract without loading the native library or a
 * real socket: a local `Document` delta flows replica.applyLocal → encodeState →
 * transport.broadcastUpdate, and an inbound remote update flows
 * transport → applyRemoteUpdate → replica.applyUpdate → converged text. The real
 * yrs convergence + fan-out is covered in Rust; here the [ReplicaNode] is a
 * deterministic in-memory fake so the SEAM wiring is provable in Kotlin.
 */
class CrdtReplicaForwarderTest {

    /**
     * Deterministic in-memory stand-in for the FFI yrs replica. It is NOT a CRDT
     * (the real one lives in Rust) — it just accumulates inserts so the seam's
     * apply/encode/apply-back plumbing is observable. "encodeState" returns the
     * full text bytes; "applyUpdate" appends the bytes as text (enough to assert
     * a remote op landed).
     */
    private class FakeNode : ReplicaNode {
        var opened = false
        var openedWith: ByteArray? = null
        val buffer = StringBuilder()
        var closed = false

        override fun open(clientId: Long, initState: ByteArray?): Boolean {
            opened = true
            openedWith = initState
            if (initState != null) buffer.append(String(initState))
            return true
        }

        override fun applyLocal(clientId: Long, offset: Int, deleteLen: Int, insert: String): Boolean {
            val pos = offset.coerceIn(0, buffer.length)
            if (deleteLen > 0) buffer.delete(pos, (pos + deleteLen).coerceAtMost(buffer.length))
            buffer.insert(pos.coerceAtMost(buffer.length), insert)
            return true
        }

        override fun applyUpdate(clientId: Long, update: ByteArray): Boolean {
            buffer.append(String(update))
            return true
        }

        override fun encodeState(): ByteArray? = buffer.toString().toByteArray()

        override fun text(): String? = buffer.toString()

        override fun close(clientId: Long) {
            closed = true
        }
    }

    private class CapturingTransport(
        private val refuseRegister: Boolean = false,
        private val clientId: Long = 42L,
        private val bootstrap: ByteArray? = null,
    ) : ReplicaTransport {
        var registered = false
        var deregistered = false
        val sentUpdates = mutableListOf<ByteArray>()

        override fun register(filePath: String, identity: String): ReplicaRegisterAck? {
            if (refuseRegister) return null
            registered = true
            return ReplicaRegisterAck(clientId, bootstrap)
        }

        override fun broadcastUpdate(filePath: String, identity: String, update: ByteArray) {
            sentUpdates.add(update)
        }

        override fun deregister(filePath: String, identity: String) {
            deregistered = true
        }
    }

    @Test
    fun `register opens the replica from the canonical bootstrap`() {
        val node = FakeNode()
        val transport = CapturingTransport(bootstrap = "BASE".toByteArray())
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:1", node, transport)

        assertTrue(fwd.register())
        assertTrue(fwd.attached)
        assertTrue(transport.registered)
        assertTrue(node.opened)
        assertEquals("BASE", String(node.openedWith!!))
    }

    @Test
    fun `local delta forwards through apply-local and encode to the transport`() {
        val node = FakeNode()
        val transport = CapturingTransport()
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:1", node, transport)
        fwd.register()

        fwd.forwardLocalDelta(offset = 0, deleteLen = 0, insert = "FROM-A")

        // The delta reached the local replica AND was shipped to the hub.
        assertEquals("FROM-A", node.text())
        assertEquals(1, transport.sentUpdates.size)
        assertEquals("FROM-A", String(transport.sentUpdates[0]))
    }

    @Test
    fun `remote update applies back into the replica and returns converged text`() {
        val node = FakeNode()
        val transport = CapturingTransport()
        val fwd = CrdtReplicaForwarder("plan.md", "vscode:2", node, transport)
        fwd.register()

        val converged = fwd.applyRemoteUpdate("FROM-PEER".toByteArray())
        assertEquals("FROM-PEER", converged)
        assertEquals("FROM-PEER", node.text())
    }

    @Test
    fun `a refused register leaves the forwarder detached and no-ops local deltas`() {
        // The Detached / headless path: the supervisor refuses register, so the
        // plugin must fall back (attached=false) and never ship deltas.
        val node = FakeNode()
        val transport = CapturingTransport(refuseRegister = true)
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:1", node, transport)

        assertFalse(fwd.register())
        assertFalse(fwd.attached)
        assertFalse(node.opened)

        fwd.forwardLocalDelta(0, 0, "ignored")
        assertTrue(transport.sentUpdates.isEmpty())
        assertNull(fwd.applyRemoteUpdate("x".toByteArray()))
    }

    @Test
    fun `two forwarders converge through a relaying transport (seam-level fan-out)`() {
        // Model the supervisor hub fan-out at the seam level: editor A's broadcast
        // is delivered to editor B's applyRemoteUpdate (the Rust hub does the real
        // delta math; this proves the plugin-side ingress/egress wiring).
        val nodeA = FakeNode()
        val nodeB = FakeNode()
        lateinit var fwdB: CrdtReplicaForwarder

        val relayToB = object : ReplicaTransport {
            override fun register(filePath: String, identity: String) = ReplicaRegisterAck(1L, null)
            override fun broadcastUpdate(filePath: String, identity: String, update: ByteArray) {
                // Hub fan-out → deliver A's op into B.
                fwdB.applyRemoteUpdate(update)
            }
            override fun deregister(filePath: String, identity: String) {}
        }
        val transportB = CapturingTransport(clientId = 2L)

        val fwdA = CrdtReplicaForwarder("plan.md", "intellij:A", nodeA, relayToB)
        fwdB = CrdtReplicaForwarder("plan.md", "vscode:B", nodeB, transportB)
        assertTrue(fwdA.register())
        assertTrue(fwdB.register())

        fwdA.forwardLocalDelta(0, 0, "HELLO")
        assertEquals("HELLO", nodeB.text())
    }

    @Test
    fun `deregister tears down the replica and notifies the hub`() {
        val node = FakeNode()
        val transport = CapturingTransport()
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:1", node, transport)
        fwd.register()

        fwd.deregister()
        assertFalse(fwd.attached)
        assertTrue(transport.deregistered)
        assertTrue(node.closed)
    }
}
