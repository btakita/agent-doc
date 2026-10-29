package com.github.btakita.agentdoc

import com.intellij.openapi.vfs.VirtualFile
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Paths

class FileCacheConflictQueuedLogTest {
    @Test
    fun `queued conflict log names File Cache Conflict and the resolver stamps`() {
        val line = fileCacheConflictQueuedIdeaLogLine(
            filePath = "/sessions/doc-a.md",
            probe = "isInConflictQueue",
            documentStamp = 21355,
            oldFileStamp = 19218,
        )

        assertTrue(line.contains("File Cache Conflict"))
        assertTrue(line.contains("queued for /sessions/doc-a.md"))
        assertTrue(line.contains("resolver=isInConflictQueue"))
        assertTrue(line.contains("documentStamp=21355"))
        assertTrue(line.contains("oldFileStamp=19218"))
        assertFalse(line.contains("Load FS"))
        assertFalse(line.contains("Keep Memory"))
        assertFalse(line.contains("Accept"))
        assertFalse(line.contains("Cancel"))
    }

    @Test
    fun `queued conflict log omits stamps the resolver did not expose`() {
        val line = fileCacheConflictQueuedIdeaLogLine(
            filePath = "/sessions/doc-a.md",
            probe = "hasConflict",
            documentStamp = null,
            oldFileStamp = null,
        )

        assertTrue(line.contains("File Cache Conflict"))
        assertTrue(line.contains("resolver=hasConflict"))
        assertFalse(line.contains("documentStamp"))
        assertFalse(line.contains("oldFileStamp"))
    }

    @Test
    fun `conflict probe prefers the queued resolver method over the legacy name`() {
        assertEquals(
            listOf("isInConflictQueue", "hasConflict"),
            IntelliJFileCacheConflictGuard.conflictProbeMethodNames,
        )
        assertEquals(
            "isInConflictQueue",
            IntelliJFileCacheConflictGuard.findConflictProbe(CurrentResolver::class.java)?.name,
        )
        assertEquals(
            "hasConflict",
            IntelliJFileCacheConflictGuard.findConflictProbe(LegacyResolver::class.java)?.name,
        )
        assertEquals(
            "hasConflict",
            IntelliJFileCacheConflictGuard.findConflictProbe(InheritedLegacyResolver::class.java)?.name,
        )
        assertNull(IntelliJFileCacheConflictGuard.findConflictProbe(UnrelatedResolver::class.java))
    }

    @Test
    fun `vfs content change logs the phrase when the resolver queues a conflict`() {
        val guard = readPluginSource("FileCacheConflictGuard.kt")
        val manager = readPluginSource("CrdtReplicaManager.kt")
        val queue = guard.indexOf("\"isInConflictQueue\"")
        val legacy = guard.indexOf("\"hasConflict\"")
        val note = manager
            .substringAfter("private fun noteQueuedFileCacheConflict(")
            .substringBefore("private fun reconcileRemotePersistence(")

        assertTrue(queue >= 0 && legacy > queue)
        assertTrue(guard.contains("findConflictProbe"))
        assertTrue(manager.contains(".forEach(::noteQueuedFileCacheConflict)"))
        assertTrue(manager.contains(".forEach(::projectNativeSaveReceipt)"))
        assertTrue(note.contains("IntelliJFileCacheConflictGuard.probe("))
        assertTrue(note.contains("fileCacheConflictQueuedIdeaLogLine("))
        assertTrue(note.contains("newlyPendingEdge"))
        assertTrue(note.contains("event.oldModificationStamp"))
        assertTrue(note.contains("Throwable(\"File Cache Conflict queued\")"))
        assertTrue(
            note.indexOf("if (!decision.newlyPendingEdge) return") <
                note.indexOf("fileCacheConflictQueuedIdeaLogLine("),
        )
    }

    private fun readPluginSource(name: String): String {
        val path = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
        ).first { Files.exists(it) }
        return Files.readString(path)
    }
}

open class LegacyResolver {
    @Suppress("unused")
    fun hasConflict(@Suppress("UNUSED_PARAMETER") file: VirtualFile): Boolean = true
}

class InheritedLegacyResolver : LegacyResolver()

class CurrentResolver {
    @Suppress("unused")
    fun isInConflictQueue(@Suppress("UNUSED_PARAMETER") file: VirtualFile): Boolean = true

    @Suppress("unused")
    fun hasConflict(@Suppress("UNUSED_PARAMETER") file: VirtualFile): Boolean = false
}

class UnrelatedResolver
