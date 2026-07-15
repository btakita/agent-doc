package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
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

    @Test
    fun `a retained ack frontier blocks another delivery pull`() {
        assertFalse(shouldPullRemoteDeliveryAfterAckReplayUtil(pendingAckCount = 1))
        assertFalse(shouldPullRemoteDeliveryAfterAckReplayUtil(pendingAckCount = 3))
        assertTrue(shouldPullRemoteDeliveryAfterAckReplayUtil(pendingAckCount = 0))
    }

    @Test
    fun `visible canonical projection is acknowledged independently of disk persistence`() {
        assertTrue(
            shouldAcknowledgeVisibleRemoteDeliveryUtil(
                editorText = "canonical",
                targetText = "canonical",
                diskPersisted = false,
            ),
        )
        assertTrue(
            shouldAcknowledgeVisibleRemoteDeliveryUtil(
                editorText = "canonical",
                targetText = "canonical",
                diskPersisted = true,
            ),
        )
        assertFalse(
            shouldAcknowledgeVisibleRemoteDeliveryUtil(
                editorText = "operator edit",
                targetText = "canonical",
                diskPersisted = true,
            ),
        )
    }

    @Test
    fun `controller transport loss requests replica refresh`() {
        assertTrue(
            pullDeliveryRequestsReplicaRefreshUtil(
                ReplicaPullDelivery.Unavailable("controller socket replaced"),
            ),
        )
        assertFalse(pullDeliveryRequestsReplicaRefreshUtil(ReplicaPullDelivery.Deltas(emptyList())))
        assertFalse(pullDeliveryRequestsReplicaRefreshUtil(ReplicaPullDelivery.Replace("current")))
    }

    @Test
    fun `template projection queues only an exact remote canonical`() {
        assertEquals(
            RemoteTemplateProjectionDecision.QueueRemote,
            remoteTemplateProjectionDecisionUtil(
                remoteState = TemplateStructureProjectionState.Exact,
                editorState = null,
                editorMatchesExpected = false,
                recoveryInFlight = false,
            ),
        )
        assertEquals(
            TemplateStructureProjectionState.RepairRequired,
            templateStructureProjectionStateUtil("raw", "normalized"),
        )
        assertEquals(
            TemplateStructureProjectionState.Invalid,
            templateStructureProjectionStateUtil("raw", null),
        )
    }

    @Test
    fun `rejected remote canonical adopts only an exact unchanged editor baseline`() {
        for (remoteState in listOf(
            TemplateStructureProjectionState.Invalid,
            TemplateStructureProjectionState.RepairRequired,
        )) {
            assertEquals(
                RemoteTemplateProjectionDecision.AdoptExactEditorBaseline,
                remoteTemplateProjectionDecisionUtil(
                    remoteState = remoteState,
                    editorState = TemplateStructureProjectionState.Exact,
                    editorMatchesExpected = true,
                    recoveryInFlight = false,
                ),
            )
        }
    }

    @Test
    fun `rejected remote canonical fails closed for stale invalid or active editor recovery`() {
        val rejectedStates = listOf(
            null,
            TemplateStructureProjectionState.Invalid,
            TemplateStructureProjectionState.RepairRequired,
        )
        for (editorState in rejectedStates) {
            assertEquals(
                RemoteTemplateProjectionDecision.RetryFailClosed,
                remoteTemplateProjectionDecisionUtil(
                    remoteState = TemplateStructureProjectionState.Invalid,
                    editorState = editorState,
                    editorMatchesExpected = true,
                    recoveryInFlight = false,
                ),
            )
        }
        assertEquals(
            RemoteTemplateProjectionDecision.RetryFailClosed,
            remoteTemplateProjectionDecisionUtil(
                remoteState = TemplateStructureProjectionState.Invalid,
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                recoveryInFlight = false,
            ),
        )
        assertEquals(
            RemoteTemplateProjectionDecision.RetryFailClosed,
            remoteTemplateProjectionDecisionUtil(
                remoteState = TemplateStructureProjectionState.Invalid,
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = true,
                recoveryInFlight = true,
            ),
        )
    }

    @Test
    fun `stale native replica adopts exact editor instead of overwriting it`() {
        assertEquals(
            ReplicaBaselineDecision.AdoptExactEditor,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = true,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                recoveryInFlight = false,
            ),
        )
        assertEquals(
            ReplicaBaselineDecision.AdoptExactEditor,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                replicaMatchesExpected = true,
                replicaMatchesEditor = false,
                recoveryInFlight = false,
            ),
        )
    }

    @Test
    fun `save echo realigns an equal replica and invalid editor fails closed`() {
        assertEquals(
            ReplicaBaselineDecision.RealignShadow,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                replicaMatchesExpected = false,
                replicaMatchesEditor = true,
                recoveryInFlight = false,
            ),
        )
        assertEquals(
            ReplicaBaselineDecision.RetryFailClosed,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.RepairRequired,
                editorMatchesExpected = true,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                recoveryInFlight = false,
            ),
        )
    }

    @Test
    fun `local delta is never applied against a stale native baseline`() {
        assertTrue(shouldForwardLocalDeltaUtil("before", "before"))
        assertFalse(shouldForwardLocalDeltaUtil("stale", "before"))
        assertFalse(shouldForwardLocalDeltaUtil(null, "before"))
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
