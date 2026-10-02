package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#pluginunloadresurrect` — the 2026-09-28 dynamic upgrade to 0.2.436.
 *
 * The old classloader handled a `reload_library` intent after unload, and its
 * native-reload restart recreated a replica manager that kept attaching every
 * open document under the retired editor identity. Nested-project controllers
 * held liveness for the replacement identity and refused each registration as
 * a stale endpoint, leaving those documents detached until an IDE restart.
 */
class PluginGenerationRetirementTest {
    @After
    fun reset() = PluginGeneration.resetForTest()

    private fun source(name: String): String =
        Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
            ).first { Files.exists(it) },
        )

    @Test
    fun `a retired generation stays retired`() {
        assertFalse(PluginGeneration.retired)
        PluginGeneration.retire()
        assertTrue(PluginGeneration.retired)
        PluginGeneration.retire()
        assertTrue(PluginGeneration.retired)
    }

    @Test
    fun `a retired generation reloads nothing`() {
        PluginGeneration.retire()
        // Returns before touching the reload gate or the application: a retired
        // generation must not even begin a handoff.
        NativeReloadCoordinator.requestReload(trigger = "test")
        assertTrue(NativeReloadCoordinator.awaitReady(1))
    }

    @Test
    fun `generation resources close once in reverse creation order`() {
        val closed = mutableListOf<String>()
        PluginGeneration.registerResource("first") { closed += "first" }
        PluginGeneration.registerResource("second") { closed += "second" }
        PluginGeneration.registerResource("second") { closed += "duplicate" }

        PluginGeneration.closeResources()
        PluginGeneration.closeResources()
        PluginGeneration.registerResource("late") { closed += "late" }

        assertEquals(listOf("second", "first", "late"), closed)
        assertTrue(PluginGeneration.resourcesClosed)
    }

    @Test
    fun `unload retires before disposing and every rebuild path checks the latch`() {
        val lifecycle = source("PluginLifecycleListener.kt")
        val cleanup = lifecycle.substringAfter("class PluginUnloadCleanupService")
        val retire = cleanup.indexOf("PluginGeneration.retire()")
        val dispose = cleanup.indexOf("disposeProjectResources(project)")
        assertTrue("unload must retire the generation", retire >= 0)
        assertTrue("retire must precede disposal", dispose > retire)
        val closeResources = cleanup.indexOf("PluginGeneration.closeResources()")
        assertTrue("global resources close only after project teardown", closeResources > dispose)

        val getInstance = source("CrdtReplicaManager.kt")
            .substringAfter("fun getInstance(project: Project): CrdtReplicaManager")
            .substringBefore("fun disposeProject(")
        assertTrue(getInstance.indexOf("PluginGeneration.retired") in 0 until getInstance.indexOf("getOrPut"))

        val reload = source("NativeReloadCoordinator.kt")
            .substringAfter("fun requestReload(")
        assertTrue(reload.indexOf("PluginGeneration.retired") in 0 until reload.indexOf("reloadGate.begin()"))
    }

    @Test
    fun `every classloader global worker has an unload owner`() {
        val lifecycle = source("PluginLifecycleListener.kt")
        assertTrue(lifecycle.contains("resources.values.toList().asReversed()"))

        val owners =
            mapOf(
                "NativeLib.kt" to "registerResource(\"native-generation\")",
                "TypingTracker.kt" to "registerResource(\"current-document-reporter\")",
                "CpRouteClient.kt" to "registerResource(\"cp-socket-watchdog\")",
                "RunAgentDocAttemptLedger.kt" to "registerResource(\"run-attempt-ledger\")",
            )
        owners.forEach { (file, registration) ->
            assertTrue("$file must register its classloader-owned worker", source(file).contains(registration))
        }

        val native = source("NativeLib.kt")
        assertTrue(native.contains("Runtime.getRuntime().removeShutdownHook(hook)"))
        assertTrue(native.contains("executor.shutdownNow()"))
        val get = native.substringAfter("fun get(): AgentDocLib?")
        assertTrue(get.indexOf("PluginGeneration.resourcesClosed") in 0 until get.indexOf("val current = instance"))
    }
}
