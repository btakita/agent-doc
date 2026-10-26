package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PluginRestartRequiredNotifierTest {
    /** The marker the 2026-10-26 v0.35.556 install left beside the IU-263.7307 plugin tree. */
    private val modularRefusalMarker =
        "dynamic upgrade unavailable: the exact-263 modular JetBrains package has no restart-free " +
            "dynamic upgrade entry point: the 2026-10-19 split-mode IU 263.7031.23 probe kept Agent Doc " +
            "included in the module graph.\n"

    @Test
    fun `installed jar version reads every package line and ignores module jars`() {
        assertEquals("0.2.840", installedPluginJarVersion(listOf("agent.doc-0.2.840.jar", "modules")))
        assertEquals(
            "0.2.839",
            installedPluginJarVersion(listOf("agent-doc-jetbrains-0.2.839.jar", "lazily-0.35.0.jar")),
        )
        assertEquals("0.2.512", installedPluginJarVersion(listOf("agent-doc-jetbrains-263-0.2.512.jar")))
        assertNull(installedPluginJarVersion(listOf("agent.doc.backend.jar", "agent.doc.shared.jar")))
        assertNull(installedPluginJarVersion(listOf("agent.doc-0.2.x.jar", "agent.doc-.jar")))
        assertEquals(
            "0.2.840",
            installedPluginJarVersion(listOf("agent.doc-0.2.835.jar", "agent.doc-0.2.840.jar", "agent.doc-0.2.84.jar")),
        )
    }

    @Test
    fun `a replaced modular tree under a running older generation requires a restart with its reason`() {
        val verdict = pluginRestartRequiredVerdict(
            loadedVersion = "0.2.835",
            installedJarVersion = "0.2.840",
            markerBody = modularRefusalMarker,
        )!!
        assertEquals("0.2.835", verdict.loadedVersion)
        assertEquals("0.2.840", verdict.pendingVersion)
        assertEquals("installed_jar", verdict.source)
        assertTrue(verdict.reason!!.startsWith("dynamic upgrade unavailable: the exact-263 modular"))

        val message = pluginRestartRequiredMessage(verdict)
        assertTrue(message, message.startsWith("Agent Doc v0.2.840 is installed, but this IDE is still running v0.2.835."))
        assertTrue(message, message.contains("agent-doc could not swap it into the running IDE: dynamic upgrade unavailable"))
        assertTrue(message, message.endsWith("Restart the IDE to load it."))
    }

    @Test
    fun `a downgrade on disk also needs the restart`() {
        val verdict = pluginRestartRequiredVerdict("0.2.840", "0.2.839", null)!!
        assertEquals("0.2.839", verdict.pendingVersion)
        assertNull(verdict.reason)
        assertEquals(
            "Agent Doc v0.2.839 is installed, but this IDE is still running v0.2.840.\nRestart the IDE to load it.",
            pluginRestartRequiredMessage(verdict),
        )
    }

    @Test
    fun `a stale marker never prompts once the running generation matches the installed tree`() {
        // Observed after the 17:15 restart: 0.2.840 loaded, the 16:46 marker still on disk.
        assertNull(pluginRestartRequiredVerdict("0.2.840", "0.2.840", modularRefusalMarker))
        assertNull(
            pluginRestartRequiredVerdict(
                "0.2.840",
                "0.2.840",
                "staged for restart: x\nstaged_version=0.2.838\nprevious_version=0.2.835\n",
            ),
        )
    }

    @Test
    fun `a staged package newer than the running generation requires a restart`() {
        val verdict = pluginRestartRequiredVerdict(
            loadedVersion = "0.2.835",
            installedJarVersion = "0.2.835",
            markerBody = "dynamic upgrade unavailable, staged for restart: refused\n" +
                "staged_version=0.2.840\nprevious_version=0.2.835\n",
        )!!
        assertEquals("staged", verdict.source)
        assertEquals("0.2.840", verdict.pendingVersion)
        assertEquals("dynamic upgrade unavailable, staged for restart: refused", verdict.reason)
        assertTrue(pluginRestartRequiredMessage(verdict).startsWith("Agent Doc v0.2.840 is staged for the next IDE start"))
    }

    @Test
    fun `an unknown running version or an absent plugin tree is never a restart verdict`() {
        assertNull(pluginRestartRequiredVerdict(null, "0.2.840", modularRefusalMarker))
        assertNull(pluginRestartRequiredVerdict("unknown", "0.2.840", null))
        assertNull(pluginRestartRequiredVerdict("0.2.835", null, modularRefusalMarker))
        assertNull(pluginRestartRequiredVerdict("0.2.835", null, null))
    }

    @Test
    fun `a long installer reason is shortened in the balloon`() {
        val verdict = PluginRestartRequired("0.2.835", "0.2.840", "installed_jar", "x".repeat(1000))
        val reasonLine = pluginRestartRequiredMessage(verdict).lines()[1]
        assertTrue(reasonLine, reasonLine.endsWith("..."))
        assertTrue(reasonLine, reasonLine.length < 320)
    }
}
