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
            .substringBefore("private fun requestNativeDocumentChanged")

        assertTrue(
            "documentChanged should enqueue the lightweight native change marker off the listener path",
            listenerBody.contains("requestNativeDocumentChanged(filePath)"),
        )
        assertTrue(
            "agent-applied editor patches must share the non-operator provenance path with remote CRDT applies",
            listenerBody.contains("CrdtReplicaManager.isOperatorDocumentEvent(filePath, event)"),
        )
        assertTrue(
            "documentChanged should enqueue the full editor buffer report for a coalesced worker",
            listenerBody.contains("scheduleFullContentReport(filePath, event.document)"),
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
            "documentChanged must not resolve the native library on the UI thread",
            listenerBody.contains("AgentDocLib.get()"),
        )
        assertFalse(
            "documentChanged must not call the native change marker on the UI thread",
            listenerBody.contains("agent_doc_document_changed(filePath)"),
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
        assertTrue(
            "full-buffer reports should use lazily KeepLatest debounce state",
            source.contains("DebounceCore<Document>") &&
                source.contains("state.debounce.input") &&
                source.contains("state.debounce.tick"),
        )
        assertTrue(
            "debounce generation removal must be atomic with new input",
            source.contains("pendingContentReports.compute(filePath)") &&
                source.contains("if (current !== state) return@compute current"),
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
        val clearBody = tracker.substringAfter("fun clearOpenDocumentReport")
            .substringBefore("fun publishLiveBufferNow")
        assertTrue(
            "file-close cleanup must queue native release/close work off the file listener path",
            clearBody.contains("contentReportExecutor.execute"),
        )
        assertFalse(
            "file-close cleanup must not resolve the native library before queueing worker cleanup",
            clearBody.substringBefore("contentReportExecutor.execute").contains("AgentDocLib.get()"),
        )
        assertTrue(
            "open-document reporting should reuse the coalesced v2 full-content reporter",
            tracker.contains("fun reportOpenMarkdownDocuments(project: Project)") &&
                tracker.contains("FileEditorManager.getInstance(project).openFiles") &&
                tracker.contains("fun scheduleOpenDocumentReport(file: VirtualFile)") &&
                tracker.contains("scheduleFullContentReport(file.path, document)") &&
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
        assertTrue(
            "a native reload broadcast must re-register every open markdown replica",
            watcher.contains("\"reload_lib\" -> {") &&
                watcher.contains("AgentDocLib.forceReload()") &&
                watcher.contains("CrdtReplicaManager.forceRefreshOpenDocumentReplicas(project"),
        )

        val publishBody = tracker.substringAfter("fun publishLiveBufferNow")
            .substringBefore("private fun scheduleFullContentReport")
        assertTrue(
            "socket-triggered publication should resolve the live editor document and publish without queued-op side effects",
            publishBody.contains("LocalFileSystem.getInstance().findFileByPath(filePath)") &&
                publishBody.contains("runReadAction<com.intellij.openapi.editor.Document?>") &&
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
                reporterBody.contains("CrdtReplicaManager.ensureReplicaForOpenDocument") &&
                reporterBody.contains("await = true") &&
                reporterBody.contains("await = false") &&
                reporterBody.contains("forceRefresh = true") &&
                reporterBody.contains("if (!replicaRefreshAccepted) return false") &&
                reporterBody.contains("if (drainEditorOps)"),
        )
    }

    @Test
    fun `patch watcher wraps editor document mutations as non-operator changes`() {
        val watcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val watcher = Files.readString(watcherPath)
        val componentApplyBody = watcher.substringAfter("\"applyPatch.component\"")
            .substringBefore("wrote = true")
        val refreshBody = watcher.substringAfter("\"refreshContent.postcommit\"")
            .substringBefore("applied = true")
        val repositionBody = watcher.substringAfter("\"repositionBoundary\"")
            .substringBefore("} else if (result != content)")

        assertTrue(
            "agent-doc IPC patch writes should not set the unsynced-local-operator flag",
            componentApplyBody.contains("CrdtReplicaManager.withAgentAppliedEditorMutation(patch.file)") &&
                componentApplyBody.contains("applyMinimalDocumentEditUtil(document, content, result)"),
        )
        assertTrue(
            "socket refresh_content writes should not look like operator typing",
            refreshBody.contains("CrdtReplicaManager.withAgentAppliedEditorMutation(filePath)") &&
                refreshBody.contains("applyMinimalDocumentEditUtil(document, proof.content, content)"),
        )
        assertTrue(
            "socket boundary reposition writes should not look like operator typing",
            repositionBody.contains("CrdtReplicaManager.withAgentAppliedEditorMutation(filePath)") &&
                repositionBody.contains("applyMinimalDocumentEditUtil(document, content, result)"),
        )
    }

    @Test
    fun `coalesced editor op offsets use the per-op shadow not the final buffer`() {
        val reports = prepareEditorOpReports(
            finalText = "x",
            ops = listOf(
                PendingEditorOp(offset = 0, oldFragment = "", newFragment = "é", nonOperatorMutation = false),
                PendingEditorOp(offset = 1, oldFragment = "", newFragment = "x", nonOperatorMutation = false),
                PendingEditorOp(offset = 0, oldFragment = "é", newFragment = "", nonOperatorMutation = false),
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
        assertTrue(
            "authority-bearing publish/open document repair must wait for the CRDT replica while ordinary open reports stay asynchronous",
            source.contains("fun ensureOpenDocumentReplica(") &&
                source.contains("forceRefresh: Boolean = false") &&
                source.contains("bypassRegisterBackoff = forceRefresh") &&
                source.contains("NativePatching.deferredWriteReconnectContent(filePath, text)") &&
                source.contains("executor.submit<Boolean> { attach() }") &&
                source.contains(".get(CRDT_AWAIT_ATTACH_TIMEOUT_MS, TimeUnit.MILLISECONDS)") &&
                source.contains("private const val CRDT_AWAIT_ATTACH_TIMEOUT_MS = 750L") &&
            source.contains("executor.execute { attach() }") &&
                source.contains("forwarder.ensureEditorText(initialEditorText)"),
        )
        assertFalse(
            "an existing CRDT replica must never be overwritten from an unproven full editor snapshot",
            source.contains("it.ensureEditorText(initialEditorText)"),
        )
        assertTrue(
            "only user-attributable incremental editor events may originate CRDT deltas",
            listenerBody.contains("isOperatorDocumentEvent(filePath, event)") &&
                listenerBody.contains("non-operator-editor-event") &&
                source.contains("wholeTextReplaced = event.isWholeTextReplaced") &&
                source.contains("stale-operator-event-fenced"),
        )
        assertTrue(
            "forced refresh must retire a cached client and issue REGISTER again after a controller recycle",
            source.contains("if (bypassRegisterBackoff)") &&
                source.contains("forwarders.remove(filePath)?.let { stale ->") &&
                source.contains("stale.deregister()") &&
                source.contains("retired cached forwarder before forced re-register"),
        )
        assertTrue(
            "visible editor applies must retain failed delivery ACKs and replay them from the current buffer",
            source.contains("pendingRemoteAckReplays") &&
                source.contains("rememberPendingRemoteAcks(pending.filePath, pending.acknowledgements)") &&
                source.contains("replayPendingRemoteAcks(filePath, forwarder)") &&
                source.contains("val visibleText = editorBufferText(filePath)"),
        )
        val remoteDrainBody = source.substringAfter("private fun drainRemoteUpdatesFor")
            .substringBefore("/**\n     * D2")
        assertTrue(
            "remote CRDT update bursts should be merged before one editor apply instead of one invokeAndWait per update",
            remoteDrainBody.contains("appliedRemoteUpdates") &&
                remoteDrainBody.contains("queueRemoteTextApply(filePath, expectedText, targetText"),
        )
        val remoteApplyBody = source.substringAfter("private fun queueRemoteTextApply")
            .substringBefore("private fun normalizeRemoteText")
        assertFalse(
            "remote CRDT editor apply must not call native template normalization inside invokeAndWait",
            remoteApplyBody.contains("NativePatching.normalizeTemplateStructure"),
        )
        assertTrue(
            "remote CRDT template normalization should run on the replica worker before the EDT apply",
            source.contains("private fun normalizeRemoteText(") &&
                source.substringAfter("private fun normalizeRemoteText(").contains("NativePatching.normalizeTemplateStructure(converged)"),
        )
        assertTrue(
            "remote CRDT editor apply should use RelayCell backpressure and schedule bounded EDT work",
            source.contains("KeyedCoalescingRelay<String, PendingRemoteEditorApply>(REMOTE_EDITOR_APPLY_MERGE)") &&
                remoteApplyBody.contains("scheduleRemoteEditorApply()") &&
                source.contains("ApplicationManager.getApplication().invokeLater"),
        )
        assertFalse(
            "ordinary remote CRDT editor apply must never synchronously wait on the EDT",
            remoteApplyBody.contains("invokeAndWait"),
        )
        assertFalse(
            "remote-drain backoff must schedule a later drain instead of parking the single replica worker",
            source.contains("Thread.sleep(delayMs)"),
        )
        assertTrue(
            "remote-drain backoff should coalesce one scheduled retry while local deltas remain runnable",
            source.contains("remoteDrainBackoffScheduled.compareAndSet(false, true)") &&
                source.contains("executor.schedule("),
        )
        assertTrue(
            "CRDT timing logs should identify slow native/socket/EDT operations",
            source.contains("[crdt-perf]") &&
                source.contains("remote-apply-edt") &&
                source.contains("remote-normalize-worker"),
        )
    }
}
