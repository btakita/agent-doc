package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths

/** GH #339: do not turn one backend probe into a false split-mode hot-upgrade claim. */
class ModularRestartBoundaryContractTest {
    private fun repositoryFile(vararg candidates: String): Path =
        candidates
            .asSequence()
            .map(Paths::get)
            .first(Files::exists)

    @Test
    fun `exact 263 stays restart bound until both modular receipts exist`() {
        val descriptor = Files.readString(
            repositoryFile(
                "../jetbrains-262/src/main/resources/META-INF/plugin.xml",
                "editors/jetbrains-262/src/main/resources/META-INF/plugin.xml",
            ),
        )
        val jetBrainsSpec = Files.readString(
            repositoryFile("SPEC.md", "editors/jetbrains/SPEC.md"),
        )
        val sharedSpec = Files.readString(
            repositoryFile("../PLUGIN-SPEC.md", "editors/PLUGIN-SPEC.md"),
        )

        assertTrue(descriptor.contains("<module name=\"agent.doc.shared\" loading=\"required\"/>"))
        assertTrue(descriptor.contains("required-if-available=\"intellij.platform.frontend\""))
        assertTrue(descriptor.contains("required-if-available=\"intellij.platform.backend\""))

        assertTrue(jetBrainsSpec.contains("Exact-263 decision (GH #339)"))
        assertTrue(jetBrainsSpec.contains("every platform registration (actions, extensions, listeners,"))
        assertTrue(jetBrainsSpec.contains("generation's classloader becomes unreachable and is collected"))
        assertTrue(jetBrainsSpec.contains("backend/frontend activation receipts"))
        assertTrue(sharedSpec.contains("Exact-263 whole-package restart-free upgrade is explicitly unsupported"))
        assertTrue(sharedSpec.contains("generation-fenced backend/frontend activation"))

        assertFalse(
            "the modular descriptor must not advertise the classic attach launcher",
            descriptor.contains("JetBrainsPluginUpgradeBootstrap"),
        )
    }
}
