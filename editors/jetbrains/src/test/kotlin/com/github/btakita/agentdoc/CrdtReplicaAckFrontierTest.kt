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
    fun `remote editor projection keeps its replica until the visible ack runs`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val listener = Files.readString(sourcePath)
            .substringAfter("override fun documentChanged(event: DocumentEvent)")
            .substringBefore("private fun seedAndAttachFromDocument(")
        val remoteApplyGuard = "if (CrdtReplicaManager.isApplyingRemote(filePath)) return"
        val forcedRefresh = "ensureOpenDocumentReplica(filePath, event.document, forceRefresh = true)"

        assertTrue(listener.contains(remoteApplyGuard))
        assertTrue(listener.indexOf(remoteApplyGuard) < listener.indexOf(forcedRefresh))
    }

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
    fun `unpublished visible text sends no ack and cannot trigger rebootstrap`() {
        val updates = listOf(
            update(generation = 11, expectedHash = "older"),
            update(generation = 12, expectedHash = "newer"),
        )

        assertNull(remoteAckReplayPlanUtil(updates, "not-yet-published"))
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
        assertTrue(shouldUrgentDrainForRemoteEventUtil("cp_write"))
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
        assertFalse(
            "one controller push must not enqueue a second drain through generic activity recording",
            deliveryBranch.contains("recordDocumentActivity(file, \"socket-crdt-remote\")"),
        )
        // #ensurereregister: the controller emits this typed reason only after it
        // observed an attached editor with no controller-side replica. A cached
        // plugin forwarder can outlive a controller recycle, so the handler must
        // not use that cache as a membership guard.
        val forceRefreshBranch = deliveryBranch
            .substringAfter("CrdtReplicaEventReason.AckRecoveryForceRefresh ->")
            .substringBefore("else -> Unit")
        assertTrue(
            "missing controller membership must force a replacement registration",
            forceRefreshBranch.contains("CrdtReplicaManager.forceRefreshOpenDocumentReplica("),
        )
        assertFalse(
            "plugin-local forwarder presence cannot suppress controller recovery",
            forceRefreshBranch.contains("hasOpenDocumentReplica"),
        )
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
    fun `replica registration preserves the attached editor as authority`() {
        assertEquals(
            ReplicaRegistrationMode.ExactTemplate,
            replicaRegistrationModeUtil(TemplateStructureProjectionState.Exact),
        )
        for (state in listOf(
            TemplateStructureProjectionState.RepairRequired,
            TemplateStructureProjectionState.Invalid,
        )) {
            assertEquals(
                ReplicaRegistrationMode.AuthoritativeEditorBaseline,
                replicaRegistrationModeUtil(state),
            )
        }
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
    fun `already applied native target replays without applying the delta twice`() {
        assertEquals(
            ReplicaBaselineDecision.ReplayRemoteTarget,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = true,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = true,
                recoveryInFlight = false,
            ),
        )
        assertEquals(
            2L,
            matchingRemoteTargetGenerationUtil(
                listOf(update(1, "first"), update(2, "target"), update(3, "later")),
                "target",
            ),
        )
    }

    @Test
    fun `stale native replica adopts an unrelated exact editor`() {
        assertEquals(
            ReplicaBaselineDecision.AdoptExactEditor,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                replicaMatchesExpected = true,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = false,
                recoveryInFlight = false,
            ),
        )
    }

    @Test
    fun `already visible target acknowledges and save echo realigns an equal replica`() {
        listOf(
            TemplateStructureProjectionState.Exact,
            TemplateStructureProjectionState.RepairRequired,
            TemplateStructureProjectionState.Invalid,
        ).forEach { validationState ->
            assertEquals(
                "delivery ACK must depend on exact target visibility, not validation=$validationState",
                ReplicaBaselineDecision.AcknowledgeRemoteTarget,
                replicaBaselineDecisionUtil(
                    editorState = validationState,
                    editorMatchesExpected = false,
                    replicaMatchesExpected = false,
                    replicaMatchesEditor = true,
                    editorMatchesRemoteTarget = true,
                    replicaMatchesRemoteTarget = true,
                    recoveryInFlight = false,
                ),
            )
        }
        assertEquals(
            ReplicaBaselineDecision.RealignShadow,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                replicaMatchesExpected = false,
                replicaMatchesEditor = true,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = false,
                recoveryInFlight = false,
            ),
        )
    }

    @Test
    fun `invalid editor requires an exact baseline`() {
        assertEquals(
            ReplicaBaselineDecision.ApplyRemoteRepair,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.RepairRequired,
                editorMatchesExpected = true,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = false,
                recoveryInFlight = false,
            ),
        )
        assertEquals(
            ReplicaBaselineDecision.RetryFailClosed,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Invalid,
                editorMatchesExpected = false,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = false,
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
