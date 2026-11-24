package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking

/**
 * GH #440: the shell RPC layer tells the ingress which JetBrains Client refuses this backend's
 * frontend release. A Client before 0.2.1052 fetches the whole package without asking for its
 * identity; every later Client asks for the identity first.
 */
class FrontendReleasePollLedgerTest {
    @Test
    fun onlyAClientThatNeverAskedForTheIdentityIsALegacyPoller() {
        val ledger = FrontendReleasePollLedger()
        assertNull(ledger.evidence("unseen"))

        ledger.packageFetched("old")
        ledger.packageFetched("old")
        val old = ledger.evidence("old")!!
        assertEquals(2, old.legacyPackagePolls)
        assertTrue(old.legacyReleasePoller)

        ledger.identityAsked("current")
        ledger.packageFetched("current")
        assertNull(ledger.evidence("current"), "a Client that asked for the identity proves nothing")

        ledger.identityAsked("idle")
        assertNull(ledger.evidence("idle"))
    }

    @Test
    fun aReportedBuildIsTheEvidence() {
        val ledger = FrontendReleasePollLedger()
        ledger.packageFetched("client")
        ledger.clientBuild("client", "b".repeat(64), "0.2.1053", "/plugins/frontend/agent-doc-jetbrains-262")
        val evidence = ledger.evidence("client")!!
        assertEquals("b".repeat(64), evidence.shellDigest)
        assertEquals("0.2.1053", evidence.pluginVersion)
        assertFalse(evidence.legacyReleasePoller, "a Client that reports its build is not a legacy poller")
    }

    @Test
    fun theLedgerKeepsOnlyTheNewestClients() {
        var now = 0L
        val ledger = FrontendReleasePollLedger(clock = { now++ }, maxClients = 2)
        ledger.packageFetched("a")
        ledger.packageFetched("b")
        ledger.packageFetched("c")
        assertNull(ledger.evidence("a"))
        assertTrue(ledger.evidence("c")!!.legacyReleasePoller)
    }

    @Test
    fun theRpcRecordsWhichReleaseCallEachClientMakes() = runBlocking {
        val ledger = FrontendReleasePollLedger()
        var caller = "old-client"
        val release = FrontendImplementationRelease(
            releaseVersion = "0.2.1052",
            packageUrl = "https://example.invalid/agent-doc.zip",
            signatureUrl = "https://example.invalid/agent-doc.zip.sig",
            implEntry = "impl/impl.jar",
            implSha256 = "e".repeat(64),
            shellDigest = "a".repeat(64),
            spiVersion = 1,
            epoch = 9,
            packageBytes = ByteArray(16),
            signatureBytes = ByteArray(64),
        )
        val api = BackendSurfaceSnapshotRpcApi(
            frontendRelease = { release },
            releasePolls = ledger,
            callerClientId = { caller },
        )
        api.frontendImplementationRelease()
        assertTrue(ledger.evidence("old-client")!!.legacyReleasePoller)

        caller = "new-client"
        api.frontendImplementationReleaseIdentity()
        api.frontendImplementationRelease()
        assertNull(ledger.evidence("new-client"))

        caller = "shell-only"
        BackendSurfaceSnapshotRpcApi(frontendRelease = { null }, releasePolls = ledger, callerClientId = { caller })
            .frontendImplementationRelease()
        assertNull(ledger.evidence("shell-only"), "no package moved, so nothing was refused")
    }
}
