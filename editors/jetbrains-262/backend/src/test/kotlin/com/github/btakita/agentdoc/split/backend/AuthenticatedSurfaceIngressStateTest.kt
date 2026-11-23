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
import com.github.btakita.agentdoc.split.forSchema
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
            onFrontendPluginMismatch = { client, _, detail -> mismatches += "$client $detail" },
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
        assertTrue(mismatches.single().contains("frontend_shell=dddddddddddd backend_shell=aaaaaaaaaaaa"), mismatches.toString())
    }

    @Test
    fun frontendPluginMismatchNamesTheStaleSide() {
        val current = snapshot("frontend-a", 1)
        val sameBuild = frontendActivation.copy(shellDigest = "a".repeat(64))
        assertEquals(null, frontendPluginMismatch(current.copy(activationReceipt = sameBuild.encode()), sameBuild, "a".repeat(64)))
        assertTrue(
            frontendPluginMismatch(current.copy(schemaVersion = 4), null, "a".repeat(64))!!
                .contains("frontend_schema=4 backend_schema=5"),
        )
        assertTrue(frontendPluginMismatch(current, null, "a".repeat(64))!!.contains("frontend_receipt=unreadable"))
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
        assertTrue(line.contains("%APPDATA%\\JetBrains\\IntelliJIdea2026.3\\plugins\\frontend"), line)
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
