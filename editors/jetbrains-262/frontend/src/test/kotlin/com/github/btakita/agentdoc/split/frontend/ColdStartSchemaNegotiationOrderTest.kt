package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.yield

/**
 * GH #441 (Coder, 2026-11-24, both peers 0.2.1052): on a cold start the Client's first release
 * poll moved the multi-megabyte frontend package over the Remote Dev link while the first lease
 * asked for the snapshot schema. The backend answers `supportedSnapshotSchemaVersion` from a
 * constant, but the answer queued behind the package: the 5 s negotiation went unanswered
 * (publisher 10:56:31, unanswered 10:56:46), the frontend published schema 1, and only after the
 * package landed (`frontend-impl activated` 10:57:07) did the retry reach schema 5 (10:57:30).
 *
 * These tests put both calls on one simulated link: a message waits for the one ahead of it.
 */
class ColdStartSchemaNegotiationOrderTest {
    private class RemoteDevLink {
        private val wire = Mutex()

        suspend fun <T> send(transferMs: Long, deliver: () -> T): T = wire.withLock {
            delay(transferMs)
            deliver()
        }
    }

    private fun negotiation(
        link: RemoteDevLink,
        order: MutableList<String>,
        gate: SchemaNegotiationGate? = null,
        transfersMs: ArrayDeque<Long> = ArrayDeque(),
    ) = SnapshotSchemaNegotiation(
        ask = {
            val transferMs = transfersMs.removeFirstOrNull() ?: SCHEMA_TRANSFER_MS
            withTimeoutOrNull(SCHEMA_ASK_TIMEOUT_MS) {
                link.send(transferMs) { order += "schema"; SURFACE_SNAPSHOT_SCHEMA_VERSION }
            }
        },
        answeredGate = gate,
    )

    private fun CoroutineScope.launchPoll(
        link: RemoteDevLink,
        order: MutableList<String>,
        gate: SchemaNegotiationGate,
        gateTimeoutMs: Long = 60_000,
        onGateTimeout: () -> Unit = { error("the lease was answered well inside the gate") },
    ) = launch {
        runFrontendReleasePoll(
            gate = gate,
            gateTimeoutMs = gateTimeoutMs,
            onGateTimeout = onGateTimeout,
            pollOnce = { link.send(PACKAGE_TRANSFER_MS) { order += "package" } },
            sleep = { awaitCancellation() },
        )
    }

    private suspend fun SchemaNegotiationGate.isOpen(): Boolean = await(timeoutMs = 1)

    @Test
    fun aPackageFetchStartedFirstStarvesTheSchemaAsk() = runBlocking {
        // The 0.2.1052 order, kept as the control: the poll fetches before the lease negotiates.
        val link = RemoteDevLink()
        val order = mutableListOf<String>()
        val fetch = launch { link.send(PACKAGE_TRANSFER_MS) { order += "package" } }
        yield()

        val negotiation = negotiation(link, order)
        assertEquals(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.negotiate())
        assertFalse(negotiation.answered, "the schema answer queued behind the package")
        fetch.join()
    }

    @Test
    fun theFirstLeaseNegotiatesSchemaFiveBeforeThePackageMoves() = runBlocking {
        val link = RemoteDevLink()
        val order = mutableListOf<String>()
        val gate = SchemaNegotiationGate()
        val poll = launchPoll(link, order, gate)
        // FrontendSurfaceSnapshotService.start launches the poll before the first lease opens.
        yield()

        // Wired as publishForLease wires it: the answer itself opens the gate.
        val negotiation = negotiation(link, order, gate)
        assertEquals(SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.negotiate(), "the first snapshot is schema 5")
        assertTrue(negotiation.answered)
        while (order.size < 2) delay(10)
        assertEquals(listOf("schema", "package"), order)
        poll.cancel()
    }

    /**
     * GH #441 review cycle 1, the control: the old wiring settled the gate after an unanswered ask.
     * The backend's first answer reached the Client after 5 s, the package then started, and the
     * re-ask after the refusal queued behind it, so the next snapshot was schema 1 again.
     */
    @Test
    fun settlingOnAnUnansweredAskLetsThePackageStarveTheReAsk() = runBlocking {
        val link = RemoteDevLink()
        val order = mutableListOf<String>()
        val gate = SchemaNegotiationGate()
        val poll = launchPoll(link, order, gate)
        yield()

        val negotiation = negotiation(link, order, transfersMs = ArrayDeque(listOf(LATE_ANSWER_MS)))
        assertEquals(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.negotiate())
        gate.settle()
        delay(PACKAGE_START_MS) // the package is on the wire now
        assertFalse(negotiation.renegotiateAfterRefusal(), "the re-ask queued behind the package")
        assertFalse(negotiation.answered)
        poll.cancel()
    }

    /**
     * GH #441 review cycle 1: a late first answer leaves the gate closed, so the re-ask after the
     * refusal reaches the backend before the package moves and the lease gets schema 5.
     */
    @Test
    fun aLateFirstAnswerKeepsThePackageBackUntilTheReAskIsAnswered() = runBlocking {
        val link = RemoteDevLink()
        val order = mutableListOf<String>()
        val gate = SchemaNegotiationGate()
        val poll = launchPoll(link, order, gate)
        yield()

        val negotiation = negotiation(link, order, gate, ArrayDeque(listOf(LATE_ANSWER_MS)))
        assertEquals(LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.negotiate())
        assertFalse(negotiation.answered)
        assertFalse(gate.isOpen(), "an unanswered ask must not release the package")
        delay(PACKAGE_TRANSFER_MS)
        assertEquals(emptyList(), order, "the package waits while the lease is provisional")

        assertTrue(negotiation.renegotiateAfterRefusal(), "the re-ask gets through and the schema rises")
        assertEquals(SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.schema)
        assertTrue(gate.isOpen())
        while (order.size < 2) delay(10)
        assertEquals(listOf("schema", "package"), order)
        poll.cancel()
    }

    @Test
    fun aBackendThatNeverAnswersReleasesThePackageOnlyAtTheCap() = runBlocking {
        // A backend that never answers must not also keep the frontend implementation away for good.
        val link = RemoteDevLink()
        val order = mutableListOf<String>()
        val gate = SchemaNegotiationGate()
        var timedOut = false
        val poll = launchPoll(link, order, gate, gateTimeoutMs = GATE_CAP_MS, onGateTimeout = { timedOut = true })
        yield()

        val negotiation = SnapshotSchemaNegotiation(ask = { null }, answeredGate = gate)
        negotiation.negotiate()
        repeat(2) { assertFalse(negotiation.renegotiateAfterRefusal()) }
        assertFalse(gate.isOpen(), "no answer, no settle")
        assertEquals(emptyList(), order)
        while (order.isEmpty()) delay(10)
        assertTrue(timedOut, "the 30 s cap (scaled down here) released the poll")
        assertEquals(listOf("package"), order)
        poll.cancel()
    }

    @Test
    fun aLeaseThatNeverOpensReleasesThePackageFetchAfterTheGateTimeout() = runBlocking {
        val gate = SchemaNegotiationGate()
        var timedOut = false
        var polls = 0
        val poll = launch {
            runFrontendReleasePoll(
                gate,
                gateTimeoutMs = 50,
                onGateTimeout = { timedOut = true },
                pollOnce = { polls++ },
                sleep = { awaitCancellation() },
            )
        }
        while (polls == 0) delay(10)
        assertTrue(timedOut)
        assertEquals(1, polls)
        poll.cancel()
    }

    private companion object {
        const val SCHEMA_TRANSFER_MS = 1L
        const val SCHEMA_ASK_TIMEOUT_MS = 100L
        const val PACKAGE_TRANSFER_MS = 300L

        /** The first answer is sent, but it reaches the Client after the ask's timeout. */
        const val LATE_ANSWER_MS = 250L
        const val GATE_CAP_MS = 400L
        const val PACKAGE_START_MS = 20L
    }
}
