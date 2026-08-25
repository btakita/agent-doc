package com.github.btakita.agentdoc

import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** `#jbrecyclecommandreplay`: Run Agent Doc clicked during controller recycle. */
class CpControllerHandoffReplayTest {
    @Test
    fun `connection reset waits and replays the exact operation once`() {
        var calls = 0
        var waits = 0
        var replayed: Throwable? = null

        val result =
            CpRouteClient.replayIdempotentControllerCommandOnce(
                operation = {
                    calls += 1
                    if (calls == 1) throw IOException("Connection reset")
                    "applied"
                },
                waitForReplacement = {
                    waits += 1
                    true
                },
                onReplay = { replayed = it },
            )

        assertEquals("applied", result)
        assertEquals(2, calls)
        assertEquals(1, waits)
        assertTrue(replayed is IOException)
    }

    @Test
    fun `a semantic command failure is not replayed`() {
        val semantic = IllegalStateException("editor route rejected: queue paused")
        var calls = 0
        var waits = 0

        try {
            CpRouteClient.replayIdempotentControllerCommandOnce(
                operation = {
                    calls += 1
                    throw semantic
                },
                waitForReplacement = {
                    waits += 1
                    true
                },
            )
            fail("semantic failure should remain terminal")
        } catch (actual: IllegalStateException) {
            assertSame(semantic, actual)
        }
        assertEquals(1, calls)
        assertEquals(0, waits)
    }

    @Test
    fun `a second handoff drop is terminal after one replay`() {
        var calls = 0
        var waits = 0

        try {
            CpRouteClient.replayIdempotentControllerCommandOnce<Unit>(
                operation = {
                    calls += 1
                    throw IOException("Broken pipe attempt=$calls")
                },
                waitForReplacement = {
                    waits += 1
                    true
                },
            )
            fail("the replay budget is exactly one")
        } catch (actual: IOException) {
            assertTrue(actual.message!!.contains("attempt=2"))
        }
        assertEquals(2, calls)
        assertEquals(1, waits)
    }

    @Test
    fun `handoff drop classification stays narrow`() {
        for (
            replaySafe in
                listOf(
                    IOException("Connection reset by peer"),
                    IOException("Broken pipe"),
                    java.io.EOFException("unexpected EOF"),
                    IllegalStateException("Project Controller returned an empty response"),
                    IllegalStateException("wrapped", IOException("Connection aborted")),
                )
        ) {
            assertTrue("expected replay-safe: $replaySafe", CpRouteClient.isReplaySafeControllerHandoffDrop(replaySafe))
        }

        for (
            terminal in
                listOf(
                    java.net.ConnectException("Connection refused"),
                    IllegalStateException("Project Controller did not respond within 60000ms"),
                    IllegalStateException("editor route rejected"),
                )
        ) {
            assertFalse("expected terminal: $terminal", CpRouteClient.isReplaySafeControllerHandoffDrop(terminal))
        }
    }
}
