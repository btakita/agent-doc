package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationKind
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendPresentationReceiptOutcome
import com.github.btakita.agentdoc.split.FrontendSurfaceIdentity
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.platform.project.ProjectId
import fleet.util.UID
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking

class AuthenticatedSurfaceIngressStateTest {
    private val projectId = ProjectId(UID.random())

    @Test
    fun twoClientsAreIndependentAndReconnectFencesOldLease() = runBlocking {
        val publisher = RecordingPublisher()
        val state = AuthenticatedSurfaceIngressState(publisher)
        val clientA1 = state.open("client-a", "frontend-a-1")
        val clientB = state.open("client-b", "frontend-b-1")

        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            state.publish("client-a", clientA1, snapshot("frontend-a-1", 1)).status,
        )
        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            state.publish("client-b", clientB, snapshot("frontend-b-1", 1)).status,
        )

        val clientA2 = state.open("client-a", "frontend-a-2")
        assertTrue(clientA2.connectionGeneration > clientA1.connectionGeneration)
        assertEquals(
            SurfaceIngressStatus.STALE_LEASE,
            state.publish("client-a", clientA1, snapshot("frontend-a-1", 2)).status,
        )
        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            state.publish("client-a", clientA2, snapshot("frontend-a-2", 1)).status,
        )

        assertTrue("client-a" to clientA1.connectionGeneration in publisher.retired)
        assertEquals(3, publisher.published.size)
    }

    /**
     * `#viewsnapshotgenseedmac`: a restarted IDE recreates this state with a fresh generation
     * counter. Seeded from wall-clock time, the successor's first lease is above every generation
     * the predecessor issued, so the controller's `(connection_generation, sequence)` fence accepts
     * the restart as a newer revision without a peer pid (macOS sockets report none).
     */
    @Test
    fun restartedBackendGenerationStartsAboveItsPredecessor() = runBlocking {
        val predecessor = AuthenticatedSurfaceIngressState(RecordingPublisher(), generationSeed = 1_000L)
        val first = predecessor.open("client-a", "frontend-a-1")
        assertEquals(1_001L, first.connectionGeneration)
        val reconnects = (1..50).map { predecessor.open("client-a", "frontend-a-${it + 1}") }
        val highWater = reconnects.last().connectionGeneration
        assertEquals(1_051L, highWater)

        // The successor process started later on the wall clock; its seed is a later instant.
        val successorPublisher = RecordingPublisher()
        val successor = AuthenticatedSurfaceIngressState(successorPublisher, generationSeed = 60_000L)
        val restarted = successor.open("client-a", "frontend-a-restarted")
        assertTrue(restarted.connectionGeneration > highWater)
        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            successor.publish("client-a", restarted, snapshot("frontend-a-restarted", 1)).status,
        )
        assertEquals(listOf(Triple("client-a", 60_001L, 1L)), successorPublisher.published)
    }

    /** The production default seeds from wall-clock milliseconds, never from 0. */
    @Test
    fun defaultGenerationSeedIsWallClock() = runBlocking {
        val before = System.currentTimeMillis()
        val lease = AuthenticatedSurfaceIngressState(RecordingPublisher()).open("client-a", "frontend-a-1")
        val after = System.currentTimeMillis()
        assertTrue(lease.connectionGeneration in (before + 1)..(after + 1), "${lease.connectionGeneration}")
    }

    /** GH #339 R1: the backend records and logs each connection's frontend receipt once per change. */
    @Test
    fun frontendActivationReceiptIsRecordedOncePerChange() = runBlocking {
        val reported = mutableListOf<Triple<String, Long, ActivationReceipt>>()
        val state = AuthenticatedSurfaceIngressState(RecordingPublisher()) { client, generation, receipt ->
            reported += Triple(client, generation, receipt)
        }
        val lease = state.open("client-a", "frontend-a")
        val receipt = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND)

        state.publish("client-a", lease, snapshot("frontend-a", 1))
        assertEquals(emptyMap(), state.frontendActivationReceipts(), "schema 4 carries no receipt")
        state.publish("client-a", lease, snapshot("frontend-a", 2).copy(activationReceipt = receipt.encode()))
        state.publish("client-a", lease, snapshot("frontend-a", 3).copy(activationReceipt = receipt.encode()))
        state.publish("client-a", lease, snapshot("frontend-a", 4).copy(activationReceipt = "not a receipt"))

        assertEquals(listOf(Triple("client-a", lease.connectionGeneration, receipt)), reported)
        assertEquals(mapOf("client-a" to receipt), state.frontendActivationReceipts())
        state.retire("client-a", lease)
        assertEquals(emptyMap(), state.frontendActivationReceipts())
    }

    @Test
    fun retirementAndSequenceBothFailClosed() = runBlocking {
        val publisher = RecordingPublisher()
        val state = AuthenticatedSurfaceIngressState(publisher)
        val lease = state.open("client-a", "frontend-a")
        val first = snapshot("frontend-a", 1)

        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", lease, first).status)
        assertEquals(SurfaceIngressStatus.STALE_SEQUENCE, state.publish("client-a", lease, first).status)
        state.retire("client-a", lease)
        assertEquals(
            SurfaceIngressStatus.STALE_LEASE,
            state.publish("client-a", lease, snapshot("frontend-a", 2)).status,
        )
        assertEquals(1, publisher.published.size)
    }

    @Test
    fun authenticatedOpenOwnsLayoutBeforeFirstSnapshotAndThroughDeliveryFailure() = runBlocking {
        val publisher = RecordingPublisher()
        val state = AuthenticatedSurfaceIngressState(publisher, RecordingForwarder())
        val lease = state.open("client-a", "frontend-a")

        assertTrue(state.frontendAuthorityActive.value, "the debounce/schema/transport window is fenced")
        assertTrue(state.layoutAuthorityActive.value, "a remote lease owns measured layout immediately")
        assertEquals(LayoutPublicationReadiness.CONNECTED_BLOCKED, state.layoutPublicationReadiness.value)
        publisher.deliver = false
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            state.publish("client-a", lease, snapshot("frontend-a", 1)).status,
        )
        assertTrue(state.frontendAuthorityActive.value, "transport failure cannot revive backend guessing")
        assertEquals(
            LayoutPublicationReadiness.CONNECTED_BLOCKED,
            state.layoutPublicationReadiness.value,
            "a connected frontend without an accepted controller publication is explicitly blocked",
        )
        publisher.deliver = true
        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            state.publish("client-a", lease, snapshot("frontend-a", 2)).status,
        )
        assertEquals(LayoutPublicationReadiness.ACCEPTED, state.layoutPublicationReadiness.value)

        state.retire("client-a", lease)
        assertFalse(state.frontendAuthorityActive.value)
        assertFalse(state.layoutAuthorityActive.value)
        assertEquals(LayoutPublicationReadiness.INACTIVE, state.layoutPublicationReadiness.value)
    }

    @Test
    fun localFrontendLeaseNeverRetiresOrReplacesTheClassicLayoutProjection() = runBlocking {
        val publisher = RecordingPublisher()
        val forwarder = RecordingForwarder(acquiresLayout = false)
        val state = AuthenticatedSurfaceIngressState(publisher, forwarder)
        val lease = state.open("Host", "local-frontend")

        assertTrue(state.frontendAuthorityActive.value, "the snapshot lease remains live")
        assertFalse(state.layoutAuthorityActive.value, "a local lease is not pane-layout authority")
        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            state.publish("Host", lease, snapshot("local-frontend", 1)).status,
        )
        assertTrue(forwarder.forwarded.isEmpty(), "local snapshots stay off the Remote Dev layout lane")

        state.retire("Host", lease)
        assertTrue(forwarder.retired.isEmpty(), "no unowned layout endpoint is retired")
    }

    @Test
    fun receiptIsFencedByCurrentProjectionRevisionAndSurfaceIncarnation() = runBlocking {
        val publisher = RecordingPublisher()
        val state = AuthenticatedSurfaceIngressState(publisher)
        val lease = state.open("client-a", "frontend-a")
        state.publish("client-a", lease, snapshot("frontend-a", 1))

        val current = receipt(lease.connectionGeneration, revision = 1, surfaceGeneration = 1)
        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publishPresentationReceipt("client-a", lease, current).status)
        assertEquals(
            SurfaceIngressStatus.STALE_SEQUENCE,
            state.publishPresentationReceipt("client-a", lease, current.copy(presentationRevision = 0)).status,
        )
        assertEquals(
            SurfaceIngressStatus.STALE_SEQUENCE,
            state.publishPresentationReceipt(
                "client-a",
                lease,
                current.copy(identity = current.identity.copy(surfaceGeneration = 2)),
            ).status,
        )
        assertEquals(1, publisher.receipts.size)
    }

    /**
     * GH #320 option B: a snapshot reaches the pane-layout forwarder only after the backend
     * accepted it and delivered it to the controller policy; a rejected or undelivered one never
     * does, and a lease retirement or reconnect retires the forwarded connection's endpoint.
     */
    @Test
    fun acceptedSnapshotsReachTheLayoutForwarderAndRetirementRetiresIt() = runBlocking {
        val forwarder = RecordingForwarder()
        val publisher = RecordingPublisher()
        val state = AuthenticatedSurfaceIngressState(publisher, forwarder)
        val lease = state.open("client-a", "frontend-a")
        assertEquals(listOf("client-a" to lease.connectionGeneration), forwarder.opened)

        state.publish("client-a", lease, snapshot("frontend-a", 1))
        state.publish("client-a", lease, snapshot("frontend-a", 1))
        assertEquals(listOf(Triple("client-a", lease.connectionGeneration, 1L)), forwarder.forwarded)

        publisher.deliver = false
        state.publish("client-a", lease, snapshot("frontend-a", 2))
        assertEquals(1, forwarder.forwarded.size, "an undelivered snapshot never reaches pane layout")

        val replacement = state.open("client-a", "frontend-a-2")
        assertEquals(listOf("client-a" to lease.connectionGeneration), forwarder.retired)
        state.retire("client-a", replacement)
        assertEquals(
            listOf(
                "client-a" to lease.connectionGeneration,
                "client-a" to replacement.connectionGeneration,
            ),
            forwarder.retired,
        )
    }

    private fun receipt(generation: Long, revision: Long, surfaceGeneration: Long) =
        FrontendPresentationReceipt(
            projectId = projectId,
            identity = FrontendSurfaceIdentity("jetbrains-rd:client-a", generation, "root", surfaceGeneration),
            presentationRevision = revision,
            kind = FrontendPresentationKind.EMPTY,
            outcome = FrontendPresentationReceiptOutcome.APPLIED,
        )

    private fun snapshot(frontendId: String, sequence: Long) = FrontendSurfaceSnapshot(
        frontendInstanceId = frontendId,
        sequence = sequence,
        projectId = projectId,
        complete = true,
        surfaces = listOf(FrontendSurface("root", 1, SurfaceRole.MAIN, true, emptyList())),
    )

    private class RecordingForwarder(private val acquiresLayout: Boolean = true) : RemoteClientLayoutForwarder {
        val opened = mutableListOf<Pair<String, Long>>()
        val forwarded = mutableListOf<Triple<String, Long, Long>>()
        val retired = mutableListOf<Pair<String, Long>>()

        override fun open(clientId: String, generation: Long): Boolean {
            opened += clientId to generation
            return acquiresLayout
        }

        override fun forward(clientId: String, generation: Long, snapshot: FrontendSurfaceSnapshot) {
            forwarded += Triple(clientId, generation, snapshot.sequence)
        }

        override fun retire(clientId: String, generation: Long) {
            retired += clientId to generation
        }
    }

    private class RecordingPublisher : SurfaceSnapshotPublisher {
        var deliver = true
        val published = mutableListOf<Triple<String, Long, Long>>()
        val retired = mutableListOf<Pair<String, Long>>()
        val receipts = mutableListOf<FrontendPresentationReceipt>()

        override suspend fun publish(
            clientId: String,
            generation: Long,
            snapshot: FrontendSurfaceSnapshot,
        ): SurfaceDeliveryResult {
            published += Triple(clientId, generation, snapshot.sequence)
            return SurfaceDeliveryResult(deliver)
        }

        override suspend fun retire(clientId: String, generation: Long) {
            retired += clientId to generation
        }

        override suspend fun publishPresentationReceipt(
            clientId: String,
            generation: Long,
            receipt: FrontendPresentationReceipt,
        ): SurfaceDeliveryResult {
            receipts += receipt
            return SurfaceDeliveryResult(true)
        }
    }
}
