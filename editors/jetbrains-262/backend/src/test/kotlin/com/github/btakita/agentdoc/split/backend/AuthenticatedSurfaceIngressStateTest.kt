package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationKind
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendPresentationReceiptOutcome
import com.github.btakita.agentdoc.split.FrontendSurfaceIdentity
import com.github.btakita.agentdoc.split.FrontendClientBuild
import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.forSchema
import com.github.btakita.agentdoc.split.frontendLeaseInstanceId
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
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            onFrontendReceipt = { client, generation, receipt -> reported += Triple(client, generation, receipt) },
        )
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
    fun shellOnlyBackendAcceptsLegacySnapshotsAndRejectsUnpairedImplementationClaims() = runBlocking {
        val publisher = RecordingPublisher()
        val state = AuthenticatedSurfaceIngressState(publisher, RecordingForwarder())
        val lease = state.open("client-a", "frontend-a")

        val unexpected = snapshot("frontend-a", 1).copy(activationReceipt = frontendActivation.encode())
        assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, state.publish("client-a", lease, unexpected).status)
        assertTrue(publisher.published.isEmpty())
        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            state.publish("client-a", lease, snapshot("frontend-a", 2).forSchema(4)).status,
        )
    }

    @Test
    fun verifiedBackendGenerationRequiresTheMatchingDistinctFrontendReceipt() = runBlocking {
        val publisher = RecordingPublisher()
        val forwarder = RecordingForwarder()
        val state = AuthenticatedSurfaceIngressState(publisher, forwarder, backendActivationReceipt = backendActivation)
        val lease = state.open("client-a", "frontend-a")
        val base = snapshot("frontend-a", 1)
        val valid = base.copy(activationReceipt = frontendActivation.encode())
        val refused = listOf(
            base,
            base.copy(activationReceipt = "malformed receipt"),
            valid.copy(schemaVersion = 4),
            base.copy(activationReceipt = frontendActivation.copy(epoch = 0).encode()),
            base.copy(activationReceipt = frontendActivation.copy(epoch = backendActivation.epoch - 1).encode()),
            base.copy(activationReceipt = frontendActivation.copy(implSha256 = "c".repeat(64)).encode()),
            base.copy(activationReceipt = frontendActivation.copy(shellDigest = "d".repeat(64)).encode()),
            base.copy(activationReceipt = frontendActivation.copy(spiVersion = 2).encode()),
            base.copy(activationReceipt = frontendActivation.copy(jvmInstanceId = backendActivation.jvmInstanceId).encode()),
            base.copy(activationReceipt = backendActivation.encode()),
        )
        refused.forEach { candidate ->
            val ack = state.publish("client-a", lease, candidate)
            assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, ack.status)
            assertEquals(FRONTEND_AUTHORITY_UNAVAILABLE_READINESS, ack.diagnostic)
        }
        assertEquals(LayoutPublicationReadiness.CONNECTED_BLOCKED, state.layoutPublicationReadiness.value)
        assertTrue(publisher.published.isEmpty())
        assertTrue(forwarder.forwarded.isEmpty())

        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", lease, valid).status)
        assertEquals(LayoutPublicationReadiness.ACCEPTED, state.layoutPublicationReadiness.value)
        assertEquals(mapOf("client-a" to frontendActivation), state.frontendActivationReceipts())
        assertEquals(1, publisher.published.size)
        assertEquals(1, forwarder.forwarded.size)
    }

    @Test
    fun ingressCreatedBeforeBackendPeerActivationUsesTheLiveDigestAndEpoch() = runBlocking {
        var backend = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_BACKEND)
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            backendActivationReceiptProvider = { backend },
        )
        val lease = state.open("client-a", "frontend-a")

        backend = backendActivation
        val paired = snapshot("frontend-a", 1).copy(activationReceipt = frontendActivation.encode())

        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", lease, paired).status)
        assertEquals(mapOf("client-a" to frontendActivation), state.frontendActivationReceipts())
    }

    @Test
    fun mismatchedReceiptWithdrawsPublishedAuthorityUntilTheNextConnection() = runBlocking {
        val publisher = RecordingPublisher()
        val forwarder = RecordingForwarder()
        val state = AuthenticatedSurfaceIngressState(publisher, forwarder, backendActivationReceipt = backendActivation)
        val lease = state.open("client-a", "frontend-a")
        val valid = snapshot("frontend-a", 1).copy(activationReceipt = frontendActivation.encode())
        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", lease, valid).status)
        val wrongJvm = frontendActivation.copy(jvmInstanceId = "replacement-client-jvm")
        val invalid = valid.copy(sequence = 2, activationReceipt = wrongJvm.encode())
        assertEquals(
            SurfaceIngressStatus.STALE_SEQUENCE,
            state.publish("client-a", lease, invalid.copy(sequence = 1)).status,
        )
        assertEquals(LayoutPublicationReadiness.ACCEPTED, state.layoutPublicationReadiness.value)
        assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, state.publish("client-a", lease, invalid).status)
        assertTrue(state.layoutAuthorityActive.value, "a mismatched peer cannot revive backend layout guessing")
        assertEquals(LayoutPublicationReadiness.CONNECTED_BLOCKED, state.layoutPublicationReadiness.value)
        assertEquals(listOf("client-a" to lease.connectionGeneration), publisher.retired)
        assertEquals(listOf("client-a" to lease.connectionGeneration), forwarder.held)
        assertTrue(state.frontendActivationReceipts().isEmpty())
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            state.publishPresentationReceipt("client-a", lease, receipt(lease.connectionGeneration, 1, 1)).status,
        )
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            state.publish("client-a", lease, valid.copy(sequence = 3)).status,
        )
        assertTrue(publisher.receipts.isEmpty())
        assertEquals(1, publisher.published.size)
        assertEquals(1, forwarder.forwarded.size)

        val next = state.open("client-a", "frontend-a-reconnected")
        assertEquals(SurfaceIngressStatus.STALE_LEASE, state.publish("client-a", lease, valid.copy(sequence = 4)).status)
        val reconnected = snapshot("frontend-a-reconnected", 1).copy(activationReceipt = wrongJvm.encode())
        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", next, reconnected).status)
        assertEquals(LayoutPublicationReadiness.ACCEPTED, state.layoutPublicationReadiness.value)
        assertEquals(mapOf("client-a" to wrongJvm), state.frontendActivationReceipts())
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

    /**
     * GH #435: a Coder backend held a remote measured-layout lease (`frontend_measured`) while the
     * controller never received the Client's geometry, and its idea.log said nothing about why.
     * The backend now logs each refusal once per change, tells the forwarder (so a deferred route
     * names the hop), reports a lease that is still unpublished after the negotiation window with
     * its last refusal, and logs the acceptance that clears it.
     */
    @Test
    fun remoteLeaseLogsTheFailingPublicationHopAndItsRecovery() = runBlocking {
        val publisher = RecordingPublisher()
        val forwarder = RecordingForwarder()
        val lines = mutableListOf<Pair<Boolean, String>>()
        val checks = mutableListOf<Pair<Long, () -> Unit>>()
        val state = AuthenticatedSurfaceIngressState(
            publisher,
            forwarder,
            scheduleDiagnostic = { delayMs, check -> checks += delayMs to check },
            log = { warn, line -> lines += warn to line },
        )
        val lease = state.open("client-a", "frontend-a")
        assertEquals(listOf(FRONTEND_AUTHORITY_DIAGNOSTIC_DELAY_MS), checks.map { it.first })
        assertTrue(lines.single().second.contains("lease opened client=client-a"), lines.toString())
        assertTrue(lines.single().second.contains("layout_authority=remote_measured"), lines.toString())

        publisher.deliver = false
        publisher.diagnostic = "Timed out waiting for 2000 ms"
        repeat(3) { attempt ->
            assertEquals(
                SurfaceIngressStatus.DELIVERY_FAILED,
                state.publish("client-a", lease, snapshot("frontend-a", attempt + 1L)).status,
            )
        }
        val refusals = lines.filter { it.second.contains("snapshot refused") }
        assertEquals(1, refusals.size, "one line per refusal change, not per retry: $lines")
        assertTrue(refusals.single().first, "a refusal is a warning")
        assertTrue(refusals.single().second.contains("status=DELIVERY_FAILED"), refusals.toString())
        assertTrue(refusals.single().second.contains("diagnostic=Timed out waiting for 2000 ms"), refusals.toString())
        assertEquals(3, forwarder.refused.size, "every refusal reaches the route-visible state")

        checks.single().second()
        val stall = lines.last()
        assertTrue(stall.first)
        assertTrue(stall.second.contains("frontend layout publisher unavailable client=client-a"), stall.second)
        assertTrue(
            stall.second.contains("readiness_reason=$FRONTEND_AUTHORITY_CONNECTED_WITHOUT_PUBLICATION"),
            stall.second,
        )
        assertTrue(stall.second.contains("refusals=3"), stall.second)
        assertTrue(
            stall.second.contains("last_refusal=DELIVERY_FAILED:Timed out waiting for 2000 ms"),
            stall.second,
        )

        publisher.deliver = true
        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", lease, snapshot("frontend-a", 4)).status)
        val accepted = lines.last()
        assertFalse(accepted.first)
        assertTrue(accepted.second.contains("snapshot accepted client=client-a"), accepted.second)
        assertTrue(accepted.second.contains("sequence=4"), accepted.second)
        assertTrue(accepted.second.contains("readiness=accepted"), accepted.second)
        assertTrue(accepted.second.contains("first=true"), accepted.second)
        assertTrue(accepted.second.contains("after_refusal=DELIVERY_FAILED:"), accepted.second)

        val before = lines.size
        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", lease, snapshot("frontend-a", 5)).status)
        assertEquals(before, lines.size, "a steady accepted stream logs nothing")
        checks.single().second()
        assertEquals(before, lines.size, "an accepted lease never reports a stall")
    }

    @Test
    fun aRemoteLeaseThatNeverPublishesSaysTheFrontendSentNothing() = runBlocking {
        val lines = mutableListOf<Pair<Boolean, String>>()
        val checks = mutableListOf<() -> Unit>()
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            RecordingForwarder(),
            scheduleDiagnostic = { _, check -> checks += check },
            log = { warn, line -> lines += warn to line },
        )
        val lease = state.open("client-a", "frontend-a")
        checks.single()()
        assertTrue(lines.last().second.contains("last_refusal=none(the_frontend_published_no_snapshot)"), lines.toString())

        state.retire("client-a", lease)
        val retiredLines = lines.size
        checks.single()()
        assertEquals(retiredLines, lines.size, "a retired lease is not reported")
    }

    @Test
    fun aLocalLeaseSchedulesNoLayoutStallCheck() = runBlocking {
        val checks = mutableListOf<() -> Unit>()
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            RecordingForwarder(acquiresLayout = false),
            scheduleDiagnostic = { _, check -> checks += check },
            log = { _, _ -> },
        )
        state.open("client-a", "frontend-a")
        assertTrue(checks.isEmpty())
    }

    /**
     * GH #435: `agent-doc upgrade` on the Coder backend left the JetBrains Client frontend on the
     * previous build. The backend names that split once per lease from the frontend's own receipt.
     */
    @Test
    fun aFrontendFromAnotherBuildIsReportedOncePerLease() = runBlocking {
        val mismatches = mutableListOf<String>()
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            RecordingForwarder(),
            backendActivationReceipt = backendActivation.copy(implSha256 = null, epoch = 0),
            scheduleDiagnostic = { _, _ -> },
            log = { _, _ -> },
            onFrontendPluginMismatch = { client, _, mismatch -> mismatches += "$client ${mismatch.detail}" },
        )
        val lease = state.open("client-a", "frontend-a")
        val stale = frontendActivation.copy(shellDigest = "d".repeat(64), implSha256 = null, epoch = 0)
        repeat(2) { attempt ->
            state.publish(
                "client-a",
                lease,
                snapshot("frontend-a", attempt + 1L).copy(activationReceipt = stale.encode()),
            )
        }
        assertEquals(1, mismatches.size, mismatches.toString())
        assertTrue(mismatches.single().contains("client_shell=dddddddddddd backend_shell=aaaaaaaaaaaa"), mismatches.toString())
        assertTrue(
            mismatches.single().contains("this backend withholds its frontend release from that Client"),
            mismatches.toString(),
        )
        // GH #440 review (cycle 2): a receipt-proved shell is confirmed by the backend's own line
        // (BackendSurfaceSnapshotRpcApi), which every Client causes; the Client's own line only
        // exists from 0.2.1055 on.
        assertTrue(
            mismatches.single().contains(
                "(backend idea.log: `[frontend-impl] client shell mismatch ... " +
                    "client_shell=dddddddddddd ... backend_shell=aaaaaaaaaaaa`",
            ),
            mismatches.toString(),
        )
        // GH #440 review (cycle 3): the quoted backend lines carry no `client=` token, so
        // `agent-doc upgrade` and a grep for `client shell mismatch client=` skip this line.
        assertFalse(mismatches.single().contains("mismatch client=..."), mismatches.toString())
        assertTrue(
            mismatches.single().contains(
                "a Client from 0.2.1055 on also logs `[frontend-impl] client plugin mismatch compatibility=... " +
                    "backend_shell=aaaaaaaaaaaa client_shell=dddddddddddd` in its idea.log)",
            ),
            mismatches.toString(),
        )
        assertFalse(mismatches.single().contains("refused:"), mismatches.toString())
    }

    /**
     * GH #435 (Coder, 2026-11-24): the Client's 0.2.1046 frontend timed out its 5 s schema
     * negotiation and published schema 1 for the whole lease. Schema 1 carries no activation
     * receipt, so every snapshot was refused, and the backend called it an "older Agent Doc
     * frontend" although both peers ran 0.2.1046. Schema 1 from a current frontend is that
     * fallback, so the backend says so and names the reconnect first.
     */
    @Test
    fun aSchemaOneSnapshotIsReportedAsAnUnansweredNegotiationWithTheReconnectRemedy() = runBlocking {
        val mismatches = mutableListOf<FrontendPluginMismatch>()
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            RecordingForwarder(),
            backendActivationReceipt = backendActivation,
            scheduleDiagnostic = { _, _ -> },
            log = { _, _ -> },
            onFrontendPluginMismatch = { _, _, mismatch -> mismatches += mismatch },
        )
        val lease = state.open("client-a", "frontend-a")
        val ack = state.publish(
            "client-a",
            lease,
            snapshot("frontend-a", 1).copy(activationReceipt = frontendActivation.encode()).forSchema(1),
        )

        assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, ack.status, "schema 1 carries no activation receipt")
        assertEquals(FRONTEND_AUTHORITY_UNAVAILABLE_READINESS, ack.diagnostic)
        val mismatch = mismatches.single()
        assertTrue(mismatch.unansweredNegotiation)
        assertTrue(mismatch.detail.contains("frontend_schema=1 backend_schema=5"), mismatch.detail)
        assertTrue(mismatch.detail.contains("negotiation"), mismatch.detail)
        assertFalse(mismatch.detail.contains("older Agent Doc frontend"), mismatch.detail)

        val remedy = frontendPluginMismatchRemedy(mismatch, "0.2.1052", "IntelliJIdea2026.3")
        assertTrue(remedy.startsWith("reconnect the JetBrains Client"), remedy)
        assertTrue(remedy.contains("snapshot schema negotiated version=1 backend_offered=none"), remedy)
        // Only when the Client log names another plugin is a reinstall the remedy.
        assertTrue(remedy.contains("names a plugin other than 0.2.1052"), remedy)
        assertTrue(remedy.contains("agent-doc-jetbrains-262-0.2.1052.zip"), remedy)
        assertFalse(remedy.contains('<') || remedy.contains('>'), remedy)
        val olderRemedy = frontendPluginMismatchRemedy(
            frontendPluginMismatch(snapshot("frontend-a", 1).copy(schemaVersion = 4), null, "a".repeat(64))!!,
            "0.2.1052",
            "IntelliJIdea2026.3",
        )
        assertTrue(olderRemedy.startsWith("install the same agent-doc-jetbrains-262-0.2.1052.zip"), olderRemedy)
    }

    /**
     * GH #441 (Coder, 2026-11-24): both peers ran 0.2.1052. On a cold start the Client's 5 s
     * schema negotiation went unanswered, so it published schema 1 provisionally and asked again
     * after the refusal; its next snapshot was schema 5 and was accepted. The backend still showed
     * the reinstall dialog for that first, provisional snapshot.
     */
    @Test
    fun aProvisionalSchemaOneFromTheBackendsOwnPluginIsNotReported() = runBlocking {
        val mismatches = mutableListOf<FrontendPluginMismatch>()
        val state = mismatchReportingState(mismatches, backendPlugin = "0.2.1053")
        val frontend = frontendLeaseInstanceId("frontend-a", "0.2.1053")
        val lease = state.open("client-a", frontend)

        val provisional = state.publish("client-a", lease, snapshot(frontend, 1).forSchema(1))
        assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, provisional.status)
        assertTrue(mismatches.isEmpty(), "the frontend asks again after this refusal: $mismatches")

        val negotiated = snapshot(frontend, 2).copy(activationReceipt = frontendActivation.encode())
        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-a", lease, negotiated).status)
        assertTrue(mismatches.isEmpty(), mismatches.toString())
    }

    @Test
    fun aSchemaOneThatFollowsARefusalIsReportedOnce() = runBlocking {
        val mismatches = mutableListOf<FrontendPluginMismatch>()
        val state = mismatchReportingState(mismatches, backendPlugin = "0.2.1053")
        val frontend = frontendLeaseInstanceId("frontend-a", "0.2.1053")
        val lease = state.open("client-a", frontend)

        state.publish("client-a", lease, snapshot(frontend, 1).forSchema(1))
        assertTrue(mismatches.isEmpty(), mismatches.toString())
        repeat(2) { attempt ->
            state.publish("client-a", lease, snapshot(frontend, attempt + 2L).forSchema(1))
        }
        val mismatch = mismatches.single()
        assertTrue(mismatch.unansweredNegotiation)
        assertTrue(mismatch.detail.contains("frontend_schema=1 backend_schema=5"), mismatch.detail)
    }

    @Test
    fun aSchemaOneFromAnotherOrAnUnnamedPluginIsReportedAtOnce() = runBlocking {
        listOf(frontendLeaseInstanceId("frontend-a", "0.2.1046"), "frontend-a").forEach { frontend ->
            val mismatches = mutableListOf<FrontendPluginMismatch>()
            val state = mismatchReportingState(mismatches, backendPlugin = "0.2.1053")
            val lease = state.open("client-a", frontend)
            state.publish("client-a", lease, snapshot(frontend, 1).forSchema(1))
            assertTrue(mismatches.single().unansweredNegotiation, "$frontend: $mismatches")
        }
    }

    @Test
    fun anAcceptedProvisionalSchemaOneIsReportedBecauseNoRefusalAsksAgain() = runBlocking {
        val mismatches = mutableListOf<FrontendPluginMismatch>()
        // A shell-only backend admits a schema-1 snapshot (no receipt is required), so the
        // frontend never sees the refusal that would make it ask for the schema again.
        val state = mismatchReportingState(
            mismatches,
            backendPlugin = "0.2.1053",
            backendReceipt = backendActivation.copy(implSha256 = null, epoch = 0),
        )
        val frontend = frontendLeaseInstanceId("frontend-a", "0.2.1053")
        val lease = state.open("client-a", frontend)

        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            state.publish("client-a", lease, snapshot(frontend, 1).forSchema(1)).status,
        )
        assertTrue(mismatches.single().unansweredNegotiation, mismatches.toString())
    }

    /**
     * GH #435 review (cycle 1): after a backend IDE restart its peer is not activated. The backend
     * answered the Client's first schema ask, but the answer reached the Client after its 5 s
     * window, so the Client published a provisional schema 1, which the shell-only backend
     * accepted (no refusal, so no re-ask). At the Client's 30 s release-poll cap the offer found
     * the Client negotiated and on this shell and activated the backend peer. The next snapshot
     * then hit the fence, which revoked the lease because it had an accepted snapshot: the
     * re-asked schema 5 with the paired receipt was refused until a reconnect. This drives the
     * real delivery gate, both RPC implementations and the ingress state through that sequence.
     */
    @Test
    fun aLateFirstSchemaAnswerOnAColdBackendRecoversOnTheSameLease() = runBlocking {
        val clientId = "client-a"
        val gate = FrontendReleaseDeliveryGate(backendShell = { backendActivation.shellDigest }, clock = { 1_000L })
        val releasePolls = FrontendReleasePollLedger()
        var backend = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_BACKEND).copy(
            jvmInstanceId = backendActivation.jvmInstanceId,
            shellDigest = backendActivation.shellDigest,
            spiVersion = backendActivation.spiVersion,
        )
        var activations = 0
        val release = FrontendImplementationRelease(
            releaseVersion = "0.2.1060",
            packageUrl = "https://example.invalid/agent-doc-jetbrains-262-0.2.1060.zip",
            signatureUrl = "https://example.invalid/agent-doc-jetbrains-262-0.2.1060.zip.sig",
            implEntry = "impl/agent-doc-frontend-impl-0.2.1060.jar",
            implSha256 = backendActivation.implSha256!!,
            shellDigest = backendActivation.shellDigest,
            spiVersion = backendActivation.spiVersion,
            epoch = backendActivation.epoch,
            packageBytes = ByteArray(16),
            signatureBytes = ByteArray(8),
        )
        // Release discovery activates the backend peer idempotently; count the transitions only.
        val activate = {
            if (backend != backendActivation) activations++
            backend = backendActivation
            release
        }
        val snapshotApi = BackendSurfaceSnapshotRpcApi(
            frontendRelease = activate,
            gate = gate,
            releasePolls = releasePolls,
            releaseSelected = { true },
            callerClientId = { clientId },
            resolveProject = { null },
        )
        val releaseApi = BackendFrontendReleaseRpcApi(
            currentRelease = activate,
            selectedRelease = { release },
            backend = { BackendPluginBuild("0.2.1060", "/backend/plugins/agent-doc-jetbrains-262", backendActivation.shellDigest) },
            gate = gate,
            releasePolls = releasePolls,
            callerClientId = { clientId },
        )
        val publisher = RecordingPublisher()
        val forwarder = RecordingForwarder()
        val lines = mutableListOf<String>()
        val state = AuthenticatedSurfaceIngressState(
            publisher,
            forwarder,
            backendActivationReceiptProvider = { backend },
            clientShellEvidence = releasePolls,
            scheduleDiagnostic = { _, _ -> },
            log = { _, line -> lines += line },
            onFrontendPluginMismatch = { _, _, _ -> },
            backendPluginVersion = { "0.2.1060" },
        )
        // The backend's half of one publish: the RPC observes the Client's shell (and may activate
        // the peer) before the ingress fence reads the snapshot.
        suspend fun publish(lease: SurfaceIngressLease, snapshot: FrontendSurfaceSnapshot): SurfaceIngressAck {
            // The test resolves no project, so the RPC stops after its shell observation.
            assertEquals(SurfaceIngressStatus.PROJECT_MISMATCH, snapshotApi.publishSnapshot(lease, snapshot).status)
            return state.publish(clientId, lease, snapshot)
        }

        val frontend = frontendLeaseInstanceId("frontend-a", "0.2.1060")
        gate.leaseOpened(clientId)
        val lease = state.open(clientId, frontend)
        // The backend answers the first ask; the Client gives up on it after 5 s.
        snapshotApi.supportedSnapshotSchemaVersion()

        assertEquals(SurfaceIngressStatus.ACCEPTED, publish(lease, snapshot(frontend, 1).forSchema(1)).status)
        assertEquals(0, activations, "a schema 1 proves no shell, so the peer stays shell-only")

        // The Client's release poll at its 30 s cap: the offer activates the backend peer.
        val offer = releaseApi.frontendReleaseOffer(
            FrontendClientBuild("0.2.1060", "/client/plugins/frontend/agent-doc-jetbrains-262", backendActivation.shellDigest, 1),
        )
        assertEquals(1, activations)
        assertTrue(offer.release != null, "a negotiated Client on this shell is offered the release")

        // A capture before the Client activated: still schema 1. Refused, but not revoked.
        val beforeActivation = publish(lease, snapshot(frontend, 2).forSchema(1))
        assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, beforeActivation.status)
        assertEquals(FRONTEND_AUTHORITY_UNAVAILABLE_READINESS, beforeActivation.diagnostic)
        assertEquals(LayoutPublicationReadiness.CONNECTED_BLOCKED, state.layoutPublicationReadiness.value)
        assertTrue(publisher.retired.isEmpty(), "the controller generation is not retired: ${publisher.retired}")
        assertTrue(forwarder.held.isEmpty(), "the layout is not held: ${forwarder.held}")
        assertEquals(1, lines.count { it.contains("snapshot awaits frontend activation client=$clientId") }, lines.toString())

        // The refusal makes the Client ask again; this time the answer arrives (schema 5). Its
        // shell-only receipt is refused while it fetches the release, again without revoking.
        assertEquals(5, snapshotApi.supportedSnapshotSchemaVersion())
        val shellOnlyFrontend = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND).copy(
            jvmInstanceId = frontendActivation.jvmInstanceId,
            shellDigest = frontendActivation.shellDigest,
            spiVersion = frontendActivation.spiVersion,
        )
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            publish(lease, snapshot(frontend, 3).copy(activationReceipt = shellOnlyFrontend.encode())).status,
        )
        assertTrue(snapshotApi.frontendImplementationRelease() != null, "the Client fetches the release once")
        assertEquals(1, activations)

        // After its activation the Client captures again: the paired receipt is admitted.
        val activated = publish(lease, snapshot(frontend, 4).copy(activationReceipt = frontendActivation.encode()))
        assertEquals(SurfaceIngressStatus.ACCEPTED, activated.status, activated.diagnostic)
        assertEquals(LayoutPublicationReadiness.ACCEPTED, state.layoutPublicationReadiness.value)
        assertEquals(mapOf(clientId to frontendActivation), state.frontendActivationReceipts())
        assertTrue(publisher.retired.isEmpty(), publisher.retired.toString())
        assertEquals(listOf(1L, 4L), publisher.published.map { it.third })
        assertTrue(lines.any { it.contains("snapshot accepted client=$clientId") && it.contains("sequence=4") }, lines.toString())

        // Once a receipt was accepted, a mismatch still revokes the lease.
        val wrongJvm = frontendActivation.copy(jvmInstanceId = "replacement-client-jvm")
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            publish(lease, snapshot(frontend, 5).copy(activationReceipt = wrongJvm.encode())).status,
        )
        assertEquals(listOf(clientId to lease.connectionGeneration), publisher.retired)
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            publish(lease, snapshot(frontend, 6).copy(activationReceipt = frontendActivation.encode())).status,
        )
    }

    /**
     * GH #435 review (cycle 1) / GH #440: a receipt-less lease whose Client is proven unable to load
     * this backend's release (another shell) can never pass the fence, so when the backend peer
     * activates (for another Client) that lease is still revoked and its layout held.
     */
    @Test
    fun aReceiptLessLeaseOfAClientOnAnotherShellIsStillRevokedWhenTheBackendActivates() = runBlocking {
        var backend = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_BACKEND).copy(
            jvmInstanceId = backendActivation.jvmInstanceId,
            shellDigest = backendActivation.shellDigest,
            spiVersion = backendActivation.spiVersion,
        )
        val releasePolls = FrontendReleasePollLedger()
        releasePolls.clientBuild("client-old", "c".repeat(64), "0.2.1055", "/client/plugins/frontend/agent-doc-jetbrains-262")
        val publisher = RecordingPublisher()
        val forwarder = RecordingForwarder()
        val state = AuthenticatedSurfaceIngressState(
            publisher,
            forwarder,
            backendActivationReceiptProvider = { backend },
            clientShellEvidence = releasePolls,
            scheduleDiagnostic = { _, _ -> },
            log = { _, _ -> },
            onFrontendPluginMismatch = { _, _, _ -> },
            backendPluginVersion = { "0.2.1060" },
        )
        val frontend = frontendLeaseInstanceId("frontend-old", "0.2.1055")
        val lease = state.open("client-old", frontend)
        assertEquals(SurfaceIngressStatus.ACCEPTED, state.publish("client-old", lease, snapshot(frontend, 1).forSchema(1)).status)

        backend = backendActivation
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            state.publish("client-old", lease, snapshot(frontend, 2).forSchema(1)).status,
        )
        assertEquals(listOf("client-old" to lease.connectionGeneration), publisher.retired)
        assertEquals(listOf("client-old" to lease.connectionGeneration), forwarder.held)
        assertEquals(LayoutPublicationReadiness.CONNECTED_BLOCKED, state.layoutPublicationReadiness.value)
    }

    /**
     * GH #435 review (cycle 4): the delivery gate keeps its negotiation per Client, not per lease.
     * Two projects on one Client connection open their leases together. Project B's lease opens
     * after the backend answered project A's schema ask and before A's first schema-5 snapshot
     * reaches the backend, so that snapshot does not activate the still cold backend peer, and the
     * shell-only fence accepts its shell-only receipt. B's first snapshot then activates the peer.
     * A's next snapshot (before the Client activated) must wait for the paired receipt, not revoke
     * A's lease: a shell-only receipt is no activation the fence could take back.
     */
    @Test
    fun aShellOnlyReceiptAcceptedBeforeASecondProjectActivatesThePeerDoesNotRevokeTheLease() = runBlocking {
        val cold = ColdBackend()
        val lines = mutableListOf<String>()
        val publisherA = RecordingPublisher()
        val forwarderA = RecordingForwarder()
        val stateA = cold.state(publisherA, forwarderA, lines)
        val stateB = cold.state(RecordingPublisher(), RecordingForwarder(), mutableListOf())
        val frontendA = frontendLeaseInstanceId("frontend-a", "0.2.1060")
        val frontendB = frontendLeaseInstanceId("frontend-b", "0.2.1060")

        cold.gate.leaseOpened(cold.clientId)
        val leaseA = stateA.open(cold.clientId, frontendA)
        assertEquals(5, cold.snapshotApi.supportedSnapshotSchemaVersion())
        // Project B opens its lease while A's first snapshot is in flight.
        cold.gate.leaseOpened(cold.clientId)
        val leaseB = stateB.open(cold.clientId, frontendB)

        assertEquals(SurfaceIngressStatus.ACCEPTED, cold.publish(stateA, leaseA, snapshot(frontendA, 1).shellOnly()).status)
        assertEquals(0, cold.activations, "A's snapshot raced B's lease, so the peer stays shell-only")

        assertEquals(5, cold.snapshotApi.supportedSnapshotSchemaVersion())
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            cold.publish(stateB, leaseB, snapshot(frontendB, 1).shellOnly()).status,
        )
        assertEquals(1, cold.activations, "B's negotiated schema-5 snapshot activates the peer")

        // A focus event on project A before the Client activated: refused, not revoked.
        val beforeActivation = cold.publish(stateA, leaseA, snapshot(frontendA, 2).shellOnly())
        assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, beforeActivation.status)
        assertEquals(FRONTEND_AUTHORITY_UNAVAILABLE_READINESS, beforeActivation.diagnostic)
        assertEquals(LayoutPublicationReadiness.CONNECTED_BLOCKED, stateA.layoutPublicationReadiness.value)
        assertTrue(publisherA.retired.isEmpty(), "A's controller generation is not retired: ${publisherA.retired}")
        assertTrue(forwarderA.held.isEmpty(), "A's layout is not held: ${forwarderA.held}")
        assertEquals(1, lines.count { it.contains("snapshot awaits frontend activation client=${cold.clientId}") }, lines.toString())

        assertTrue(cold.snapshotApi.frontendImplementationRelease() != null, "the Client fetches the release")
        val activatedA = cold.publish(stateA, leaseA, snapshot(frontendA, 3).copy(activationReceipt = frontendActivation.encode()))
        assertEquals(SurfaceIngressStatus.ACCEPTED, activatedA.status, activatedA.diagnostic)
        assertEquals(LayoutPublicationReadiness.ACCEPTED, stateA.layoutPublicationReadiness.value)
        assertEquals(mapOf(cold.clientId to frontendActivation), stateA.frontendActivationReceipts())
        assertEquals(listOf(1L, 3L), publisherA.published.map { it.third })
        assertTrue(publisherA.retired.isEmpty(), publisherA.retired.toString())
        assertEquals(
            SurfaceIngressStatus.ACCEPTED,
            cold.publish(stateB, leaseB, snapshot(frontendB, 2).copy(activationReceipt = frontendActivation.encode())).status,
        )
    }

    /**
     * GH #435 review (cycle 4): the backend's first release discovery for a negotiated Client
     * failed, so its schema-5 snapshot with a shell-only receipt was accepted by the still
     * shell-only fence. The Client's next release offer activates the peer. The lease then waits
     * for the paired receipt instead of being revoked; once that activated receipt is accepted, a
     * mismatch still revokes the lease.
     */
    @Test
    fun aShellOnlyReceiptAcceptedAfterAFailedFirstReleaseDiscoveryDoesNotRevokeTheLease() = runBlocking {
        val cold = ColdBackend(failFirstDiscovery = true)
        val lines = mutableListOf<String>()
        val publisher = RecordingPublisher()
        val forwarder = RecordingForwarder()
        val state = cold.state(publisher, forwarder, lines)
        val frontend = frontendLeaseInstanceId("frontend-a", "0.2.1060")

        cold.gate.leaseOpened(cold.clientId)
        val lease = state.open(cold.clientId, frontend)
        assertEquals(5, cold.snapshotApi.supportedSnapshotSchemaVersion())
        assertEquals(SurfaceIngressStatus.ACCEPTED, cold.publish(state, lease, snapshot(frontend, 1).shellOnly()).status)
        assertEquals(0, cold.activations, "the first release discovery failed, so the peer stays shell-only")

        val offer = cold.releaseApi.frontendReleaseOffer(
            FrontendClientBuild("0.2.1060", "/client/plugins/frontend/agent-doc-jetbrains-262", backendActivation.shellDigest, 1),
        )
        assertTrue(offer.release != null, "a negotiated Client on this shell is offered the release")
        assertEquals(1, cold.activations)

        val beforeActivation = cold.publish(state, lease, snapshot(frontend, 2).shellOnly())
        assertEquals(SurfaceIngressStatus.DELIVERY_FAILED, beforeActivation.status)
        assertEquals(LayoutPublicationReadiness.CONNECTED_BLOCKED, state.layoutPublicationReadiness.value)
        assertTrue(publisher.retired.isEmpty(), "the controller generation is not retired: ${publisher.retired}")
        assertTrue(forwarder.held.isEmpty(), "the layout is not held: ${forwarder.held}")
        assertEquals(1, lines.count { it.contains("snapshot awaits frontend activation client=${cold.clientId}") }, lines.toString())

        assertTrue(cold.snapshotApi.frontendImplementationRelease() != null, "the Client fetches the release")
        val activated = cold.publish(state, lease, snapshot(frontend, 3).copy(activationReceipt = frontendActivation.encode()))
        assertEquals(SurfaceIngressStatus.ACCEPTED, activated.status, activated.diagnostic)
        assertEquals(LayoutPublicationReadiness.ACCEPTED, state.layoutPublicationReadiness.value)
        assertTrue(publisher.retired.isEmpty(), publisher.retired.toString())

        // Once an activated receipt was accepted, a mismatch still revokes the lease.
        val wrongJvm = frontendActivation.copy(jvmInstanceId = "replacement-client-jvm")
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            cold.publish(state, lease, snapshot(frontend, 4).copy(activationReceipt = wrongJvm.encode())).status,
        )
        assertEquals(listOf(cold.clientId to lease.connectionGeneration), publisher.retired)
        assertEquals(
            SurfaceIngressStatus.DELIVERY_FAILED,
            cold.publish(state, lease, snapshot(frontend, 5).copy(activationReceipt = frontendActivation.encode())).status,
        )
    }

    /**
     * GH #435 review (cycle 4): a freshly restarted backend whose peer is not activated yet, with
     * the real delivery gate and both RPC implementations; release discovery activates the peer.
     */
    private inner class ColdBackend(failFirstDiscovery: Boolean = false) {
        val clientId = "client-a"
        val gate = FrontendReleaseDeliveryGate(backendShell = { backendActivation.shellDigest }, clock = { 1_000L })
        private val releasePolls = FrontendReleasePollLedger()
        private var backend = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_BACKEND).copy(
            jvmInstanceId = backendActivation.jvmInstanceId,
            shellDigest = backendActivation.shellDigest,
            spiVersion = backendActivation.spiVersion,
        )
        var activations = 0
            private set
        private var discoveries = 0
        private val release = FrontendImplementationRelease(
            releaseVersion = "0.2.1060",
            packageUrl = "https://example.invalid/agent-doc-jetbrains-262-0.2.1060.zip",
            signatureUrl = "https://example.invalid/agent-doc-jetbrains-262-0.2.1060.zip.sig",
            implEntry = "impl/agent-doc-frontend-impl-0.2.1060.jar",
            implSha256 = backendActivation.implSha256!!,
            shellDigest = backendActivation.shellDigest,
            spiVersion = backendActivation.spiVersion,
            epoch = backendActivation.epoch,
            packageBytes = ByteArray(16),
            signatureBytes = ByteArray(8),
        )
        private val activate = {
            discoveries++
            if (failFirstDiscovery && discoveries == 1) error("transient release discovery failure")
            if (backend != backendActivation) activations++
            backend = backendActivation
            release
        }
        val snapshotApi = BackendSurfaceSnapshotRpcApi(
            frontendRelease = activate,
            gate = gate,
            releasePolls = releasePolls,
            releaseSelected = { true },
            callerClientId = { clientId },
            resolveProject = { null },
        )
        val releaseApi = BackendFrontendReleaseRpcApi(
            currentRelease = activate,
            selectedRelease = { release },
            backend = { BackendPluginBuild("0.2.1060", "/backend/plugins/agent-doc-jetbrains-262", backendActivation.shellDigest) },
            gate = gate,
            releasePolls = releasePolls,
            callerClientId = { clientId },
        )

        /** One project's ingress state; every project of the Client shares the gate. */
        fun state(publisher: RecordingPublisher, forwarder: RecordingForwarder, lines: MutableList<String>) =
            AuthenticatedSurfaceIngressState(
                publisher,
                forwarder,
                backendActivationReceiptProvider = { backend },
                clientShellEvidence = releasePolls,
                scheduleDiagnostic = { _, _ -> },
                log = { _, line -> lines += line },
                onFrontendPluginMismatch = { _, _, _ -> },
                backendPluginVersion = { "0.2.1060" },
            )

        /** The backend's half of one publish: the RPC observes the shell before the fence reads it. */
        suspend fun publish(
            state: AuthenticatedSurfaceIngressState,
            lease: SurfaceIngressLease,
            snapshot: FrontendSurfaceSnapshot,
        ): SurfaceIngressAck {
            assertEquals(SurfaceIngressStatus.PROJECT_MISMATCH, snapshotApi.publishSnapshot(lease, snapshot).status)
            return state.publish(clientId, lease, snapshot)
        }
    }

    /** A schema-5 snapshot from a Client that has not activated the release yet. */
    private fun FrontendSurfaceSnapshot.shellOnly() = copy(
        activationReceipt = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND).copy(
            jvmInstanceId = frontendActivation.jvmInstanceId,
            shellDigest = frontendActivation.shellDigest,
            spiVersion = frontendActivation.spiVersion,
        ).encode(),
    )

    private fun mismatchReportingState(
        mismatches: MutableList<FrontendPluginMismatch>,
        backendPlugin: String?,
        backendReceipt: ActivationReceipt = backendActivation,
    ) = AuthenticatedSurfaceIngressState(
        RecordingPublisher(),
        RecordingForwarder(),
        backendActivationReceipt = backendReceipt,
        scheduleDiagnostic = { _, _ -> },
        log = { _, _ -> },
        onFrontendPluginMismatch = { _, _, mismatch -> mismatches += mismatch },
        backendPluginVersion = { backendPlugin },
    )

    /**
     * GH #440 (Coder, 2026-11-24): a 0.2.1046 Client refused the 0.2.1052 backend's release shell
     * 33 times and published schema 1. The backend blamed an unanswered negotiation and said the
     * Client would ask again, which an older shell never can. The Client's whole-package polls
     * (it never asks for the release identity) prove it predates 0.2.1052, so the shell refusal,
     * the version comparison and the ZIP install lead instead.
     */
    @Test
    fun aClientThatPollsTheWholeReleasePackageLeadsWithTheShellRefusal() = runBlocking {
        val mismatches = mutableListOf<FrontendPluginMismatch>()
        val evidence = mutableMapOf<String, FrontendClientShellEvidence>()
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            RecordingForwarder(),
            backendActivationReceipt = backendActivation.copy(shellDigest = BACKEND_SHELL),
            clientShellEvidence = FrontendClientShellEvidenceSource { evidence[it] },
            backendPluginVersion = { "0.2.1052" },
            scheduleDiagnostic = { _, _ -> },
            log = { _, _ -> },
            onFrontendPluginMismatch = { _, _, mismatch -> mismatches += mismatch },
        )
        val lease = state.open("client-a", "frontend-a")
        // The Client's first snapshot can arrive before its first release poll.
        state.publish("client-a", lease, snapshot("frontend-a", 1).forSchema(1))
        assertTrue(mismatches.single().unansweredNegotiation, "no evidence yet: the negotiation story")

        evidence["client-a"] = FrontendClientShellEvidence(legacyPackagePolls = 2)
        state.publish("client-a", lease, snapshot("frontend-a", 2).forSchema(1))
        state.publish("client-a", lease, snapshot("frontend-a", 3).forSchema(1))
        assertEquals(2, mismatches.size, "the shell refusal is reported once, after the negotiation story")
        val refusal = mismatches.last()
        assertFalse(refusal.unansweredNegotiation)
        assertTrue(
            refusal.detail.startsWith(
                "client_shell=unreported backend_shell=1265693a728a client_plugin=before_0.2.1052 backend_plugin=0.2.1052",
            ),
            refusal.detail,
        )
        // GH #440 review (cycle 1): since GH #435 the backend withholds the release from this Client,
        // so the story names the backend's withholding line; the Client never receives the release
        // and never logs `[frontend-impl] refused:`.
        assertEquals(
            "the JetBrains Client polled this backend's whole frontend release 2 time(s) without asking " +
                "for its identity, which only Agent Doc Clients before 0.2.1052 do; their shell is older than " +
                "this backend's, so this backend withholds the release from that Client (backend idea.log: " +
                "`[frontend-impl] release withheld client=... legacy_poll=true`) and the Client cannot load it. " +
                "Asking for the schema again or reconnecting cannot change the Client's shell; only this " +
                "backend's plugin in the Client can",
            frontendShellRefusalStory(refusal.shellRefusal!!),
        )
        assertTrue(refusal.detail.contains("withholds the release from that Client"), refusal.detail)
        assertTrue(refusal.detail.contains("legacy_poll=true"), refusal.detail)
        assertFalse(refusal.detail.contains("refused:"), refusal.detail)
        assertFalse(refusal.detail.contains("fetched"), refusal.detail)
        assertTrue(refusal.detail.contains("cannot change the Client's shell"), refusal.detail)
        // `agent-doc upgrade` reads `frontend_schema=1` as the reconnect case.
        assertFalse(refusal.detail.contains("frontend_schema="), refusal.detail)

        val title = frontendPluginMismatchTitle(refusal, "0.2.1052")
        assertEquals(
            "Agent Doc: the JetBrains Client plugin is older than the backend's 0.2.1052: update the Client plugin",
            title,
        )
        val remedy = frontendPluginMismatchRemedy(refusal, "0.2.1052", "IntelliJIdea2026.3", RELEASE_URL)
        assertTrue(
            remedy.startsWith(
                "update the JetBrains Client plugin: the Client plugin (older than 0.2.1052) is older than " +
                    "this backend's 0.2.1052. Replace the Client's plugins/frontend/agent-doc-jetbrains-262",
            ),
            remedy,
        )
        assertTrue(remedy.contains("agent-doc-jetbrains-262-0.2.1052.zip from $RELEASE_URL"), remedy)
        assertTrue(remedy.contains("`Loaded custom plugins: Agent Doc, 0.2.1052`"), remedy)
        assertFalse(remedy.contains("reconnect the JetBrains Client"), remedy)
        assertFalse(remedy.contains('<') || remedy.contains('>'), remedy)
    }

    /** GH #440: a Client that reported its own build names its version and folder exactly. */
    @Test
    fun aReportedOlderClientBuildNamesBothVersionsAndItsPluginFolder() {
        val clientPath = "C:\\Users\\dev\\AppData\\Roaming\\JetBrains\\IntelliJIdea2026.3\\plugins\\frontend\\agent-doc-jetbrains-262"
        val mismatch = frontendPluginMismatch(
            snapshot("frontend-a", 1).forSchema(1),
            null,
            BACKEND_SHELL,
            FrontendClientShellEvidence(CLIENT_SHELL, "0.2.1046", clientPath),
            "0.2.1052",
        )!!
        val refusal = mismatch.shellRefusal!!
        assertEquals(CLIENT_SHELL, refusal.clientShell)
        assertTrue(
            mismatch.detail.startsWith(
                "client_shell=b5741f110bfb backend_shell=1265693a728a client_plugin=0.2.1046 backend_plugin=0.2.1052",
            ),
            mismatch.detail,
        )
        assertTrue(
            mismatch.detail.contains(
                "this backend withholds its frontend release from that Client and the Client cannot load it " +
                    "(backend idea.log: `[frontend-impl] client shell mismatch ... " +
                    "client_shell=b5741f110bfb ... backend_shell=1265693a728a` or " +
                    "`[frontend-impl] client plugin mismatch ... client_shell=b5741f110bfb ... " +
                    "backend_shell=1265693a728a`; a Client from 0.2.1055 on also logs " +
                    "`[frontend-impl] client plugin mismatch compatibility=... " +
                    "backend_shell=1265693a728a client_shell=b5741f110bfb` in its idea.log). " +
                    "Asking for the schema again or reconnecting cannot change the Client's shell",
            ),
            mismatch.detail,
        )
        assertEquals(
            "Agent Doc: JetBrains Client plugin 0.2.1046 is older than the backend's 0.2.1052: update the Client plugin",
            frontendPluginMismatchTitle(mismatch, "0.2.1052"),
        )
        val remedy = frontendPluginMismatchRemedy(mismatch, "0.2.1052", "IntelliJIdea2026.3", RELEASE_URL)
        assertTrue(
            remedy.startsWith(
                "update the JetBrains Client plugin: Client plugin 0.2.1046 is older than this backend's 0.2.1052. " +
                    "Replace the Client's $clientPath with",
            ),
            remedy,
        )
        assertTrue(remedy.contains(RELEASE_URL), remedy)
        // A newer Client is named as such, not as stale.
        val newer = frontendPluginMismatch(
            snapshot("frontend-a", 1).forSchema(1),
            null,
            BACKEND_SHELL,
            FrontendClientShellEvidence(CLIENT_SHELL, "0.2.1060", null),
            "0.2.1052",
        )!!
        assertTrue(frontendPluginMismatchTitle(newer, "0.2.1052").contains("is newer than the backend's 0.2.1052"))
    }

    /**
     * GH #440 with GH #441's lease metadata: a schema-5 Client on another shell names its plugin in
     * its lease, so the report compares both versions even without a build report.
     */
    @Test
    fun aReceiptShellMismatchNamesTheLeasePluginVersion() = runBlocking {
        val mismatches = mutableListOf<FrontendPluginMismatch>()
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            RecordingForwarder(),
            backendActivationReceipt = backendActivation.copy(shellDigest = BACKEND_SHELL),
            scheduleDiagnostic = { _, _ -> },
            log = { _, _ -> },
            onFrontendPluginMismatch = { _, _, mismatch -> mismatches += mismatch },
            backendPluginVersion = { "0.2.1054" },
        )
        val frontend = frontendLeaseInstanceId("frontend-a", "0.2.1053")
        val lease = state.open("client-a", frontend)
        val olderShell = frontendActivation.copy(shellDigest = CLIENT_SHELL)
        repeat(2) { attempt ->
            state.publish("client-a", lease, snapshot(frontend, attempt + 1L).copy(activationReceipt = olderShell.encode()))
        }
        val mismatch = mismatches.single()
        assertTrue(
            mismatch.detail.startsWith(
                "client_shell=b5741f110bfb backend_shell=1265693a728a client_plugin=0.2.1053 backend_plugin=0.2.1054",
            ),
            mismatch.detail,
        )
        assertEquals(
            "Agent Doc: JetBrains Client plugin 0.2.1053 is older than the backend's 0.2.1054: update the Client plugin",
            frontendPluginMismatchTitle(mismatch, "0.2.1054"),
        )
    }

    /**
     * GH #440 review (cycle 1): a Client that names its plugin in its lease (GH #441) is 0.2.1053 or
     * newer. A 0.2.1055 Client whose release offer went unanswered falls back to the whole-package
     * poll without asking for the identity; that is not proof of a shell before 0.2.1052, so it is
     * never labeled `before_0.2.1052` or told to update an older plugin.
     */
    @Test
    fun aLegacyPollFromAClientThatNamesItsLeasePluginIsNotAnOldShell() = runBlocking {
        val schemaOne = snapshot("frontend-a", 1).forSchema(1)
        val legacy = FrontendClientShellEvidence(legacyPackagePolls = 1)
        assertEquals(null, frontendShellRefusal(schemaOne, null, BACKEND_SHELL, legacy, "0.2.1054", "0.2.1055"))
        val named = frontendPluginMismatch(schemaOne, null, BACKEND_SHELL, legacy, "0.2.1054", "0.2.1055")!!
        assertEquals(null, named.shellRefusal)
        assertTrue(named.unansweredNegotiation, named.detail)
        assertFalse(named.detail.contains("before_0.2.1052"), named.detail)
        // Without a lease version the same poll still proves an old shell.
        assertTrue(frontendPluginMismatch(schemaOne, null, BACKEND_SHELL, legacy, "0.2.1054")!!.shellRefusal!!.legacyReleasePoller)

        // Through the lease state machine: the lease names plugin 0.2.1055.
        val mismatches = mutableListOf<FrontendPluginMismatch>()
        val state = AuthenticatedSurfaceIngressState(
            RecordingPublisher(),
            RecordingForwarder(),
            backendActivationReceipt = backendActivation.copy(shellDigest = BACKEND_SHELL),
            clientShellEvidence = FrontendClientShellEvidenceSource { legacy },
            backendPluginVersion = { "0.2.1054" },
            scheduleDiagnostic = { _, _ -> },
            log = { _, _ -> },
            onFrontendPluginMismatch = { _, _, mismatch -> mismatches += mismatch },
        )
        val frontend = frontendLeaseInstanceId("frontend-a", "0.2.1055")
        val lease = state.open("client-a", frontend)
        repeat(3) { attempt -> state.publish("client-a", lease, snapshot(frontend, attempt + 1L).forSchema(1)) }
        assertTrue(mismatches.none { it.shellRefusal != null }, mismatches.toString())
        assertTrue(mismatches.none { it.detail.contains("before_0.2.1052") }, mismatches.toString())
    }

    /**
     * GH #440: matching shells keep the GH #435 story. A schema-1 lease from a Client on this
     * backend's shell is an unanswered negotiation, repaired by asking again.
     */
    @Test
    fun matchingShellsKeepTheUnansweredNegotiationStory() {
        val schemaOne = snapshot("frontend-a", 1).forSchema(1)
        val sameShell = frontendPluginMismatch(
            schemaOne,
            null,
            BACKEND_SHELL,
            FrontendClientShellEvidence(BACKEND_SHELL, "0.2.1052", null),
            "0.2.1052",
        )!!
        assertTrue(sameShell.unansweredNegotiation)
        assertEquals(null, sameShell.shellRefusal)
        assertTrue(
            frontendPluginMismatchRemedy(sameShell, "0.2.1052", "IntelliJIdea2026.3")
                .startsWith("reconnect the JetBrains Client"),
        )
        // An unbuilt identity on either side proves no refusal.
        val unknown = FrontendClientShellEvidence("unknown", "0.2.1046", null)
        assertTrue(frontendPluginMismatch(schemaOne, null, BACKEND_SHELL, unknown, "0.2.1052")!!.unansweredNegotiation)
        val legacy = FrontendClientShellEvidence(legacyPackagePolls = 1)
        assertTrue(frontendPluginMismatch(schemaOne, null, "unknown", legacy, "0.2.1052")!!.unansweredNegotiation)
    }

    @Test
    fun frontendPluginMismatchNamesTheStaleSide() {
        val current = snapshot("frontend-a", 1)
        val sameBuild = frontendActivation.copy(shellDigest = "a".repeat(64))
        assertEquals(null, frontendPluginMismatch(current.copy(activationReceipt = sameBuild.encode()), sameBuild, "a".repeat(64)))
        val older = frontendPluginMismatch(current.copy(schemaVersion = 4), null, "a".repeat(64))!!
        assertTrue(older.detail.contains("frontend_schema=4 backend_schema=5"), older.detail)
        assertFalse(older.unansweredNegotiation, "a frontend that negotiated schema 4 is an older build")
        assertTrue(frontendPluginMismatch(current, null, "a".repeat(64))!!.detail.contains("frontend_receipt=unreadable"))
        assertEquals(null, frontendPluginMismatch(current, sameBuild, "unknown"), "an unbuilt identity proves nothing")
        val line = frontendPluginMismatchLine(
            "client-a",
            3,
            "frontend_shell=x backend_shell=y",
            frontendPluginMismatchRemedy("0.2.1043", "IntelliJIdea2026.3"),
        )
        assertTrue(line.contains("frontend/backend plugin mismatch client=client-a generation=3"), line)
        assertTrue(line.contains("agent-doc-jetbrains-263"), line)
        assertTrue(line.contains("start the Client again"), line)
        // GH #439: the full asset name, the thin-client directory and the trust file; no `<...>`
        // placeholder, which the HTML notification body rendered as `agent-doc-jetbrains-262-.zip`.
        assertTrue(line.contains("agent-doc-jetbrains-262-0.2.1043.zip"), line)
        assertTrue(line.contains("%APPDATA%\\JetBrains\\IntelliJIdea2026.3\\plugins\\frontend on Windows"), line)
        assertTrue(
            line.contains("~/Library/Application Support/JetBrains/IntelliJIdea2026.3/plugins/frontend on macOS"),
            line,
        )
        // Review 7118503929: IntelliJ's Linux plugins path has no `plugins` segment.
        assertTrue(line.contains("~/.local/share/JetBrains/IntelliJIdea2026.3/frontend on Linux"), line)
        assertFalse(line.contains("IntelliJIdea2026.3/plugins/frontend in its"), line)
        assertTrue(line.contains("delete every agent-doc-jetbrains, agent-doc-jetbrains-262"), line)
        assertTrue(line.contains(".agent-doc-frontend-release/trusted-ed25519-public-keys"), line)
        assertFalse(line.contains('<') || line.contains('>'), line)
        val unknown = frontendPluginMismatchRemedy("unknown", null)
        assertFalse(unknown.contains("262-unknown") || unknown.contains('<'), unknown)
    }

    @Test
    fun publicationStateIsOneCompactLogField() {
        assertEquals("forwarded", publicationState("forwarded", null))
        assertEquals("snapshot_refused(DELIVERY_FAILED)", publicationState("snapshot_refused", "DELIVERY_FAILED"))
        assertEquals("held(a_b_c)", publicationState("held", " a  b\tc "))
        assertEquals(96, publicationState("x", "y".repeat(500)).removePrefix("x(").removeSuffix(")").length)
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

    private companion object {
        const val BACKEND_SHELL = "1265693a728a62b322ebd425fb86d314509817d66e541729731da946b798e85b"
        const val CLIENT_SHELL = "b5741f110bfb37fe71dfec65ae64e7cfda754b53959f59a1039b76ed11e9dbf9"
        const val RELEASE_URL =
            "https://github.com/btakita/agent-doc/releases/download/v0.35.590/agent-doc-jetbrains-262-0.2.1052.zip"
    }

    private val backendActivation = ActivationReceipt(
        role = ActivationReceipt.ROLE_BACKEND,
        jvmInstanceId = "backend-jvm",
        shellDigest = "a".repeat(64),
        spiVersion = 1,
        implVersion = "generation-2",
        implSha256 = "b".repeat(64),
        generation = 2,
        epoch = 7,
    )
    private val frontendActivation = backendActivation.copy(
        role = ActivationReceipt.ROLE_FRONTEND,
        jvmInstanceId = "frontend-jvm",
        generation = 8,
    )

    private class RecordingForwarder(private val acquiresLayout: Boolean = true) : RemoteClientLayoutForwarder {
        val opened = mutableListOf<Pair<String, Long>>()
        val forwarded = mutableListOf<Triple<String, Long, Long>>()
        val held = mutableListOf<Pair<String, Long>>()
        val retired = mutableListOf<Pair<String, Long>>()
        val refused = mutableListOf<Pair<Long, String>>()

        override fun refused(clientId: String, generation: Long, status: String, diagnostic: String?) {
            refused += generation to status
        }

        override fun open(clientId: String, generation: Long): Boolean {
            opened += clientId to generation
            return acquiresLayout
        }

        override fun forward(clientId: String, generation: Long, snapshot: FrontendSurfaceSnapshot) {
            forwarded += Triple(clientId, generation, snapshot.sequence)
        }

        override fun hold(clientId: String, generation: Long) {
            held += clientId to generation
        }

        override fun retire(clientId: String, generation: Long) {
            retired += clientId to generation
        }
    }

    private class RecordingPublisher : SurfaceSnapshotPublisher {
        var deliver = true
        var diagnostic: String? = null
        val published = mutableListOf<Triple<String, Long, Long>>()
        val retired = mutableListOf<Pair<String, Long>>()
        val receipts = mutableListOf<FrontendPresentationReceipt>()

        override suspend fun publish(
            clientId: String,
            generation: Long,
            snapshot: FrontendSurfaceSnapshot,
        ): SurfaceDeliveryResult {
            published += Triple(clientId, generation, snapshot.sequence)
            return SurfaceDeliveryResult(deliver, diagnostic.takeUnless { deliver })
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
