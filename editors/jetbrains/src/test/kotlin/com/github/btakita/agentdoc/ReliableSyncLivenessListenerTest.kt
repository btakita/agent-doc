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

    @Test
    fun `native reload republishes liveness before replica registration`() {
        val root = Paths.get("src/main/kotlin/com/github/btakita/agentdoc")
        val listener = root.resolve("ReliableSyncLivenessListener.kt").toFile().readText()
        val coordinator = root.resolve("NativeReloadCoordinator.kt").toFile().readText()

        assertTrue(listener.contains("fun republishOpenDocumentsAfterNativeReload("))
        val liveness = coordinator.indexOf("republishOpenDocumentsAfterNativeReload(")
        val replicas = coordinator.indexOf("CrdtReplicaManager.restartAfterNativeReload(")
        assertTrue("liveness endpoint authority must advance before replica admission", liveness >= 0)
        assertTrue("replica restart must follow liveness republish", replicas > liveness)
    }
}
