package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
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
            "JetBrains live-buffer reports should advertise operator-text and lazily receipt capabilities",
            source.contains("agent_doc_document_changed_digest_content_for_editor_v2") &&
                source.contains("operator_text_authority_v1") &&
                source.contains("lazily_transport_receipts_v1"),
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
    fun `open markdown buffers publish capability-bearing live-buffer reports before typing`() {
        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val lifecyclePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt"),
        ).first { Files.exists(it) }
        val tracker = Files.readString(trackerPath)
        val lifecycle = Files.readString(lifecyclePath)

        assertTrue(
            "project startup must seed already-open markdown buffers with a live-buffer authority report",
            lifecycle.contains("TypingTracker.reportOpenMarkdownDocuments(project)"),
        )
        assertTrue(
            "file-open events must seed newly opened markdown buffers with a live-buffer authority report",
            lifecycle.contains("override fun fileOpened(source: FileEditorManager, file: VirtualFile)") &&
                lifecycle.contains("TypingTracker.scheduleOpenDocumentReport(file)"),
        )
        assertTrue(
            "file-close events must clear this editor's live-buffer sidecar",
            lifecycle.contains("override fun fileClosed(source: FileEditorManager, file: VirtualFile)") &&
                lifecycle.contains("TypingTracker.clearOpenDocumentReport(file)"),
        )
        assertTrue(
            "open-document reporting should reuse the coalesced v2 full-content reporter",
            tracker.contains("fun reportOpenMarkdownDocuments(project: Project)") &&
                tracker.contains("FileEditorManager.getInstance(project).openFiles") &&
                tracker.contains("fun scheduleOpenDocumentReport(file: VirtualFile)") &&
                tracker.contains("scheduleFullContentReport(lib, file.path, document)") &&
                tracker.contains("agent_doc_document_changed_digest_content_for_editor_v2"),
        )
    }

    @Test
    fun `socket live-buffer publication is read-only and authority-bearing`() {
        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val watcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val tracker = Files.readString(trackerPath)
        val watcher = Files.readString(watcherPath)

        assertTrue(
            "socket IPC should expose a read-only live-buffer publication command",
            watcher.contains("\"publish_live_buffer\" -> {") &&
                watcher.contains("TypingTracker.publishLiveBufferNow(file)"),
        )

        val publishBody = tracker.substringAfter("fun publishLiveBufferNow")
            .substringBefore("private fun scheduleFullContentReport")
        assertTrue(
            "socket-triggered publication should resolve the live editor document and publish without queued-op side effects",
            publishBody.contains("LocalFileSystem.getInstance().findFileByPath(filePath)") &&
                publishBody.contains("return reportFullContentNow(") &&
                publishBody.contains("drainEditorOps = false") &&
                publishBody.contains("requireAuthority = true"),
        )

        val reporterBody = tracker.substringAfter("private fun reportFullContentNow")
            .substringBefore("private fun reportLiveBufferContentV1")
        assertTrue(
            "authority refresh must require the v2 capability-bearing ABI and keep legacy fallback only for non-authority reports",
            reporterBody.contains("agent_doc_document_changed_digest_content_for_editor_v2") &&
                reporterBody.contains("if (requireAuthority) false else") &&
                reporterBody.contains("if (drainEditorOps)"),
        )
    }

    @Test
    fun `coalesced editor op offsets use the per-op shadow not the final buffer`() {
        val reports = prepareEditorOpReports(
            finalText = "x",
            ops = listOf(
                PendingEditorOp(offset = 0, oldFragment = "", newFragment = "é", remoteCrdtApply = false),
                PendingEditorOp(offset = 1, oldFragment = "", newFragment = "x", remoteCrdtApply = false),
                PendingEditorOp(offset = 0, oldFragment = "é", newFragment = "", remoteCrdtApply = false),
            ),
        )

        assertEquals(
            listOf(
                PreparedEditorOp(opKind = "insert", byteOffset = 0L, insertText = "é", deleteBytes = 0L),
                PreparedEditorOp(opKind = "insert", byteOffset = 2L, insertText = "x", deleteBytes = 0L),
                PreparedEditorOp(opKind = "delete", byteOffset = 0L, insertText = null, deleteBytes = 2L),
            ),
            reports,
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
            "CRDT documentChanged should enqueue local CRDT forwarding onto the replica worker",
            listenerBody.contains("executor.execute"),
        )
        assertTrue(
            "CRDT documentChanged should defer full-buffer seeding to a background worker",
            listenerBody.contains("seedAndAttachFromDocument(filePath, event.document)"),
        )
        assertFalse(
            "CRDT documentChanged must not copy the full editor buffer on every keystroke",
            listenerBody.contains("event.document.text"),
        )
        assertFalse(
            "CRDT documentChanged must not compute code-point offsets on the UI thread for large shadows",
            listenerBody.contains("codePointOffset("),
        )
        assertFalse(
            "CRDT documentChanged must not apply shadow deltas on the UI thread",
            listenerBody.contains("applyEventToShadow("),
        )
        assertFalse(
            "CRDT documentChanged must not call the replica/socket forwarder on the UI thread",
            listenerBody.contains("forwardLocalDelta("),
        )
    }
}
