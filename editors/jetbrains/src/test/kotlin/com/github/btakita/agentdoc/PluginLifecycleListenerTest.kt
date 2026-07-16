package com.github.btakita.agentdoc

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
        assertTrue(
            source.contains(
                "CrdtReplicaManager.forceRefreshOpenDocumentReplicas(project, \"plugin-startup\")"
            )
        )
        assertFalse(source.contains("openFile("))
        assertFalse(source.contains("TmuxPaneFocusSync.install(project)"))
    }
}
