package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Test

class PluginLifecycleListenerTest {

    @Test
    fun `startup does not run automatic resync audit`() {
        val source = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
        )

        assertFalse(source.contains("agent-doc\", \"resync"))
        assertFalse(source.contains("agent-doc\", \"resync\", \"--fix"))
        assertFalse(source.contains("agent-doc-resync"))
        // `#ctrlkillreregister` Tier 3: startup asks which registrations are actually
        // stranded rather than re-registering every open document. See
        // [PeerReplicaPullTest] for why the blind sweep is the wrong startup shape.
        assertTrue(
            source.contains(
                "CrdtReplicaManager.pullMissingReplicas(project, \"plugin-startup\")"
            )
        )
        assertFalse(source.contains("openFile("))
        assertFalse(source.contains("TmuxPaneFocusSync.install(project)"))

        val focusSyncSource = Files.readString(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TmuxPaneFocusSync.kt")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TmuxPaneFocusSync.kt"),
        )
        assertEquals(
            "only explicit install may construct the reverse-focus poller",
            1,
            Regex("""instances\.computeIfAbsent\(project\)""").findAll(focusSyncSource).count(),
        )
        assertTrue(focusSyncSource.contains("manager.openFile(file, false, true)"))
        assertFalse(focusSyncSource.contains("manager.openFile(file, true, true)"))
    }

    @Test
    fun `plugin package restart policy is independent of native generation hot reload`() {
        val pluginXml = Files.readString(
            Paths.get("src/main/resources/META-INF/plugin.xml")
                .takeIf { Files.exists(it) }
                ?: Paths.get("editors/jetbrains/src/main/resources/META-INF/plugin.xml")
        )

        assertTrue(pluginXml.contains("<idea-plugin require-restart=\"true\">"))
        assertFalse(pluginXml.contains("<idea-plugin require-restart=\"false\">"))
    }
}
