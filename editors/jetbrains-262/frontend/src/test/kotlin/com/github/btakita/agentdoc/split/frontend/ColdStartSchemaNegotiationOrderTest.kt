package com.github.btakita.agentdoc.split.frontend

import com.github.btakita.agentdoc.split.LEGACY_SURFACE_SNAPSHOT_SCHEMA_VERSION
import com.github.btakita.agentdoc.split.SURFACE_SNAPSHOT_SCHEMA_VERSION
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
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

    private fun negotiation(link: RemoteDevLink, order: MutableList<String>) = SnapshotSchemaNegotiation {
        withTimeoutOrNull(SCHEMA_ASK_TIMEOUT_MS) {
            link.send(SCHEMA_TRANSFER_MS) { order += "schema"; SURFACE_SNAPSHOT_SCHEMA_VERSION }
        }
    }

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
        val poll = launch {
            runFrontendReleasePoll(
                gate = gate,
                gateTimeoutMs = 60_000,
                onGateTimeout = { error("the lease negotiated well inside the gate") },
                pollOnce = { link.send(PACKAGE_TRANSFER_MS) { order += "package" } },
                sleep = { awaitCancellation() },
            )
        }
        // FrontendSurfaceSnapshotService.start launches the poll before the first lease opens.
        yield()

        val negotiation = negotiation(link, order)
        assertEquals(SURFACE_SNAPSHOT_SCHEMA_VERSION, negotiation.negotiate(), "the first snapshot is schema 5")
        assertTrue(negotiation.answered)
        gate.settle()
        while (order.size < 2) delay(10)
        assertEquals(listOf("schema", "package"), order)
        poll.cancel()
    }

    @Test
    fun anUnansweredNegotiationStillReleasesThePackageFetch() = runBlocking {
        // A backend that never answers must not also keep the frontend implementation away.
        val gate = SchemaNegotiationGate()
        var polls = 0
        val poll = launch {
            runFrontendReleasePoll(gate, 60_000, onGateTimeout = {}, pollOnce = { polls++ }, sleep = { awaitCancellation() })
        }
        yield()
        assertEquals(0, polls)
        SnapshotSchemaNegotiation { null }.negotiate()
        gate.settle()
        while (polls == 0) delay(10)
        assertEquals(1, polls)
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
    }
}
