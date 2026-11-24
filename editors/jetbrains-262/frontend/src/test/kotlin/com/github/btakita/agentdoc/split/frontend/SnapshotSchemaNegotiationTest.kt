package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.shell.receipt.ActivationReceipt
import com.github.btakita.agentdoc.split.FrontendReleaseGeneration
import com.github.btakita.agentdoc.split.FrontendReleaseOffer
import com.github.btakita.agentdoc.split.FrontendShellCompatibility
import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking

/**
 * GH #435: on a Coder backend the Client's 0.2.1046 frontend asked for the snapshot schema while
 * its first release poll was moving the multi-megabyte package over the same Remote Dev link. The
 * 5 s negotiation went unanswered, the frontend fell back to schema 1 and kept it for the whole
 * lease. A schema-1 snapshot carries no activation receipt, so the backend refused every one
 * (`frontend_authority_absent_or_generation_mismatched`) and blamed an "older Agent Doc frontend"
 * the Client did not have.
 */
class SnapshotSchemaNegotiationTest {
    @Test
    fun anUnansweredNegotiationIsAskedAgainAfterARefusal() = runBlocking {
        val answers = ArrayDeque(listOf<Int?>(null, SURFACE_SNAPSHOT_SCHEMA_VERSION))
        var asks = 0
        val negotiation = SnapshotSchemaNegotiation { asks++; answers.removeFirst() }

        assertEquals(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.negotiate())
        assertFalse(negotiation.answered, "a timed-out negotiation is provisional, not the backend's answer")

        assertTrue(negotiation.renegotiateAfterRefusal(), "the refusal re-asks and the schema rises")
        assertEquals(SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.schema)
        assertTrue(negotiation.answered)
        assertEquals(2, asks)

        assertFalse(negotiation.renegotiateAfterRefusal(), "an answered schema is never re-asked")
        assertEquals(2, asks)
    }

    @Test
    fun anAnsweredNegotiationKeepsTheBackendsSchema() = runBlocking {
        var asks = 0
        val negotiation = SnapshotSchemaNegotiation { asks++; 4 }

        assertEquals(4, negotiation.negotiate())
        assertTrue(negotiation.answered)
        assertFalse(negotiation.renegotiateAfterRefusal())
        assertEquals(4, negotiation.schema)
        assertEquals(1, asks)
    }

    @Test
    fun aBackendThatNeverAnswersStaysOnTheLegacySchemaAndIsAskedOncePerRefusal() = runBlocking {
        var asks = 0
        val negotiation = SnapshotSchemaNegotiation { asks++; null }

        assertEquals(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.negotiate())
        repeat(3) { assertFalse(negotiation.renegotiateAfterRefusal()) }
        assertEquals(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.schema)
        assertEquals(4, asks)
    }

    @Test
    fun theReleasePollFetchesBytesOnlyForANewGeneration() {
        val poll = FrontendReleasePoll()
        val active = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND)
            .copy(implSha256 = "a".repeat(64), epoch = 3)
        val same = releaseIdentity("a", 3)
        val next = releaseIdentity("b", 4)

        assertFalse(poll.shouldFetch(null, active, nowMs = 0), "a shell-only backend has nothing to send")
        assertFalse(poll.shouldFetch(same, active, nowMs = 0), "the active generation is never re-downloaded")
        assertTrue(poll.shouldFetch(next, active, nowMs = 0))
    }

    @Test
    fun aRefusedGenerationIsRefetchedOnlyAfterTheBackoff() {
        val poll = FrontendReleasePoll()
        val active = ActivationReceipt.shellOnly(ActivationReceipt.ROLE_FRONTEND)
        val refused = releaseIdentity("c", 5)

        assertTrue(poll.shouldFetch(refused, active, nowMs = 1_000))
        poll.refused(refused, nowMs = 1_000)
        assertFalse(poll.shouldFetch(refused, active, nowMs = 1_000 + FRONTEND_REFUSED_RELEASE_REFETCH_MS - 1))
        assertTrue(poll.shouldFetch(refused, active, nowMs = 1_000 + FRONTEND_REFUSED_RELEASE_REFETCH_MS))
        val other = releaseIdentity("d", 6)
        assertTrue(poll.shouldFetch(other, active, nowMs = 1_001), "a different generation is fetched at once")
    }

    @Test
    fun negotiationLogSaysWhetherTheSchemaIsProvisional() = runBlocking {
        val unanswered = SnapshotSchemaNegotiation { null }.apply { negotiate() }
        assertTrue(unanswered.logLine().contains("version=1 backend_offered=none"), unanswered.logLine())
        assertTrue(unanswered.logLine().contains("provisional=true"), unanswered.logLine())
        val answered = SnapshotSchemaNegotiation { 5 }.apply { negotiate() }
        assertTrue(answered.logLine().contains("version=5 backend_offered=5"), answered.logLine())
        assertTrue(answered.logLine().contains("provisional=false"), answered.logLine())
    }

    /**
     * GH #435: a Client whose shell is not the backend's logs both builds once, not every 30 s,
     * and says when it matches again.
     */
    @Test
    fun aClientPluginMismatchIsLoggedOncePerDistinctOffer() {
        val poll = FrontendReleasePoll()
        val older = FrontendReleaseOffer(
            backendPluginVersion = "0.2.1053",
            backendPluginPath = "/backend/plugins/agent-doc-jetbrains-262",
            backendShellDigest = "b".repeat(64),
            compatibility = FrontendShellCompatibility.CLIENT_OLDER,
            release = releaseIdentity("e", 9),
        )
        val line = poll.offerLogLine(older)!!
        assertTrue(line.contains("client plugin mismatch compatibility=client_older"), line)
        assertTrue(line.contains("backend_plugin=0.2.1053"), line)
        assertTrue(line.contains("backend_plugin_path=/backend/plugins/agent-doc-jetbrains-262"), line)
        assertEquals(null, poll.offerLogLine(older), "the same mismatch is not repeated")
        val matched = older.copy(compatibility = FrontendShellCompatibility.SAME_SHELL)
        assertTrue(poll.offerLogLine(matched)!!.contains("matches the backend 0.2.1053 again"))
        assertEquals(null, poll.offerLogLine(matched))
    }

    /** GH #435: a backend up to 0.2.1052 has no offer API; its package is asked for once per backoff. */
    @Test
    fun aLegacyBackendIsAskedForThePackageOncePerBackoff() {
        val poll = FrontendReleasePoll()
        assertTrue(poll.shouldFetchFromLegacyBackend(1_000))
        assertFalse(poll.shouldFetchFromLegacyBackend(1_000 + 30_000), "not on the next 30 s poll")
        assertTrue(poll.shouldFetchFromLegacyBackend(1_000 + FRONTEND_REFUSED_RELEASE_REFETCH_MS))
    }

    private fun releaseIdentity(digit: String, epoch: Long) = FrontendReleaseGeneration(
        releaseVersion = "0.2.1053",
        packageUrl = "https://example.invalid/agent-doc-jetbrains-262-0.2.1053.zip",
        implSha256 = digit.repeat(64),
        shellDigest = "a".repeat(64),
        spiVersion = 1,
        epoch = epoch,
    )
}
