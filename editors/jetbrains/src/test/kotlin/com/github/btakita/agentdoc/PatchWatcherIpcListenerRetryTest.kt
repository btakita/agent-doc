package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

class PatchWatcherIpcListenerRetryTest {
    @Test
    fun `listener retry backs off and remains bounded`() {
        assertEquals(250L, socketListenerRetryDelayMsUtil(0))
        assertEquals(250L, socketListenerRetryDelayMsUtil(1))
        assertEquals(500L, socketListenerRetryDelayMsUtil(2))
        assertEquals(1_000L, socketListenerRetryDelayMsUtil(3))
        assertEquals(30_000L, socketListenerRetryDelayMsUtil(20))
        assertEquals(30_000L, socketListenerRetryDelayMsUtil(Int.MAX_VALUE))
    }

    @Test
    fun `failed dynamic reload bind is retried without retaining a false listener`() {
        val source = Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            ).first { Files.exists(it) },
        )
        val start = source
            .substringAfter("private fun startSocketListenerViaFfi(state: RootState)")
            .substringBefore("private fun socketListenerCanStart")
        val retry = source
            .substringAfter("private fun scheduleSocketListenerRetry(state: RootState")
            .substringBefore("private fun dispatchSocketMessage")

        assertTrue(
            "a failed v1 bind must not masquerade as a live callback",
            start.indexOf("state.ipcCallback = callback") >
                start.indexOf("if (started)"),
        )
        assertTrue(start.contains("state.ipcCallback = null"))
        assertTrue(start.contains("scheduleSocketListenerRetry(state, \"bind_failed\")"))
        assertTrue(
            "one scheduled retry per root prevents a failure loop",
            retry.contains("listenerRetryScheduled.compareAndSet(false, true)"),
        )
        assertTrue(
            "retired and reload-quiesced generations must not restart listeners",
            source.contains("!PluginGeneration.retired") &&
                source.contains("!nativeEndpointsQuiesced") &&
                source.contains("rootStates[state.root] === state") &&
                source.contains("nativeEndpointsQuiesced = true") &&
                source.contains("nativeEndpointsQuiesced = false"),
        )
    }
}
