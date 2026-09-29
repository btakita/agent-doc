package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.After
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
    fun `unload retires before disposing and every rebuild path checks the latch`() {
        val lifecycle = source("PluginLifecycleListener.kt")
        val cleanup = lifecycle.substringAfter("class PluginUnloadCleanupService")
        val retire = cleanup.indexOf("PluginGeneration.retire()")
        val dispose = cleanup.indexOf("disposeProjectResources(project)")
        assertTrue("unload must retire the generation", retire >= 0)
        assertTrue("retire must precede disposal", dispose > retire)

        val getInstance = source("CrdtReplicaManager.kt")
            .substringAfter("fun getInstance(project: Project): CrdtReplicaManager")
            .substringBefore("fun disposeProject(")
        assertTrue(getInstance.indexOf("PluginGeneration.retired") in 0 until getInstance.indexOf("getOrPut"))

        val reload = source("NativeReloadCoordinator.kt")
            .substringAfter("fun requestReload(")
        assertTrue(reload.indexOf("PluginGeneration.retired") in 0 until reload.indexOf("reloadGate.begin()"))
    }
}
