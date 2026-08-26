package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CrdtReplicaProjectionFrontierTest {
    @Test
    fun `remote editor projection keeps its replica until visible state is published`() {
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
    fun `external replica events cannot bypass retained ack backoff`() {
        assertFalse(shouldStartRemoteDrainUtil(backoffScheduled = true))
        assertTrue(shouldStartRemoteDrainUtil(backoffScheduled = false))
    }

    @Test
    fun `retained canonical projection has no whole editor adoption surface`() {
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val manager = Files.readString(managerPath)

        assertFalse(manager.contains("shouldAdoptEditorTextUtil"))
        assertFalse(manager.contains("requestTextAdopt("))
        assertFalse(manager.contains("pushTextAdopt("))
    }

    @Test
    fun `remote projection is retained before op capture fencing can fail`() {
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val manager = Files.readString(managerPath)
        val queueBody = manager
            .substringAfter("private fun queueRemoteTextApply(")
            .substringBefore("private fun recoverRejectedRemoteCanonical(")

        assertTrue(
            queueBody.indexOf("retainedCanonicalProjectionPaths.add(filePath)") <
                queueBody.indexOf("prepareNonOperatorEditorMutationOnWorker(filePath)"),
        )
    }

    @Test
    fun `restart projects the controller bootstrap without whole editor publication`() {
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val manager = Files.readString(managerPath)
        val registrationBody = manager
            .substringAfter("private fun forwarderFor(")
            .substringBefore("private fun refreshReplicaAfterTransportLoss(")
        assertTrue(registrationBody.contains("forwarder.canonicalProjectionRetained"))
        assertTrue(
            registrationBody.contains("retainCanonicalProjectionAfterRegistration(filePath, forwarder)"),
        )
        assertFalse(
            registrationBody.contains("forwarder.ensureEditorText(initialEditorText)"),
        )
        val retainedRegistrationBody = manager
            .substringAfter("private fun retainCanonicalProjectionAfterRegistration(")
            .substringBefore("private fun refreshReplicaAfterTransportLoss(")
        assertTrue(
            "an exact registered buffer must publish the visible-state receipt",
            retainedRegistrationBody.contains("val visibleText = editorBufferText(filePath)") &&
                retainedRegistrationBody.contains("if (visibleText == canonical)") &&
                retainedRegistrationBody.contains(
                    "projectSettledVisibleState(filePath, forwarder, visibleText)",
                ),
        )
        assertFalse(
            "a captured pre-swap editor cut must never acknowledge the replacement generation",
            retainedRegistrationBody.contains("projectVisibleState(canonical)"),
        )
        assertTrue(
            "a failed registration receipt must remain retryable",
            retainedRegistrationBody.contains(
                "requestRemoteDrain(filePath, \"registration-visible-projection-retry\")",
            ),
        )
    }

    @Test
    fun `persist current saves and receipts only the exact visible replica`() {
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val manager = Files.readString(managerPath)
        val body = manager
            .substringAfter("private fun persistCurrentVisibleRevision(")
            .substringBefore("private fun reconcileRemotePersistence(")

        assertTrue(body.contains("visibleText.toByteArray(Charsets.UTF_8).size != expectedContentLen"))
        assertTrue(body.contains("forwarder.replicaText() != visibleText"))
        assertTrue(body.contains("saveDocument(document)"))
        assertTrue(body.contains("readRawDiskText(filePath) == visibleText"))
        assertTrue(
            body.contains("projectSettledVisibleState(filePath, forwarder, visibleText, true)"),
        )
        assertFalse(body.contains("applyMinimalDocumentEditUtil("))
        assertFalse(body.contains("reloadFromDisk("))
    }

    /**
     * #crdtpushdrain: a controller-published frontier is positive evidence of pending
     * work, so it must drain urgently rather than sit behind the speculative no-op
     * backoff. The retained Lazily key is the coalescing boundary.
     */
    @Test
    fun `controller published crdt frontiers bypass the speculative no-op drain backoff`() {
        assertTrue(shouldUrgentDrainForRemoteEventUtil("cp_write"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("ack_replay"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("fanout"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("response_cell_add"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("rebootstrap"))
        assertTrue(shouldUrgentDrainForRemoteEventUtil(null))
        assertTrue(shouldUrgentDrainForRemoteEventUtil("canonical_projection"))
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
        assertFalse(
            "controller delivery must never replace its authority from the editor buffer",
            deliveryBranch.contains("forceRefreshOpenDocumentReplica("),
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
            .substringBefore("private fun queueRemoteDrain(")

        assertTrue(
            "a useful urgent drain must clear the escalated no-op backoff counter",
            urgentBody.contains("applied > 0") &&
                urgentBody.contains("consecutiveNoOpReschedules.set(0)"),
        )
    }

    @Test
    fun `visible canonical projection is acknowledged independently of disk persistence`() {
        assertTrue(
            shouldProjectVisibleRemoteDeliveryUtil(
                editorText = "canonical",
                targetText = "canonical",
                diskPersisted = false,
            ),
        )
        assertTrue(
            shouldProjectVisibleRemoteDeliveryUtil(
                editorText = "canonical",
                targetText = "canonical",
                diskPersisted = true,
            ),
        )
        assertFalse(
            shouldProjectVisibleRemoteDeliveryUtil(
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
    fun `controller missing-member pull requests replica re-registration`() {
        val refused = JsonObject().apply {
            addProperty("refused", true)
            addProperty("reason", "missing_replica")
        }
        val reason = refusedReplicaPullReasonUtil(refused)
        assertEquals("missing_replica", reason)
        assertTrue(
            pullDeliveryRequestsReplicaRefreshUtil(
                ReplicaPullDelivery.Unavailable(reason!!),
            ),
        )
        assertEquals(null, refusedReplicaPullReasonUtil(JsonObject()))
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
        assertEquals(
            RemoteTemplateProjectionDecision.RecoverEditorBaseline,
            remoteTemplateProjectionDecisionUtil(
                remoteState = TemplateStructureProjectionState.Invalid,
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = true,
                recoveryInFlight = false,
            ),
        )
        assertEquals(
            RemoteTemplateProjectionDecision.RetryFailClosed,
            remoteTemplateProjectionDecisionUtil(
                remoteState = TemplateStructureProjectionState.RepairRequired,
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = true,
                recoveryInFlight = true,
            ),
        )
        assertEquals(
            RemoteTemplateProjectionDecision.RetryFailClosed,
            remoteTemplateProjectionDecisionUtil(
                remoteState = TemplateStructureProjectionState.Invalid,
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                recoveryInFlight = false,
            ),
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
    fun `replica registration preserves the controller bootstrap as authority`() {
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val registration = Files.readString(managerPath)
            .substringAfter("private fun retainCanonicalProjectionAfterRegistration(")
            .substringBefore("private fun refreshReplicaAfterTransportLoss(")

        assertTrue(registration.contains("val canonical = forwarder.replicaText()"))
        assertTrue(registration.contains("queueRemoteTextApply("))
        assertFalse(registration.contains("ensureEditorText("))
    }

    @Test
    fun `rejected remote canonical rebuilds from an unchanged exact editor baseline`() {
        for (remoteState in listOf(
            TemplateStructureProjectionState.Invalid,
            TemplateStructureProjectionState.RepairRequired,
        )) {
            assertEquals(
                RemoteTemplateProjectionDecision.RecoverEditorBaseline,
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
    fun `stale native replica never adopts an unrelated exact editor`() {
        assertEquals(
            ReplicaBaselineDecision.RetryFailClosed,
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
    fun `retained canonical projection never adopts a stale whole editor`() {
        assertEquals(
            ReplicaBaselineDecision.ApplyRemote,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                replicaMatchesExpected = true,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = false,
                recoveryInFlight = false,
                canonicalProjectionRetained = true,
            ),
        )
        assertEquals(
            ReplicaBaselineDecision.ReplayRemoteTarget,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = true,
                recoveryInFlight = false,
                canonicalProjectionRetained = true,
            ),
        )
        assertEquals(
            ReplicaBaselineDecision.RetryFailClosed,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = false,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = false,
                replicaMatchesRemoteTarget = false,
                recoveryInFlight = false,
                canonicalProjectionRetained = true,
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
                ReplicaBaselineDecision.ProjectRemoteTarget,
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
    fun `exact visible target reboots a stale replica from controller canonical before ack`() {
        assertEquals(
            ReplicaBaselineDecision.RebootstrapVisibleRemoteTarget,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = true,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = true,
                replicaMatchesRemoteTarget = false,
                recoveryInFlight = false,
                canonicalProjectionRetained = true,
            ),
        )
        assertEquals(
            ReplicaBaselineDecision.RetryFailClosed,
            replicaBaselineDecisionUtil(
                editorState = TemplateStructureProjectionState.Exact,
                editorMatchesExpected = true,
                replicaMatchesExpected = false,
                replicaMatchesEditor = false,
                editorMatchesRemoteTarget = true,
                replicaMatchesRemoteTarget = false,
                recoveryInFlight = true,
                canonicalProjectionRetained = true,
            ),
        )

        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val manager = Files.readString(managerPath)
        val rebootstrapEffect = manager
            .substringAfter("decision == ReplicaBaselineDecision.RebootstrapVisibleRemoteTarget")
            .substringBefore("decision == ReplicaBaselineDecision.ReplayRemoteTarget")
        assertTrue(rebootstrapEffect.contains("expectedEditorTextAtSwap = editorText"))
        assertTrue(rebootstrapEffect.contains("bootstrapFromControllerCanonical = true"))
        assertTrue(rebootstrapEffect.indexOf("replacement.replicaText()") <
            rebootstrapEffect.indexOf(
                "projectSettledVisibleState(filePath, replacement, editorText)",
            ))
        val registration = manager
            .substringAfter("private fun forwarderFor(")
            .substringBefore("private fun retainCanonicalProjectionAfterRegistration(")
        assertTrue(registration.contains("if (bootstrapFromControllerCanonical)"))
        assertTrue(registration.contains("null"))
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
        assertEquals(
            LocalReplicaBaselineDecision.ForwardLocal,
            localReplicaBaselineDecisionUtil("before", "before"),
        )
        assertEquals(
            LocalReplicaBaselineDecision.RebootstrapCanonicalThenForward,
            localReplicaBaselineDecisionUtil("stale", "before"),
        )

        val managerPath =
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
                Paths.get(
                    "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt",
                ),
            ).first { Files.exists(it) }
        val manager = Files.readString(managerPath)
        val localEffect =
            manager
                .substringAfter("private fun forwardLocalEditsFromShadow(")
                .substringBefore("fun requestRemoteDrain(")
        assertTrue(localEffect.contains("expectedCanonicalTextAtSwap = capturedBaseText"))
        assertTrue(localEffect.contains("expectedEditorTextAtSwap = visibleEditorText"))
        assertTrue(localEffect.contains("bootstrapFromControllerCanonical = true"))
        assertTrue(localEffect.contains("shadows[filePath] = beforeText"))
        assertTrue(
            localEffect.indexOf("replacement.replicaText() != capturedBaseText") <
                localEffect.indexOf("replacement.forwardLocalEdits(edits)"),
        )
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
