package com.github.btakita.agentdoc.split.backend

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

    private class RecordingPublisher : SurfaceSnapshotPublisher {
        val published = mutableListOf<Triple<String, Long, Long>>()
        val retired = mutableListOf<Pair<String, Long>>()
        val receipts = mutableListOf<FrontendPresentationReceipt>()

        override suspend fun publish(
            clientId: String,
            generation: Long,
            snapshot: FrontendSurfaceSnapshot,
        ): SurfaceDeliveryResult {
            published += Triple(clientId, generation, snapshot.sequence)
            return SurfaceDeliveryResult(true)
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
