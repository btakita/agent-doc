package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#rebootselfheal`: the IDE must recover from a host reboot on its own.
 *
 * The plugin connects to `.agent-doc/controller.sock` directly. After a reboot
 * that connect fails in two ways that mean exactly the same thing — nothing is
 * listening — and neither is fixable by retrying the connect:
 *
 *  - `NoSuchFileException`: the socket file went away with the tmpfs.
 *  - `ECONNREFUSED`: the file outlived the process that bound it.
 *
 * Both used to surface verbatim ("Sync failed: ... Connection refused") and the
 * IDE stayed broken until a human deleted the socket by hand. Reported
 * 2026-08-03 after an X11 wedge and reboot.
 *
 * The binary already recovered from both — `connect_or_launch` adopts or
 * launches, and the bind path unlinks a stale socket file. The editors simply
 * had no way to ask, which is why this is a delegation and not a reimplementation.
 */
class CpControllerRebootSelfHealTest {
    private fun source(relative: String): String =
        listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$relative"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$relative"),
        ).first { Files.exists(it) }.let { Files.readString(it) }

    @Test
    fun `a missing socket file proves no controller is listening`() {
        assertTrue(
            CpRouteClient.provesNoControllerListening(
                java.nio.file.NoSuchFileException("/p/.agent-doc/controller.sock")
            )
        )
    }

    @Test
    fun `a refused connection proves no controller is listening`() {
        assertTrue(
            CpRouteClient.provesNoControllerListening(
                java.net.ConnectException("Connection refused")
            )
        )
    }

    /** The real shape: JNA/NIO wrap the cause rather than throwing it directly. */
    @Test
    fun `a wrapped refusal is still recognized through the cause chain`() {
        assertTrue(
            CpRouteClient.provesNoControllerListening(
                IllegalStateException(
                    "pane-layout state projection publish failed",
                    java.net.ConnectException("Connection refused"),
                )
            )
        )
    }

    /**
     * The half that matters most. Relaunching over a controller that is alive but
     * slow or unreachable is worse than reporting the error, so only positive
     * proof of death may trigger recovery — the same rule the binary's
     * `SocketLiveness` probe follows when it treats an ambiguous error as live.
     */
    @Test
    fun `an ambiguous error is never treated as proof of death`() {
        for (ambiguous in listOf(
            java.nio.file.AccessDeniedException("/p/.agent-doc/controller.sock"),
            IllegalStateException("Project Controller did not respond within 60000ms"),
            IllegalStateException("Project Controller returned an empty response"),
            java.io.IOException("Broken pipe"),
        )) {
            assertFalse(
                "an ambiguous failure must not relaunch over a possibly-live controller: $ambiguous",
                CpRouteClient.provesNoControllerListening(ambiguous),
            )
        }
    }

    /**
     * The recovery itself must stay a single delegation into the shared library.
     * A plugin that grows its own unlink/launch logic is how the editor and the
     * binary drift into disagreeing about controller liveness.
     */
    @Test
    fun `recovery delegates to the shared library instead of reimplementing it`() {
        val client = source("CpRouteClient.kt")

        assertTrue(
            "the retry must ask the shared library to ensure a controller",
            client.contains("agent_doc_ensure_controller_running("),
        )
        for (reimplementation in listOf(".delete()", "deleteIfExists", "ProcessBuilder")) {
            assertFalse(
                "the plugin must not carry its own controller socket recovery ($reimplementation)",
                client.contains(reimplementation),
            )
        }
    }

    /**
     * The replica lane must self-heal too, and this is the half I got wrong first.
     *
     * I originally excluded `CrdtReplicaForwarder` on the theory that replica
     * transport is a passive lane. It is not: replica registration is what makes
     * the editor a live authority for a document, so with every project's
     * controller dead after a reboot the register retry ladder re-attempts a
     * connect that cannot succeed — forever. Observed 2026-08-03: five projects
     * sat dead across a full IDE restart, `failure_count` climbing into the
     * teens at an 8s backoff, until a human ran `controller status --ensure` per
     * project. "I restarted but it's not recovering" was exactly this.
     */
    @Test
    fun `the replica lane recovers a dead project controller`() {
        val forwarder = source("CrdtReplicaForwarder.kt")

        assertTrue(
            "a register failure that proves no listener must ensure the controller",
            forwarder.contains("agent_doc_ensure_controller_running("),
        )
        assertTrue(
            "the dead-socket classification is shared, not re-derived here",
            forwarder.contains("CpRouteClient.provesNoControllerListening("),
        )
        assertTrue(
            "a successful send must clear the rate limit so a later death is recoverable",
            forwarder.contains("controllerEnsuredAtMs = 0L"),
        )
    }

    /**
     * The rate limit, asserted by calling it.
     *
     * The first version of this was a source scan for the constant's name, and a
     * mutation that deleted the check stayed green because the constant was still
     * named in a comment — the same hole that let an earlier guard here pass on
     * prose. A source scan can say a rule is mentioned; only a call can say it
     * holds.
     */
    @Test
    fun `the controller launch is rate limited per project root`() {
        val interval = 30_000L

        assertTrue(
            "the first failure after a successful send must be allowed to launch",
            shouldAttemptControllerLaunch(nowMs = 1_000_000L, lastAttemptMs = 0L),
        )
        assertFalse(
            "a second attempt inside the window would turn the retry ladder into a launch loop",
            shouldAttemptControllerLaunch(
                nowMs = 1_000_000L + interval - 1,
                lastAttemptMs = 1_000_000L,
            ),
        )
        assertTrue(
            "once the window elapses a genuinely dead project may be retried",
            shouldAttemptControllerLaunch(
                nowMs = 1_000_000L + interval,
                lastAttemptMs = 1_000_000L,
            ),
        )
    }

    /**
     * `editors/SPEC.md`: the passive editor-surface lane sends "over the
     * existing-controller socket; it never launches the controller". A tab click
     * must stay free, so the self-heal is confined to lanes a human asked for.
     * Putting the retry in the shared send helper — the first thing I did — would
     * have made every tab click able to start a controller.
     */
    @Test
    fun `the passive observation lane never launches a controller`() {
        val client = source("CpRouteClient.kt")

        val passiveLane = client.substringAfter("fun observeEditorSurface(")
            .substringBefore("fun observeDocumentPathTransition(")
        assertFalse(
            "the passive surface-observation lane must not use the launching send path",
            passiveLane.contains("sendOperatorRequestDataToSocket("),
        )
        assertTrue(
            "the passive lane keeps the plain, non-launching send path",
            passiveLane.contains("sendRequestDataToSocket("),
        )
    }
}
