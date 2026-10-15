package com.github.btakita.agentdoc.split.backend

import com.intellij.openapi.client.ClientType
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** GH #262: the selection fallback must observe Remote Dev controller sessions, not only guests. */
class BackendRemoteSessionGateTest {
    @Test
    fun remoteDevControllerSessionIsObserved() {
        assertTrue(BackendRemoteSessionGate.observes(ClientType.CONTROLLER))
    }

    @Test
    fun codeWithMeGuestSessionIsStillObserved() {
        assertTrue(BackendRemoteSessionGate.observes(ClientType.GUEST))
    }

    @Test
    fun localAndFrontendSessionsAreNotObserved() {
        assertFalse(BackendRemoteSessionGate.observes(ClientType.LOCAL))
        assertFalse(BackendRemoteSessionGate.observes(ClientType.FRONTEND))
    }

    @Test
    fun everyPlatformClientTypeHasAnExplicitDecision() {
        // A new platform ClientType must be classified here deliberately, not inherited silently.
        assertEquals(
            mapOf(
                ClientType.LOCAL to false,
                ClientType.FRONTEND to false,
                ClientType.CONTROLLER to true,
                ClientType.GUEST to true,
            ),
            ClientType.entries.associateWith(BackendRemoteSessionGate::observes),
        )
    }

    @Test
    fun installLineNamesObservedSessionsOrNone() {
        assertEquals("none", BackendRemoteSessionGate.describe(emptyList()))
        assertEquals(
            "[ClientId(a):CONTROLLER,ClientId(b):GUEST]",
            BackendRemoteSessionGate.describe(
                listOf("ClientId(a)" to ClientType.CONTROLLER, "ClientId(b)" to ClientType.GUEST),
            ),
        )
    }
}
