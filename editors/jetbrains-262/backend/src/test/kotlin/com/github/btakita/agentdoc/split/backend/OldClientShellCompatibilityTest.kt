@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.split.FrontendClientBuild
import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.FrontendImplementationReleaseIdentity
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendReleaseRpcApi
import com.github.btakita.agentdoc.split.FrontendShellCompatibility
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SurfaceIngressAck
import com.github.btakita.agentdoc.split.SurfaceIngressLease
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi
import com.github.btakita.agentdoc.split.forSchema
import com.intellij.platform.project.ProjectId
import fleet.rpc.RemoteApi
import fleet.rpc.Rpc
import fleet.rpc.remoteApiDescriptor
import fleet.util.UID
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking

/**
 * The [SurfaceSnapshotRpcApi] a 0.2.1046 JetBrains Client was built with (`git show
 * 6c140168be:editors/jetbrains-262/shared/.../SurfaceSnapshotContract.kt`), method for method.
 * Its message types are the current ones: `SharedRpcContractGuardTest` pins them to the same
 * 0.2.1046 shapes.
 */
@Rpc
interface SurfaceSnapshotRpcApiAt0_2_1046 : RemoteApi<Unit> {
    suspend fun openIngress(projectId: ProjectId, frontendInstanceId: String): Flow<SurfaceIngressLease>

    suspend fun publishSnapshot(lease: SurfaceIngressLease, snapshot: FrontendSurfaceSnapshot): SurfaceIngressAck

    suspend fun publishPresentationReceipt(
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck

    suspend fun supportedSnapshotSchemaVersion(): Int

    suspend fun frontendImplementationRelease(): FrontendImplementationRelease?
}

/** The same API as a 0.2.1052 or 0.2.1053 Client calls it: 0.2.1046's plus the identity poll. */
@Rpc
interface SurfaceSnapshotRpcApiAt0_2_1052 : RemoteApi<Unit> {
    suspend fun openIngress(projectId: ProjectId, frontendInstanceId: String): Flow<SurfaceIngressLease>

    suspend fun publishSnapshot(lease: SurfaceIngressLease, snapshot: FrontendSurfaceSnapshot): SurfaceIngressAck

    suspend fun publishPresentationReceipt(
        lease: SurfaceIngressLease,
        receipt: FrontendPresentationReceipt,
    ): SurfaceIngressAck

    suspend fun supportedSnapshotSchemaVersion(): Int

    suspend fun frontendImplementationRelease(): FrontendImplementationRelease?

    suspend fun frontendImplementationReleaseIdentity(): FrontendImplementationReleaseIdentity?
}

/**
 * GH #435 cross-version test: a 0.2.1046 Client against this backend, over the Fleet RPC wire.
 *
 * On Coder (2026-11-24) a 0.2.1052 backend stranded a 0.2.1046 Client: the Client's release poll
 * moved the whole ZIP every 30 s (refusing it each time, because its shell differs), its 5 s schema
 * negotiation went unanswered behind that transfer, and the lease published schema 1, which the
 * activated backend refused for good.
 */
class OldClientShellCompatibilityTest {
    private val backendShell = "b".repeat(64)
    private val oldClientShell = "a".repeat(64)
    private val projectId = ProjectId(UID.random())

    private class Backend(backendShell: String) {
        var activations = 0
        val gate = FrontendReleaseDeliveryGate(backendShell = { backendShell }, clock = { now })
        val releasePolls = FrontendReleasePollLedger()
        var now = 1_000L
        var clientId = "client-1046"
        val release = FrontendImplementationRelease(
            releaseVersion = "0.2.1053",
            packageUrl = "https://github.com/btakita/agent-doc/releases/download/v0.35.591/agent-doc-jetbrains-262-0.2.1053.zip",
            signatureUrl = "https://github.com/btakita/agent-doc/releases/download/v0.35.591/agent-doc-jetbrains-262-0.2.1053.zip.sig",
            implEntry = "impl/agent-doc-frontend-impl-0.2.1053.jar",
            implSha256 = "e".repeat(64),
            shellDigest = backendShell,
            spiVersion = 1,
            epoch = 9,
            packageBytes = ByteArray(1024),
            signatureBytes = ByteArray(64),
        )
        val snapshotApi = BackendSurfaceSnapshotRpcApi(
            frontendRelease = { activations++; release },
            gate = gate,
            releasePolls = releasePolls,
            releaseSelected = { true },
            callerClientId = { clientId },
            resolveProject = { null },
        )
        val releaseApi = BackendFrontendReleaseRpcApi(
            currentRelease = { activations++; release },
            selectedRelease = { release },
            backend = {
                BackendPluginBuild("0.2.1053", "/home/coder/.local/share/JetBrains/IntelliJIdea2026.3/agent-doc-jetbrains-262", backendShell)
            },
            gate = gate,
            releasePolls = releasePolls,
            callerClientId = { clientId },
        )
        val calls = mutableListOf<String>()
        val oldClient: SurfaceSnapshotRpcApiAt0_2_1046 = fleetRpcWireClient(
            remoteApiDescriptor<SurfaceSnapshotRpcApiAt0_2_1046>(),
            remoteApiDescriptor<SurfaceSnapshotRpcApi>(),
            snapshotApi,
            onCall = { calls += it },
        )
    }

    private fun snapshot(sequence: Long, shell: String?) = FrontendSurfaceSnapshot(
        frontendInstanceId = "frontend-1046",
        sequence = sequence,
        projectId = projectId,
        complete = true,
        surfaces = listOf(FrontendSurface("root", 1, SurfaceRole.MAIN, true, emptyList())),
        activationReceipt = shell?.let {
            ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND).copy(shellDigest = it).encode()
        },
    )

    @Test
    fun everyOldClientMethodRoutesToThisBackendByNameAndDecodes() = runBlocking<Unit> {
        val backend = Backend(backendShell)
        val lease = SurfaceIngressLease("lease-1", 7)

        assertEquals(emptyList(), backend.oldClient.openIngress(projectId, "frontend-1046").toList())
        assertEquals(SURFACE_SNAPSHOT_SCHEMA_VERSION, backend.oldClient.supportedSnapshotSchemaVersion())
        val ack = backend.oldClient.publishSnapshot(lease, snapshot(1, oldClientShell))
        assertEquals(SurfaceIngressStatus.PROJECT_MISMATCH, ack.status, "no project in this unit test")
        assertEquals(7, ack.connectionGeneration)
        assertNull(backend.oldClient.frontendImplementationRelease())
        assertEquals(
            listOf(
                "openIngress",
                "supportedSnapshotSchemaVersion",
                "publishSnapshot",
                "frontendImplementationRelease",
            ),
            backend.calls,
        )
    }

    /** The Coder case: an older shell is never sent the ZIP, so nothing races its negotiation. */
    @Test
    fun anOlderShellIsNeverSentTheReleaseAndNeverActivatesTheBackendPeer() = runBlocking<Unit> {
        val backend = Backend(backendShell)
        val lease = SurfaceIngressLease("lease-1", 7)

        // The 0.2.1046 poll starts with the Client, before its lease negotiated.
        assertNull(backend.oldClient.frontendImplementationRelease(), "no bytes before the schema answer")
        assertEquals(5, backend.oldClient.supportedSnapshotSchemaVersion())
        assertEquals(
            FrontendReleaseDeliveryGate.Admission.SHELL_UNKNOWN,
            backend.gate.admission(backend.clientId),
            "the bytes still wait for the Client's shell",
        )
        assertNull(backend.oldClient.frontendImplementationRelease())
        backend.oldClient.publishSnapshot(lease, snapshot(1, oldClientShell).forSchema(5))
        assertEquals(FrontendReleaseDeliveryGate.Admission.SHELL_MISMATCH, backend.gate.admission(backend.clientId))
        repeat(33) {
            backend.now += 30_000
            assertNull(backend.oldClient.frontendImplementationRelease(), "poll $it of the 33 refused downloads")
        }
        assertEquals(0, backend.activations, "a backend no connected Client can follow stays shell-only")
        assertTrue(
            backend.releasePolls.evidence(backend.clientId)?.legacyReleasePoller == true,
            "a withheld whole-package poll still proves a pre-0.2.1052 Client to the #440 diagnostic",
        )
    }

    /** The same-version pair keeps its swap: one delivery after the schema answer, then none. */
    @Test
    fun aClientOnThisShellGetsTheReleaseOnceAfterItsSchemaAnswer() = runBlocking<Unit> {
        val backend = Backend(backendShell)
        val lease = SurfaceIngressLease("lease-1", 7)

        assertNull(backend.oldClient.frontendImplementationRelease())
        assertEquals(0, backend.activations)
        backend.oldClient.supportedSnapshotSchemaVersion()
        backend.oldClient.publishSnapshot(lease, snapshot(1, backendShell))
        assertEquals(1, backend.activations, "the backend peer activates before the fence reads this snapshot")
        val delivered = assertNotNull(backend.oldClient.frontendImplementationRelease())
        assertEquals(backend.release.implSha256, delivered.implSha256)
        assertTrue(delivered.packageBytes.contentEquals(backend.release.packageBytes), "the bytes survive the wire")
        backend.now += 30_000
        assertNull(backend.oldClient.frontendImplementationRelease(), "the same generation is not resent every 30 s")
        backend.now += FrontendReleaseDeliveryGate.RELEASE_REFETCH_AFTER_MS
        assertNotNull(backend.oldClient.frontendImplementationRelease(), "offered again after the backoff")
    }

    /**
     * A Client after this fix names its build in the additive offer. Before its schema answer the
     * backend does no release work at all; after it, only a Client on this shell activates the peer.
     */
    @Test
    fun theOfferNamesBothBuildsAndDoesNoReleaseWorkBeforeTheSchemaAnswer() = runBlocking<Unit> {
        val backend = Backend(backendShell)
        val client: FrontendReleaseRpcApi = fleetRpcWireClient(
            remoteApiDescriptor<FrontendReleaseRpcApi>(),
            remoteApiDescriptor<FrontendReleaseRpcApi>(),
            backend.releaseApi,
        )
        val windowsClient = "C:\\Users\\dev\\AppData\\Roaming\\JetBrains\\IntelliJIdea2026.3\\plugins\\frontend\\agent-doc-jetbrains-262"

        val older = client.frontendReleaseOffer(FrontendClientBuild("0.2.1052", windowsClient, oldClientShell, 1))
        assertEquals(FrontendShellCompatibility.CLIENT_OLDER, older.compatibility)
        assertEquals("0.2.1053", older.backendPluginVersion)
        assertEquals(backendShell, older.backendShellDigest)
        assertNull(older.release, "no release discovery before the schema answer")
        assertEquals(windowsClient, backend.gate.clientBuild(backend.clientId)?.pluginPath)
        assertEquals(windowsClient, backend.releasePolls.evidence(backend.clientId)?.pluginPath)
        assertEquals(oldClientShell, backend.releasePolls.evidence(backend.clientId)?.shellDigest)
        backend.snapshotApi.supportedSnapshotSchemaVersion()
        assertEquals(backend.release.implSha256, client.frontendReleaseOffer(FrontendClientBuild("0.2.1052", windowsClient, oldClientShell, 1)).release?.implSha256)
        assertEquals(0, backend.activations, "an older Client never activates the backend peer")

        backend.clientId = "client-current"
        val build = FrontendClientBuild("0.2.1053", "/client/agent-doc-jetbrains-262", backendShell, 1)
        val early = client.frontendReleaseOffer(build)
        assertEquals(FrontendShellCompatibility.SAME_SHELL, early.compatibility)
        assertNull(early.release)
        assertEquals(0, backend.activations, "not even a same-shell Client activates before its schema answer")
        backend.snapshotApi.supportedSnapshotSchemaVersion()
        assertEquals(backend.release.implSha256, client.frontendReleaseOffer(build).release?.implSha256)
        assertEquals(1, backend.activations, "a negotiated Client on this shell activates the backend peer")

        val newer = client.frontendReleaseOffer(FrontendClientBuild("0.2.1060", "/client/x", "c".repeat(64), 1))
        assertEquals(FrontendShellCompatibility.CLIENT_NEWER, newer.compatibility)
    }

    /** A 0.2.1052/0.2.1053 Client's identity poll still routes, and names nothing to fetch. */
    @Test
    fun a0_2_1052ClientIdentityPollIsAnsweredOnlyForItsAdmittedShell() = runBlocking<Unit> {
        val backend = Backend(backendShell)
        val client: SurfaceSnapshotRpcApiAt0_2_1052 = fleetRpcWireClient(
            remoteApiDescriptor<SurfaceSnapshotRpcApiAt0_2_1052>(),
            remoteApiDescriptor<SurfaceSnapshotRpcApi>(),
            backend.snapshotApi,
        )
        assertNull(client.frontendImplementationReleaseIdentity(), "nothing before the schema answer")
        assertEquals(null, backend.releasePolls.evidence(backend.clientId), "an identity poller is not legacy")
        client.supportedSnapshotSchemaVersion()
        client.publishSnapshot(SurfaceIngressLease("lease-1", 7), snapshot(1, oldClientShell))
        assertNull(client.frontendImplementationReleaseIdentity(), "an older shell is told nothing to fetch")
        assertEquals(0, backend.activations)
        backend.clientId = "same-shell"
        client.supportedSnapshotSchemaVersion()
        client.publishSnapshot(SurfaceIngressLease("lease-2", 8), snapshot(1, backendShell))
        assertEquals(
            FrontendImplementationReleaseIdentity(backend.release.implSha256, backend.release.epoch),
            client.frontendImplementationReleaseIdentity(),
        )
    }

    /**
     * The frozen API answers every method a 0.2.1046 and a 0.2.1052 Client calls, with the same
     * parameter names, and has no other method.
     */
    @Test
    fun theFrozenApiStillHasEveryShippedMethodSignature() {
        val current = remoteApiDescriptor<SurfaceSnapshotRpcApi>()
        val currentMethods = SurfaceSnapshotRpcApi::class.java.declaredMethods.map { it.name }.toSortedSet()
        for ((old, type) in listOf(
            remoteApiDescriptor<SurfaceSnapshotRpcApiAt0_2_1046>() to SurfaceSnapshotRpcApiAt0_2_1046::class.java,
            remoteApiDescriptor<SurfaceSnapshotRpcApiAt0_2_1052>() to SurfaceSnapshotRpcApiAt0_2_1052::class.java,
        )) {
            val methods = type.declaredMethods.map { it.name }.toSortedSet()
            assertTrue(currentMethods.containsAll(methods), "${type.simpleName}: $methods")
            for (method in methods) {
                assertEquals(
                    old.getSignature(method).parameters.map { it.parameterName },
                    current.getSignature(method).parameters.map { it.parameterName },
                    method,
                )
            }
        }
        assertEquals(
            SurfaceSnapshotRpcApiAt0_2_1052::class.java.declaredMethods.map { it.name }.toSortedSet(),
            currentMethods,
            "the frozen API neither lost a shipped method nor gained one",
        )
    }
}
