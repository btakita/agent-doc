package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
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
    fun `external replica events cannot bypass retained ack backoff`() {
        assertFalse(shouldStartRemoteDrainUtil(backoffScheduled = true))
        assertTrue(shouldStartRemoteDrainUtil(backoffScheduled = false))
    }

    /**
     * #crdtpushdrain: a controller-published frontier is positive evidence of pending
     * work, so it must drain urgently rather than sit behind the speculative no-op
     * backoff. Only `request_full_state` is exempt — it owns the text-adopt path.
     */
    @Test
    fun `controller published crdt frontiers bypass the speculative no-op drain backoff`() {
        assertTrue(shouldUrgentDrainForRemoteEventUtil("cpc_write"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("ack_replay"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("ack_recovery_force_refresh"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("fanout"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("response_cell_add"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("rebootstrap"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil(null))
        assertFalse(shouldUrgentDrainForRemoteEventUtil("request_full_state"))
    }

    @Test
    fun `crdt remote delivery urgently drains without replacing editor authority`() {
        val watcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val watcher = Files.readString(watcherPath)
        val deliveryBranch = watcher
            .substringAfter("EditorIntent.DeliverCrdtRemote.token ->")
            .substringBefore("EditorIntent.RefreshVcs.token ->")

        assertTrue(
            "controller pushes must route through the backoff-bypassing urgent drain",
            deliveryBranch.contains("shouldUrgentDrainForRemoteEventUtil(reasonToken)") &&
                deliveryBranch.contains("CrdtReplicaManager.requestUrgentRemoteDrain("),
        )
        assertFalse(deliveryBranch.contains("forceRefreshOpenDocumentReplica("))
    }

    /**
     * #crdtpushdrain: a successful urgent drain proves the document is live again, so
     * the escalated no-op backoff must be reset. Leaving it parked at its previous
     * (up to 30s) delay re-suppresses the *next* controller push.
     */
    @Test
    fun `useful urgent drain work resets the escalated no-op backoff`() {
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val manager = Files.readString(managerPath)
        val urgentBody = manager
            .substringAfter("fun requestUrgentRemoteDrain(")
            .substringBefore("fun requestTextAdopt(")

        assertTrue(
            "a useful urgent drain must clear the escalated no-op backoff counter",
            urgentBody.contains("applied > 0") &&
                urgentBody.contains("consecutiveNoOpReschedules.set(0)"),
        )
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
    fun `replace delivery rejects malformed or repairable remote canonical`() {
        assertTrue(remoteReplaceStructureAcceptedUtil(TemplateStructureProjectionState.Exact))
        assertFalse(
            remoteReplaceStructureAcceptedUtil(TemplateStructureProjectionState.RepairRequired),
        )
        assertFalse(remoteReplaceStructureAcceptedUtil(TemplateStructureProjectionState.Invalid))
    }

    @Test
    fun `replica registration never adopts malformed editor authority`() {
        assertTrue(replicaRegistrationStructureAcceptedUtil(TemplateStructureProjectionState.Exact))
        assertTrue(
            replicaRegistrationStructureAcceptedUtil(TemplateStructureProjectionState.RepairRequired),
        )
        assertFalse(
            replicaRegistrationStructureAcceptedUtil(TemplateStructureProjectionState.Invalid),
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
