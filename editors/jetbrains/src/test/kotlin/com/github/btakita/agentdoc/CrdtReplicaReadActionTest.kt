package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

class CrdtReplicaReadActionTest {
    @Test
    fun `forced replica refresh captures IntelliJ model state on EDT without blocking workers`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)
        val refreshAll = source.substringAfter("fun forceRefreshOpenDocumentReplicas")
            .substringBefore("fun <T> withAgentAppliedEditorMutation")
        val refreshOne = source.substringAfter("fun forceRefreshOpenDocumentReplica(")
            .substringBefore("fun ensureReplicaForOpenDocument")
        val recoveryRefresh = source.substringAfter("fun refreshOpenDocumentReplicaForRecoveryAndWait(")
            .substringBefore("private fun runOnEdtNonBlocking")

        assertEdtCapturePrecedesDocumentLookup(refreshAll, "all-open-document refresh")
        assertEdtCapturePrecedesDocumentLookup(refreshOne, "single-document refresh")
        assertTrue(
            "all-open-document refresh should leave CRDT work on a pooled thread",
            refreshAll.indexOf("openDocuments.forEach") >
                refreshAll.indexOf("executeOnPooledThread"),
        )
        assertTrue(
            "single-document refresh should leave CRDT work on a pooled thread",
            refreshOne.indexOf("manager.ensureOpenDocumentReplica") >
                refreshOne.indexOf("executeOnPooledThread"),
        )
        assertTrue(
            "typed recovery must capture IntelliJ model state on the EDT before acknowledging attach",
            recoveryRefresh.contains("invokeAndWait") &&
                recoveryRefresh.contains("await = true") &&
                recoveryRefresh.contains("forceRefresh = true"),
        )
        assertTrue(
            "typed recovery must return the completed attach result",
            recoveryRefresh.contains("return attached"),
        )
        assertTrue(
            "typed recovery must not wait on an IDEA read permit",
            !recoveryRefresh.contains("runReadAction"),
        )

        val forwarderSwap = source.substringAfter("if (forwarders.replace(filePath, cached, forwarder))")
            .substringBefore("return forwarder")
        assertTrue(
            "a successful replica swap must deregister the old member without an ACK sidecar",
            forwarderSwap.contains("cached.deregister()") &&
                !forwarderSwap.contains("clearPendingRemoteAcks"),
        )
    }

    private fun assertEdtCapturePrecedesDocumentLookup(body: String, label: String) {
        val edtCaptureIndex = body.indexOf("runOnEdtNonBlocking")
        val documentLookupIndex = body.indexOf("getDocument(file)")
        assertTrue("$label must enter a nonblocking EDT capture", edtCaptureIndex >= 0)
        assertTrue(
            "$label must perform FileDocumentManager.getDocument inside the EDT capture",
            documentLookupIndex > edtCaptureIndex,
        )
        assertTrue("$label must not block a callback thread on a read action", !body.contains("runReadAction"))
    }
}
