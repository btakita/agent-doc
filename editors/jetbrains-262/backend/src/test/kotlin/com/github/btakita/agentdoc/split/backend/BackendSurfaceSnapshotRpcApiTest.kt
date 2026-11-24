package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendImplementationRelease
import com.github.btakita.agentdoc.split.FrontendImplementationReleaseIdentity
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlinx.coroutines.runBlocking

/**
 * GH #435: the Client's 30 s release poll asks for the selected generation's identity, so the
 * multi-megabyte package crosses the Remote Dev link only when the Client must activate it.
 */
class BackendSurfaceSnapshotRpcApiTest {
    @Test
    fun releaseIdentityNamesTheSelectedGenerationWithoutItsBytes() = runBlocking {
        var discoveries = 0
        val release = FrontendImplementationRelease(
            releaseVersion = "0.2.1052",
            packageUrl = "https://example.invalid/agent-doc.zip",
            signatureUrl = "https://example.invalid/agent-doc.zip.sig",
            implEntry = "lib/impl.jar",
            implSha256 = "e".repeat(64),
            shellDigest = "a".repeat(64),
            spiVersion = 1,
            epoch = 9,
            packageBytes = ByteArray(4 * 1024 * 1024),
            signatureBytes = ByteArray(64),
        )
        val api = BackendSurfaceSnapshotRpcApi(
            frontendRelease = { discoveries++; release },
            releasePolls = FrontendReleasePollLedger(),
            callerClientId = { "client-a" },
        )

        assertEquals(FrontendImplementationReleaseIdentity("e".repeat(64), 9), api.frontendImplementationReleaseIdentity())
        assertEquals(1, discoveries, "the identity uses the same discovery (and backend peer activation)")
        assertNull(
            BackendSurfaceSnapshotRpcApi(
                frontendRelease = { null },
                releasePolls = FrontendReleasePollLedger(),
                callerClientId = { "client-a" },
            ).frontendImplementationReleaseIdentity(),
        )
    }
}
