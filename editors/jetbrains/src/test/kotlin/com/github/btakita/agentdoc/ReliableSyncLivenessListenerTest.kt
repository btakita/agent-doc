package com.github.btakita.agentdoc

import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

class ReliableSyncLivenessListenerTest {
    @Test
    fun `liveness uses and retains the owning nested project root`() {
        val source =
            Paths.get(
                "src/main/kotlin/com/github/btakita/agentdoc/ReliableSyncLivenessListener.kt",
            ).toFile().readText()

        assertTrue(
            "open must resolve the nearest agent-doc root instead of assuming the IDE base path",
            source.contains("NativePatching.resolveProjectPath(filePath)?.first ?: fallbackRoot"),
        )
        assertTrue(
            "close must reuse the exact root selected at open even if the file is no longer readable",
            source.contains("projectRoots.remove(documentHash)"),
        )
        assertTrue(
            "the resolved root must own the liveness outbox and controller flush",
            source.contains("push(lib, root, documentHash, opsJson)"),
        )
    }
}
