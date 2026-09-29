package com.github.btakita.agentdoc

import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

/**
 * `#reloadignorestorm`: a retired generation's `NativeLib.get()` requested a reload on every call
 * once the cdylib mtime moved, and each ignored request logged a line: ~400 lines a second on
 * 2026-09-29, rotating idea.log every ~3 minutes.
 */
class ReloadIgnoreStormTest {
    private fun source(name: String): String =
        Files.readString(
            listOf(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
                Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
            ).first { Files.exists(it) },
        )

    @Test
    fun `a retired generation never probes the cdylib mtime`() {
        val get = source("NativeLib.kt").substringAfter("fun get(): AgentDocLib?")
        val probe = get.indexOf("libMtimeChanged(path, loadedMtime)")
        val guard = get.indexOf("!PluginGeneration.retired")
        assertTrue(probe >= 0)
        assertTrue("the retired check must gate the mtime probe", guard in 0 until probe)
    }

    @Test
    fun `an ignored reload logs once per generation`() {
        val coordinator = source("NativeReloadCoordinator.kt")
        val retiredBranch = coordinator.substringAfter("if (PluginGeneration.retired) {").substringBefore("return")
        assertTrue(retiredBranch.contains("retiredReloadLogged.compareAndSet(false, true)"))
    }
}
