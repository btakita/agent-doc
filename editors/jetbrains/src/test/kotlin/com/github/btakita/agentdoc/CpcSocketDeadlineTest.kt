package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#jbsockdeadline`: a blocking `SocketChannel` has no read-timeout API, so a
 * wedged controller left `readLine()` blocked forever. That stranded the route
 * thread before it could reach the `finally` that releases the RUN_AGENT_DOC
 * registry slot, so every later click deduped away — the likely mechanism behind
 * "Run Agent Doc does nothing".
 */
class CpcSocketDeadlineTest {
    private fun source(relative: String): String =
        listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$relative"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$relative"),
        ).first { Files.exists(it) }.let { Files.readString(it) }

    @Test
    fun `controller socket requests are bounded by a watchdog that closes the channel`() {
        val client = source("CpcRouteClient.kt")

        assertTrue(
            "the socket request must have a hard timeout",
            client.contains("SOCKET_REQUEST_TIMEOUT_MS"),
        )
        // Closing the channel is the ONLY way to unblock a stuck blocking read.
        assertTrue(
            "the watchdog must close the channel to unblock a stuck read",
            client.contains("socketWatchdog.schedule(") && client.contains("channel.close()"),
        )
        assertTrue(
            "the watchdog must be cancelled once the request completes",
            client.contains("watchdog.cancel(false)"),
        )
        assertTrue(
            "a timeout must be reported as a wedged controller, not a generic socket error",
            client.contains("did not respond within") && client.contains("may be wedged"),
        )
    }

    /**
     * The ceiling is a hang guard, not a latency control. It must stay above the
     * longest legitimate server-side wait — `routed_cycle_ack_timeout` is 30s
     * with a live child — or it would abort routes that are still running
     * correctly, which is the failure recorded in #jbroutasync.
     */
    @Test
    fun `the socket ceiling stays above the longest legitimate server wait`() {
        val client = source("CpcRouteClient.kt")
        val declaration = client
            .substringAfter("SOCKET_REQUEST_TIMEOUT_MS")
            .substringAfter("=")
            .substringBefore("\n")
        val millis = declaration.replace("_", "").replace("L", "").trim().toLong()
        assertTrue(
            "must exceed the 30s live-child routed_cycle_ack_timeout, got ${millis}ms",
            millis > 30_000,
        )
    }

    /**
     * The registry slot release already lives in the route thread's `finally`;
     * the leak was purely that a permanent hang never reached it. Pin that so the
     * release is not "simplified" out on the assumption the deadline covers it.
     */
    @Test
    fun `the run agent doc registry slot is released from a finally block`() {
        val terminal = source("TerminalUtil.kt")
        val finallyIdx = terminal.indexOf("} finally {")
        val completeIdx = terminal.indexOf(
            "editorCommandRegistry.complete(routeKey, EditorCommandKind.RUN_AGENT_DOC)",
            finallyIdx,
        )
        assertTrue("the route thread must have a finally block", finallyIdx >= 0)
        assertTrue(
            "the RUN_AGENT_DOC slot must be released from the route thread's finally",
            completeIdx > finallyIdx,
        )
    }
}
