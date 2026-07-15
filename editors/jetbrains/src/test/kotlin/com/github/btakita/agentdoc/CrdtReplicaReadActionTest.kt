package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertTrue
import org.junit.Test

class CrdtReplicaReadActionTest {
    @Test
    fun `forced replica refresh captures IntelliJ model state under read action`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)
        val refreshAll = source.substringAfter("fun forceRefreshOpenDocumentReplicas")
            .substringBefore("fun <T> withAgentAppliedEditorMutation")
        val refreshOne = source.substringAfter("fun forceRefreshOpenDocumentReplica(")
            .substringBefore("fun ensureReplicaForOpenDocument")

        assertReadActionPrecedesDocumentLookup(refreshAll, "all-open-document refresh")
        assertReadActionPrecedesDocumentLookup(refreshOne, "single-document refresh")
        assertTrue(
            "all-open-document refresh should leave CRDT work outside the read action",
            refreshAll.indexOf("openDocuments.forEach") > refreshAll.indexOf("getDocument(file)"),
        )
        assertTrue(
            "single-document refresh should leave CRDT work outside the read action",
            refreshOne.indexOf("manager.ensureOpenDocumentReplica") > refreshOne.indexOf("val (resolvedFilePath"),
        )

        val forwarderSwap = source.substringAfter("if (forwarders.replace(filePath, cached, forwarder))")
            .substringBefore("return forwarder")
        val retireIndex = forwarderSwap.indexOf("clearPendingRemoteAcks(filePath)")
        assertTrue("a successful replica swap must retire the old ACK frontier", retireIndex >= 0)
        assertTrue(
            "ACK retirement must happen before the old member is deregistered",
            forwarderSwap.indexOf("cached.deregister()") > retireIndex,
        )
    }

    private fun assertReadActionPrecedesDocumentLookup(body: String, label: String) {
        val readActionIndex = body.indexOf("runReadAction")
        val documentLookupIndex = body.indexOf("getDocument(file)")
        assertTrue("$label must enter a read action", readActionIndex >= 0)
        assertTrue(
            "$label must perform FileDocumentManager.getDocument inside the read-action capture",
            documentLookupIndex > readActionIndex,
        )
    }
}
