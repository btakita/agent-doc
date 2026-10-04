package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** `editoractionmenu`: About Agent Doc parsing, mismatch warnings, and report text. */
class AboutAgentDocActionTest {
    private val cliJson =
        """
        {"schema":"agent-doc-build-info-v1","component":"binary","version":"0.35.451",
         "build_id":"0.35.451+abc","executable":"/home/u/.cargo/bin/agent-doc",
         "library":"/home/u/.cargo/bin/libagent_doc.so",
         "expected_plugins":{"jetbrains":"0.2.487","vscode":"0.2.79","zed":null}}
        """.trimIndent()

    private fun facts(
        plugin: String = "0.2.487",
        cli: AgentDocBuildFacts? = parseBuildInfoJson(cliJson),
        native: AgentDocBuildFacts? =
            AgentDocBuildFacts("0.35.451", "0.35.451+abc", library = "/lib.so", expectedPluginVersion = "0.2.487"),
    ) = AboutAgentDocFacts(
        pluginVersion = plugin,
        cliCommand = "/home/u/.cargo/bin/agent-doc",
        cli = cli,
        cliNote = null,
        nativePath = native?.library,
        native = native,
        nativeNote = null,
    )

    @Test
    fun `parses build info json for the jetbrains expectation`() {
        val parsed = parseBuildInfoJson(cliJson)!!
        assertEquals("0.35.451", parsed.version)
        assertEquals("0.35.451+abc", parsed.buildId)
        assertEquals("/home/u/.cargo/bin/agent-doc", parsed.executable)
        assertEquals("/home/u/.cargo/bin/libagent_doc.so", parsed.library)
        assertEquals("0.2.487", parsed.expectedPluginVersion)
        assertNull(parseBuildInfoJson(cliJson, "zed")!!.expectedPluginVersion)
    }

    @Test
    fun `rejects output that is not the build info contract`() {
        assertNull(parseBuildInfoJson("agent-doc 0.35.451"))
        assertNull(parseBuildInfoJson("""{"version":"0.35.451"}"""))
        assertNull(parseBuildInfoJson("[]"))
    }

    @Test
    fun `parses legacy version text`() {
        assertEquals("0.35.449", parseAgentDocVersionText("agent-doc 0.35.449\n"))
        assertNull(parseAgentDocVersionText(""))
        assertNull(parseAgentDocVersionText("bash 5.2"))
        assertNull(parseAgentDocVersionText("agent-doc"))
    }

    @Test
    fun `matching plugin cli and native library report no mismatch`() {
        val f = facts()
        assertEquals(emptyList<String>(), aboutMismatches(f))
        val report = renderAboutReport(f)
        assertTrue(report, report.startsWith("Agent Doc plugin (JetBrains): 0.2.487"))
        assertTrue(report, report.contains("agent-doc CLI: 0.35.451"))
        assertTrue(report, report.contains("build id: 0.35.451+abc"))
        assertTrue(report, report.contains("Native library: 0.35.451"))
        assertTrue(report, report.endsWith("Plugin, CLI, and native library agree."))
    }

    @Test
    fun `warns when the cli expects a different plugin version`() {
        val mismatches = aboutMismatches(facts(plugin = "0.2.486"))
        assertEquals(1, mismatches.size)
        assertTrue(mismatches[0], mismatches[0].contains("Plugin 0.2.486"))
        assertTrue(mismatches[0], mismatches[0].contains("expects (0.2.487)"))
        assertTrue(renderAboutReport(facts(plugin = "0.2.486")).contains("Version mismatch:"))
    }

    @Test
    fun `warns when cli and native library are different builds`() {
        val native = AgentDocBuildFacts("0.35.451", "0.35.451+def", expectedPluginVersion = "0.2.487")
        val mismatches = aboutMismatches(facts(native = native))
        assertEquals(1, mismatches.size)
        assertTrue(mismatches[0], mismatches[0].contains("0.35.451+abc"))
        assertTrue(mismatches[0], mismatches[0].contains("0.35.451+def"))
    }

    @Test
    fun `falls back to versions when a build id is unknown`() {
        val native = AgentDocBuildFacts("0.35.440")
        val mismatches = aboutMismatches(facts(native = native))
        assertEquals(1, mismatches.size)
        assertTrue(mismatches[0], mismatches[0].contains("0.35.440"))
        assertEquals(emptyList<String>(), aboutMismatches(facts(native = AgentDocBuildFacts("0.35.451"))))
    }

    @Test
    fun `unqueryable cli is a warning and an unloaded library is not`() {
        val f = facts(cli = null, native = null).copy(cliNote = "No such file", nativeNote = "not loaded yet")
        val mismatches = aboutMismatches(f)
        assertEquals(1, mismatches.size)
        assertTrue(mismatches[0], mismatches[0].contains("No such file"))
        val report = renderAboutReport(f)
        assertTrue(report, report.contains("agent-doc CLI: unavailable"))
        assertTrue(report, report.contains("Native library: not loaded"))
        assertTrue(report, report.contains("note: not loaded yet"))
    }

    @Test
    fun `native facts come from the loaded generation without build info for old libraries`() {
        val (old, oldNote) = nativeAboutFacts(NativeAboutSnapshot("/lib.so", "/shadow.so", "0.35.440", null, null))
        assertEquals("0.35.440", old!!.version)
        assertNull(old.buildId)
        assertTrue(oldNote!!.contains("build id unavailable"))

        val json = cliJson.replace("\"binary\"", "\"native_library\"")
        val (current, note) = nativeAboutFacts(NativeAboutSnapshot("/lib.so", "/shadow.so", "0.35.451", json, null))
        assertEquals("0.35.451+abc", current!!.buildId)
        assertEquals("/lib.so", current.library)
        assertEquals("0.2.487", current.expectedPluginVersion)
        assertNull(note)

        val (unloaded, why) = nativeAboutFacts(NativeAboutSnapshot(null, null, null, null, "lib-path failed"))
        assertNull(unloaded)
        assertEquals("lib-path failed", why)
    }
}
