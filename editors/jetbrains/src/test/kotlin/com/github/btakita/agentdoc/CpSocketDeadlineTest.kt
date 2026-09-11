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
class CpSocketDeadlineTest {
    private fun source(relative: String): String =
        listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$relative"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$relative"),
        ).first { Files.exists(it) }.let { Files.readString(it) }

    @Test
    fun `controller socket requests are bounded by a watchdog that closes the channel`() {
        val client = source("CpRouteClient.kt")

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
     * longest legitimate server-side wait — reactive turn admission is 30s
     * with a live child — or it would abort routes that are still running
     * correctly, which is the failure recorded in #jbroutasync.
     */
    @Test
    fun `the socket ceiling stays above the longest legitimate server wait`() {
        val client = source("CpRouteClient.kt")
        val declaration = client
            .substringAfter("SOCKET_REQUEST_TIMEOUT_MS")
            .substringAfter("=")
            .substringBefore("\n")
        val millis = declaration.replace("_", "").replace("L", "").trim().toLong()
        assertTrue(
            "must exceed the 30s turn-admission projection await, got ${millis}ms",
            millis > 30_000,
        )
    }

    /**
     * `#ctrlacceptleg`: the command-plane ACCEPT leg is a pure enqueue — the
     * controller publishes `Accepted` and hands the work to a worker thread
     * before replying. Measured round trip against a live controller is 25-50ms.
     *
     * It must not inherit the hang-guard ceiling above. A controller binds its
     * socket before its accept loop runs, so a client connecting into that window
     * is connected and silent until its own budget expires — surfaced on
     * 2026-09-11 as `Run Agent Doc` failing after a full minute with "the
     * controller may be wedged", indistinguishable from a real wedge.
     */
    @Test
    fun `the command submit accept leg is bounded well below the hang guard ceiling`() {
        val client = source("CpRouteClient.kt")

        val declaration = client
            .substringAfter("COMMAND_SUBMIT_ACCEPT_TIMEOUT_MS")
            .substringAfter("=")
            .substringBefore("\n")
        val acceptMillis = declaration.replace("_", "").replace("L", "").trim().toLong()
        val ceilingDeclaration = client
            .substringAfter("SOCKET_REQUEST_TIMEOUT_MS")
            .substringAfter("=")
            .substringBefore("\n")
        val ceilingMillis = ceilingDeclaration.replace("_", "").replace("L", "").trim().toLong()

        assertTrue(
            "the accept leg must be bounded far below the hang guard, got ${acceptMillis}ms " +
                "against a ${ceilingMillis}ms ceiling",
            acceptMillis < ceilingMillis / 2,
        )
        assertTrue(
            "the accept leg still needs room for a slow but serving controller, got ${acceptMillis}ms",
            acceptMillis >= 5_000,
        )
        assertTrue(
            "the accept leg must pass its own ceiling, not the shared hang guard",
            client.contains("timeoutMs = COMMAND_SUBMIT_ACCEPT_TIMEOUT_MS"),
        )
    }

    /**
     * A bound-but-not-serving controller is operationally the same as no
     * controller: this client cannot be served. It must take the same
     * `#rebootselfheal` recovery instead of reporting a dead minute — but only on
     * a leg that has no legitimate server-side wait, or a route that is still
     * running correctly would be aborted (`#jbroutasync`).
     */
    @Test
    fun `an unacknowledged accept leg self-heals instead of reporting a wedge`() {
        val client = source("CpRouteClient.kt")

        assertTrue(
            "the timeout self-heal must be opt-in per leg",
            client.contains("selfHealOnTimeout: Boolean = false") &&
                client.contains("selfHealOnTimeout = true"),
        )
        assertTrue(
            "an unacknowledged request must be distinguished from a failed connect",
            client.contains("provesControllerDidNotAcknowledge"),
        )
        assertTrue(
            "the self-heal must stay a single delegation to the shared library",
            client.contains("agent_doc_ensure_controller_running"),
        )

        // The terminal leg genuinely waits on server-side work and must keep the
        // full ceiling with no timeout self-heal.
        val terminalLeg = client.substringAfter("private fun awaitCommandSubmitTerminal(")
        assertTrue(
            "the terminal leg must not inherit the accept leg's short ceiling",
            !terminalLeg.substringBefore("\n}").contains("COMMAND_SUBMIT_ACCEPT_TIMEOUT_MS"),
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
