package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TypingTrackerEdtBudgetTest {
    @Test
    fun `document listener records cheap change event and defers full content reporting`() {
        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(trackerPath)
        val listenerBody = source.substringAfter("override fun documentChanged")
            .substringBefore("private fun scheduleFullContentReport")

        assertTrue(
            "documentChanged should record only the cheap native change marker on the listener path",
            listenerBody.contains("agent_doc_document_changed(filePath)"),
        )
        assertTrue(
            "documentChanged should enqueue the full editor buffer report for a coalesced worker",
            listenerBody.contains("scheduleFullContentReport(lib, filePath, event.document)"),
        )
        assertTrue(
            "documentChanged should capture the small editor op payload for async native reporting",
            listenerBody.contains("val op = PendingEditorOp("),
        )
        assertTrue(
            "documentChanged should append the small editor op payload without replacing earlier burst ops",
            listenerBody.contains("recordPendingEditorOp(filePath, op)"),
        )
        assertFalse(
            "documentChanged must not copy the full editor buffer on every keystroke",
            listenerBody.contains("event.document.text"),
        )
        assertFalse(
            "documentChanged must not synchronously write full buffer content through JNA",
            listenerBody.contains("agent_doc_document_changed_digest_content"),
        )
        assertFalse(
            "documentChanged must not synchronously record editor ops through JNA",
            listenerBody.contains("reportEditorOp("),
        )
    }

    @Test
    fun `coalesced full content reporting preserves every editor op in a typing burst`() {
        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(trackerPath)

        assertTrue(
            "typing bursts should accumulate editor ops instead of replacing the previous op",
            source.contains("pendingEditorOps"),
        )
        assertTrue(
            "full-buffer report should drain the accumulated op burst",
            source.contains("drainPendingEditorOps(filePath)"),
        )
        assertFalse(
            "coalescing must not overwrite the previous editor op with only the newest event",
            source.contains("scheduleFullContentReport(lib, filePath, event.document, op)"),
        )
    }

    @Test
    fun `crdt document listener uses shadows instead of copying full editor text`() {
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(managerPath)
        val listenerBody = source.substringAfter("override fun documentChanged")
            .substringBefore("private fun seedAndAttachFromDocument")

        assertTrue(
            "CRDT documentChanged should update an in-memory shadow from the DocumentEvent delta",
            listenerBody.contains("applyEventToShadow(beforeText, event.offset, oldFragment, newFragment)"),
        )
        assertTrue(
            "CRDT documentChanged should defer full-buffer seeding to a background worker",
            listenerBody.contains("seedAndAttachFromDocument(filePath, event.document)"),
        )
        assertFalse(
            "CRDT documentChanged must not copy the full editor buffer on every keystroke",
            listenerBody.contains("event.document.text"),
        )
    }
}
