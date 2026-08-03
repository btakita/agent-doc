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
