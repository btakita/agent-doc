package com.github.btakita.agentdoc.split.backend

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** GH #262: the selection fallback must observe Remote Dev controller sessions, not only guests. */
class BackendRemoteSessionGateTest {
    @Test
    fun remoteDevControllerSessionIsObserved() {
        assertTrue(BackendRemoteSessionGate.observes("CONTROLLER"))
    }

    @Test
    fun codeWithMeGuestSessionIsStillObserved() {
        assertTrue(BackendRemoteSessionGate.observes("GUEST"))
    }

    @Test
    fun onlyRemoteDevControllerSelectionProvesFocus() {
        assertTrue(BackendRemoteSessionGate.selectionProvesFocus("CONTROLLER"))
        assertFalse(BackendRemoteSessionGate.selectionProvesFocus("GUEST"))
        assertFalse(BackendRemoteSessionGate.selectionProvesFocus("LOCAL"))
        assertFalse(BackendRemoteSessionGate.selectionProvesFocus("FRONTEND"))
    }

    @Test
    fun localAndFrontendSessionsAreNotObserved() {
        assertFalse(BackendRemoteSessionGate.observes("LOCAL"))
        assertFalse(BackendRemoteSessionGate.observes("FRONTEND"))
    }

    @Test
    fun authenticatedLocalFrontendDoesNotOwnRemotePaneLayout() {
        val sessions = listOf(
            "Host" to "LOCAL",
            "remote-1" to "CONTROLLER",
        )

        assertFalse(BackendRemoteSessionGate.ownsLayout("Host", sessions))
        assertTrue(BackendRemoteSessionGate.ownsLayout("remote-1", sessions))
        assertFalse(BackendRemoteSessionGate.ownsLayout("missing", sessions))
    }

    @Test
    fun everyPlatformClientTypeHasAnExplicitDecision() {
        // A new platform ClientType must be classified here deliberately, not inherited silently.
        assertEquals(
            mapOf(
                "LOCAL" to false,
                "FRONTEND" to false,
                "CONTROLLER" to true,
                "GUEST" to true,
            ),
            listOf("LOCAL", "FRONTEND", "CONTROLLER", "GUEST")
                .associateWith(BackendRemoteSessionGate::observes),
        )
    }

    @Test
    fun installLineNamesObservedSessionsOrNone() {
        assertEquals("none", BackendRemoteSessionGate.describe(emptyList()))
        assertEquals(
            "[ClientId(a):CONTROLLER,ClientId(b):GUEST]",
            BackendRemoteSessionGate.describe(
                listOf("ClientId(a)" to "CONTROLLER", "ClientId(b)" to "GUEST"),
            ),
        )
    }
}
