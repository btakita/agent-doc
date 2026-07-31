package com.github.btakita.agentdoc

import io.github.lazily.ThreadSafeContext
import java.nio.file.Files
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
    private class FakeNode(private val openSucceeds: Boolean = true) : ReplicaNode {
        var opened = false
        var openedWith: ByteArray? = null
        val buffer = StringBuilder()
        var closed = false
        var textReads = 0

        override fun open(clientId: Long, initState: ByteArray?): Boolean {
            opened = true
            openedWith = initState
            if (initState != null) buffer.append(String(initState))
            return openSucceeds
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

        override fun stateVector(): ByteArray? = "sv:${buffer}".toByteArray()

        override fun text(): String? {
            textReads++
            return buffer.toString()
        }

        override fun close(clientId: Long) {
            closed = true
        }
    }

    private class CapturingTransport(
        private val refuseRegister: Boolean = false,
        private val clientId: Long = 42L,
        private val bootstrap: ByteArray? = null,
        private val lineage: String? = "lineage-test",
        private val bootstrapKind: ReplicaBootstrapKind = ReplicaBootstrapKind.Full,
        private val canonicalStateVector: ByteArray? = null,
    ) : ReplicaTransport {
        var registered = false
        var registeredStateVector: ByteArray? = null
        var deregistered = false
        val sentUpdates = mutableListOf<ByteArray>()
        val sentLineages = mutableListOf<String?>()
        val pendingUpdates = mutableListOf<ReplicaRemoteUpdate>()
        val ackedUpdates = mutableListOf<String>()
        val ackedContentHashes = mutableListOf<String?>()
        val ackedStamps = mutableListOf<ReplicaAckStamps>()

        override fun register(filePath: String, identity: String): ReplicaRegisterAck? {
            return register(filePath, identity, null)
        }

        override fun register(
            filePath: String,
            identity: String,
            stateVector: ByteArray?,
        ): ReplicaRegisterAck? {
            if (refuseRegister) return null
            registered = true
            registeredStateVector = stateVector
            return ReplicaRegisterAck(
                clientId = clientId,
                bootstrap = bootstrap,
                lineage = lineage,
                bootstrapKind = bootstrapKind,
                canonicalStateVector = canonicalStateVector,
            )
        }

        override fun broadcastUpdate(filePath: String, identity: String, update: ByteArray) {
            sentUpdates.add(update)
        }

        override fun pushDocumentOps(filePath: String, lineage: String?, deltaJson: String): Boolean {
            sentLineages.add(lineage)
            return true
        }

        override fun pullUpdates(filePath: String, identity: String): List<ReplicaRemoteUpdate> =
            pendingUpdates.toList()

        override fun ackUpdate(
            filePath: String,
            identity: String,
            patchId: String,
            generation: Long,
            contentHash: String?,
            stamps: ReplicaAckStamps,
        ): Boolean {
            ackedUpdates.add("$patchId:$generation")
            ackedContentHashes.add(contentHash)
            ackedStamps.add(stamps)
            pendingUpdates.removeIf { it.patchId == patchId && it.generation == generation }
            return true
        }

        override fun deregister(filePath: String, identity: String) {
            deregistered = true
        }
    }

    @Test
    fun `register opens the replica from the canonical bootstrap`() {
        val node = FakeNode()
        val transport = CapturingTransport(bootstrap = "BASE".toByteArray())
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:1", node, transport, ThreadSafeContext())

        assertTrue(fwd.register())
        assertTrue(fwd.attached)
        assertEquals(MergeOwnershipPhase.EditorOwnsBuffer, fwd.ownershipPhase)
        assertTrue(transport.registered)
        assertTrue(node.opened)
        assertEquals("BASE", String(node.openedWith!!))
    }

    @Test
    fun `replacement register opens retained state and applies only canonical delta`() {
        val node = FakeNode()
        val retained =
            ReplicaResumeState(
                encodedState = "LOCAL".toByteArray(),
                stateVector = "LOCAL-SV".toByteArray(),
            )
        val transport =
            CapturingTransport(
                bootstrap = "-CANONICAL-DELTA".toByteArray(),
                bootstrapKind = ReplicaBootstrapKind.Delta,
                canonicalStateVector = "CANONICAL-SV".toByteArray(),
            )
        val fwd =
            CrdtReplicaForwarder(
                "plan.md",
                "intellij:refresh-1",
                node,
                transport,
                resumeState = retained,
            )

        assertTrue(fwd.register())
        assertEquals("LOCAL", String(node.openedWith!!))
        assertEquals("LOCAL-CANONICAL-DELTA", node.text())
        assertEquals("LOCAL-SV", String(transport.registeredStateVector!!))
        assertEquals(
            "resume registration must publish any local suffix from the canonical frontier",
            1,
            transport.sentUpdates.size,
        )
    }

    @Test
    fun `local retirement closes native node without deregistering editor authority`() {
        val node = FakeNode()
        val transport = CapturingTransport()
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:refresh-1", node, transport)

        assertTrue(fwd.register())
        fwd.retireLocal()

        assertTrue(node.closed)
        assertFalse(fwd.attached)
        assertEquals(MergeOwnershipPhase.Detached, fwd.ownershipPhase)
        assertFalse(transport.deregistered)
    }

    @Test
    fun `failed native bootstrap rolls back only the new hub membership`() {
        val node = FakeNode(openSucceeds = false)
        val transport = CapturingTransport(bootstrap = "CANONICAL".toByteArray())
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:refresh-2", node, transport)

        assertFalse(fwd.register())

        assertFalse(fwd.attached)
        assertTrue(transport.registered)
        assertTrue(transport.deregistered)
        assertFalse(node.closed)
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
        assertEquals(listOf("lineage-test"), transport.sentLineages)
    }

    @Test
    fun `new replica can be aligned to the live editor buffer before first delta`() {
        val node = FakeNode()
        val transport = CapturingTransport(bootstrap = "DISK".toByteArray())
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:1", node, transport)
        fwd.register()

        fwd.ensureEditorText("BUFFER")

        assertEquals("BUFFER", node.text())
        assertEquals(1, transport.sentUpdates.size)
        assertEquals("BUFFER", String(transport.sentUpdates[0]))
    }

    @Test
    fun `large document typing uses the actor projection instead of reading native text per delta`() {
        val base = "x".repeat(100_000)
        val node = FakeNode()
        val transport = CapturingTransport(bootstrap = base.toByteArray())
        val fwd = CrdtReplicaForwarder("large.md", "intellij:1", node, transport)

        assertTrue(fwd.register())
        assertEquals("registration establishes the one native text observation", 1, node.textReads)

        // Aligning an already-equal editor cut canonicalizes the manager's
        // String instance without another FFI materialization.
        fwd.ensureEditorText(base)
        assertEquals(1, node.textReads)

        var current = base
        repeat(8) {
            val next = "$current!"
            fwd.forwardLocalDelta(
                offset = current.length,
                deleteLen = 0,
                insert = "!",
                resultingText = next,
            )
            assertTrue(shouldForwardLocalDeltaUtil(fwd.replicaText(), next))
            current = next
        }

        assertEquals(
            "ordinary keystrokes must not materialize the 100K native document",
            1,
            node.textReads,
        )
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
    fun `remote pull applies only after caller acks the applied update`() {
        val node = FakeNode()
        val transport = CapturingTransport()
        transport.pendingUpdates.add(
            ReplicaRemoteUpdate(
                patchId = "crdt:1:2:1",
                origin = 1L,
                target = 2L,
                generation = 1L,
                update = "FROM-PEER".toByteArray(),
            ),
        )
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:2", node, transport)
        fwd.register()

        val pulled = fwd.pullRemoteUpdates()
        assertEquals(1, pulled.size)
        assertEquals("FROM-PEER", fwd.applyRemoteUpdate(pulled[0].update))
        assertTrue(fwd.ackRemoteUpdate(pulled[0], "FROM-PEER"))

        assertEquals(listOf("crdt:1:2:1:1"), transport.ackedUpdates)
        assertEquals(
            listOf("a6c6f01a3d023a48fd52677f25b60502ea1c596e76c6e5ae91b5216d4d035841"),
            transport.ackedContentHashes,
        )
        assertTrue(transport.pendingUpdates.isEmpty())
    }

    @Test
    fun `the ack carries the editor-side stamps the binary cannot observe`() {
        // #ackeditorstamps: the binary can time its own wait and the moment the
        // receipt lands, but "received" and "applied to buffer" happen inside the
        // plugin. If the ACK does not carry them, an 11s delivery_ack_pending
        // stays one opaque number instead of three legs with different fixes.
        val node = FakeNode()
        val transport = CapturingTransport()
        val pulledAtMs = 1_700_000_000_000L
        transport.pendingUpdates.add(
            ReplicaRemoteUpdate(
                patchId = "crdt:1:2:1",
                origin = 1L,
                target = 2L,
                generation = 1L,
                update = "FROM-PEER".toByteArray(),
                pulledAtMs = pulledAtMs,
            ),
        )
        val fwd = CrdtReplicaForwarder("plan.md", "intellij:2", node, transport)
        fwd.register()

        val pulled = fwd.pullRemoteUpdates()
        assertEquals("FROM-PEER", fwd.applyRemoteUpdate(pulled[0].update))
        val appliedAtMs = pulledAtMs + 400L
        assertTrue(fwd.ackRemoteUpdate(pulled[0], "FROM-PEER", appliedAtMs))

        assertEquals(
            "both editor-side moments must reach the transport, not just the applied one",
            listOf(ReplicaAckStamps(pulledAtMs = pulledAtMs, appliedAtMs = appliedAtMs)),
            transport.ackedStamps,
        )

        // A caller that cannot name the apply moment (self-echo: the update never
        // crossed the buffer) must leave it UNSTAMPED rather than substitute a
        // clock read, or the profile reports a fabricated apply of ~0ms.
        transport.pendingUpdates.add(
            ReplicaRemoteUpdate(
                patchId = "crdt:1:2:2",
                origin = 1L,
                target = 2L,
                generation = 2L,
                update = "SELF-ECHO".toByteArray(),
                pulledAtMs = pulledAtMs,
            ),
        )
        val echo = fwd.pullRemoteUpdates().last()
        assertTrue(fwd.ackRemoteUpdate(echo, "FROM-PEER"))
        assertEquals(0L, transport.ackedStamps.last().appliedAtMs)
        assertEquals(pulledAtMs, transport.ackedStamps.last().pulledAtMs)
    }

    @Test
    fun `remote update decision applies peer edits but suppresses self echo`() {
        val peerUpdate = ReplicaRemoteUpdate(
            patchId = "crdt:1:42:1",
            origin = 1L,
            target = 42L,
            generation = 1L,
            update = "peer".toByteArray(),
        )
        val selfEcho = peerUpdate.copy(
            patchId = "crdt:42:42:2",
            origin = 42L,
            target = 42L,
            generation = 2L,
        )

        assertTrue(shouldApplyRemoteCrdtUpdateUtil(peerUpdate, clientId = 42L))
        assertFalse(shouldApplyRemoteCrdtUpdateUtil(selfEcho, clientId = 42L))
    }

    @Test
    fun `remote apply boundary rejects stale targets after editor text advances`() {
        assertTrue(remoteCrdtApplyStillCurrentUtil("base", "base", "base remote"))
        assertTrue(remoteCrdtApplyStillCurrentUtil("base", "base remote", "base remote"))
        assertFalse(remoteCrdtApplyStillCurrentUtil("base", "base typed", "base remote"))
    }

    @Test
    fun `remote apply persists only across an exact disk baseline or target`() {
        assertTrue(remoteCrdtDiskCanPersistUtil("base", "base remote", "base"))
        assertTrue(remoteCrdtDiskCanPersistUtil("base", "base remote", "base remote"))
        assertFalse(remoteCrdtDiskCanPersistUtil("base", "base remote", "external edit"))
        assertFalse(remoteCrdtDiskCanPersistUtil("base", "base remote", null))
    }

    @Test
    fun `failed remote persistence rolls back only when both exact planes prove it safe`() {
        assertEquals(
            RemotePersistReconciliation.Persisted,
            remotePersistReconciliationUtil(
                beforeText = "base",
                targetText = "remote",
                editorAfterSave = "remote",
                diskAfterSave = "remote",
            ),
        )
        assertEquals(
            RemotePersistReconciliation.RollbackToBefore,
            remotePersistReconciliationUtil(
                beforeText = "base",
                targetText = "remote",
                editorAfterSave = "remote",
                diskAfterSave = "base",
            ),
        )
        assertEquals(
            RemotePersistReconciliation.PreserveAdvancedEditor,
            remotePersistReconciliationUtil(
                beforeText = "base",
                targetText = "remote",
                editorAfterSave = "operator advanced",
                diskAfterSave = "base",
            ),
        )
        assertEquals(
            RemotePersistReconciliation.PreserveAdvancedEditor,
            remotePersistReconciliationUtil(
                beforeText = "base",
                targetText = "remote",
                editorAfterSave = "remote",
                diskAfterSave = "external advanced",
            ),
        )
    }

    @Test
    fun `unsaved editor projects remote deltas in memory without disk side effects`() {
        assertEquals(
            RemoteCrdtProjectionMode.MemoryOnly,
            remoteCrdtProjectionModeUtil(documentUnsaved = true, diskCanPersist = false),
        )
        assertEquals(
            RemoteCrdtProjectionMode.MemoryOnly,
            remoteCrdtProjectionModeUtil(documentUnsaved = true, diskCanPersist = true),
        )
        assertEquals(
            RemoteCrdtProjectionMode.Persist,
            remoteCrdtProjectionModeUtil(documentUnsaved = false, diskCanPersist = true),
        )
        assertEquals(
            RemoteCrdtProjectionMode.Reject,
            remoteCrdtProjectionModeUtil(documentUnsaved = false, diskCanPersist = false),
        )
    }

    @Test
    fun `replace delivery boundary requires editor buffer and replica to match the expected baseline`() {
        assertTrue(remoteCrdtReplaceStillCurrentUtil("base", "base", "base"))
        assertFalse(remoteCrdtReplaceStillCurrentUtil("base", "base typed", "base"))
        assertFalse(remoteCrdtReplaceStillCurrentUtil("base", "base", "base typed"))
        assertFalse(remoteCrdtReplaceStillCurrentUtil("base", "base", null))
    }

    @Test
    fun `only incremental user events may originate editor deltas`() {
        assertTrue(
            isOperatorDocumentEventUtil(
                nonOperatorMutation = false,
                wholeTextReplaced = false,
            ),
        )
        assertFalse(
            isOperatorDocumentEventUtil(
                nonOperatorMutation = true,
                wholeTextReplaced = false,
            ),
        )
        assertFalse(
            isOperatorDocumentEventUtil(
                nonOperatorMutation = false,
                wholeTextReplaced = true,
            ),
        )
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
        // Model the CP hub fan-out at the seam level: editor A's broadcast
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
    fun `socket loss is distinguishable from an idle controller pull`() {
        val projectRoot = Files.createTempDirectory("agent-doc-missing-controller").toFile()
        try {
            val transport =
                CpSocketReplicaTransport(
                    projectRoot = projectRoot.absolutePath,
                    flushRetainedOpsBeforePull = false,
                )

            val delivery = transport.pullDelivery("plan.md", "intellij:lost-controller")

            assertTrue(delivery is ReplicaPullDelivery.Unavailable)
            assertTrue((delivery as ReplicaPullDelivery.Unavailable).reason.contains("controller.sock"))
        } finally {
            projectRoot.deleteRecursively()
        }
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
