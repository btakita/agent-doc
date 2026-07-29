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
            .substringBefore("private fun recordPendingEditorOp")

        assertFalse(
            "the removed file-backed change marker must not be scheduled",
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
            "JetBrains current-document reports should advertise operator-text and lazily receipt capabilities",
            source.contains("agent_doc_lazily_current_observed_v1") &&
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
        val reportBody = source.substringAfter("private fun reportEditorOps")
        assertTrue(
            "one quiet typing burst should cross native persistence as one batch",
            reportBody.contains("agent_doc_record_editor_ops_json(filePath, baseHash, batch.toString())"),
        )
        assertEquals(
            "the batch reporter should resolve the merge base only once per burst",
            1,
            reportBody.substringBefore("\n}").split("agent_doc_document_base_hash").size - 1,
        )
        assertFalse(
            "the batch reporter must not reopen SQLite through one native call per editor op",
            reportBody.contains("agent_doc_record_editor_op("),
        )
    }

    @Test
    fun `open markdown buffers publish capability-bearing Lazily registrations before typing`() {
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
            "project startup must seed already-open markdown buffers with a Lazily registration",
            lifecycle.contains("TypingTracker.reportOpenMarkdownDocuments(project)"),
        )
        assertTrue(
            "file-open events must seed newly opened markdown buffers with a Lazily registration",
            lifecycle.contains("override fun fileOpened(source: FileEditorManager, file: VirtualFile)") &&
                lifecycle.contains("TypingTracker.scheduleOpenDocumentReport(file)"),
        )
        assertTrue(
            "file-close events must publish this editor's reliable-sync close",
            lifecycle.contains("override fun fileClosed(source: FileEditorManager, file: VirtualFile)") &&
                lifecycle.contains("TypingTracker.clearOpenDocumentReport(file)"),
        )
        val clearBody = tracker.substringAfter("fun clearOpenDocumentReport")
            .substringBefore("fun observeLazilyCurrentNow")
        assertTrue(
            "file-close cleanup must queue native release/close work off the file listener path",
            clearBody.contains("contentReportExecutor.execute"),
        )
        assertFalse(
            "file-close cleanup must not resolve the native library before queueing worker cleanup",
            clearBody.substringBefore("contentReportExecutor.execute").contains("AgentDocLib.get()"),
        )
        assertTrue(
            "file-close cleanup must publish the exact closing Lazily cut before closing authority",
            clearBody.contains("CrdtReplicaManager.publishClosingDocumentCut(filePath, closingDocument)") &&
                clearBody.indexOf("publishClosingDocumentCut") <
                clearBody.indexOf("agent_doc_document_closed_for_editor"),
        )
        assertFalse("file-close cleanup must not use plugin-owner compatibility", clearBody.contains("plugin_owner"))
        assertTrue(
            "a failed final publish must retain editor authority instead of emitting a lossy close",
            clearBody.contains("retaining editor authority instead of emitting a lossy close") &&
                clearBody.contains("return@execute"),
        )
        assertTrue(
            "open-document reporting should reuse the current coalesced full-content reporter",
            tracker.contains("fun reportOpenMarkdownDocuments(project: Project)") &&
                tracker.contains("FileEditorManager.getInstance(project).openFiles") &&
                tracker.contains("fun scheduleOpenDocumentReport(file: VirtualFile)") &&
                tracker.contains("scheduleFullContentReport(file.path, document)") &&
                tracker.contains("agent_doc_lazily_current_observed_v1"),
        )
    }

    @Test
    fun `socket current-document publication refreshes Lazily authority`() {
        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val watcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val nativePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/NativeLib.kt"),
        ).first { Files.exists(it) }
        val tracker = Files.readString(trackerPath)
        val watcher = Files.readString(watcherPath)
        val native = Files.readString(nativePath)

        assertTrue(
            "the shared socket intent should refresh the Lazily current document",
            watcher.contains("EditorIntent.ObserveLazilyCurrent.token -> {") &&
                watcher.contains("TypingTracker.observeLazilyCurrentNow(file)"),
        )
        val reloadIntentBody = watcher
            .substringAfter("EditorIntent.ReloadLibrary.token -> {")
            .substringBefore("EditorIntent.SaveDocument.token -> {")
        assertTrue(
            "JetBrains reload intents must enter the application-wide generation handoff",
            reloadIntentBody.contains("NativeReloadCoordinator.requestReload(libVersion)") &&
                !reloadIntentBody.contains("markRestartRequired"),
        )
        assertTrue(
            "the old generation must quiesce and drain calls before its JNA handle closes",
            native.contains("agent_doc_quiesce_for_reload") &&
                native.contains("stopAcceptingAndAwait") &&
                native.contains("executor.awaitTermination") &&
                native.contains("handler.nativeLibrary.close()") &&
                native.contains("nativeGenerationIsUnmapped"),
        )
        assertTrue(
            "a wedged native call must time out and poison its generation instead of blocking callers forever",
            native.contains("future.get(NATIVE_CALL_TIMEOUT_MS, TimeUnit.MILLISECONDS)") &&
                native.contains("catch (_: TimeoutException)") &&
                native.contains("poisonGeneration(this, reason)") &&
                native.contains("disabled the wedged generation to keep the IDE responsive"),
        )
        assertFalse("JNA's path-cached Native.load shortcut must not own generations", native.contains("Native.load("))
        assertTrue("mtime changes must schedule the same generation handoff", native.contains("requestReload(\"mtime\")"))
        assertTrue("reload must have no filesystem watcher", !watcher.contains("newWatchService()"))

        val publishBody = tracker.substringAfter("fun observeLazilyCurrentNow")
            .substringBefore("private fun scheduleFullContentReport")
        assertTrue(
            "socket-triggered publication should resolve the live editor document and publish without queued-op side effects",
            publishBody.contains("LocalFileSystem.getInstance().findFileByPath(filePath)") &&
                publishBody.contains("as? ApplicationEx") &&
                publishBody.contains("application.tryRunReadAction") &&
                !publishBody.contains(".runReadAction") &&
                publishBody.contains("return reportFullContentNow(") &&
                publishBody.contains("drainEditorOps = false") &&
                publishBody.contains("requireAuthority = true"),
        )

        val reporterBody = tracker.substringAfter("private fun reportFullContentNow")
            .substringBefore("private fun reportEditorOp")
        assertTrue(
            "authority refresh must require the current capability-bearing ABI without legacy fallback",
            reporterBody.contains("agent_doc_lazily_current_observed_v1") &&
                !reporterBody.contains("reportCompatibilityContent") &&
                reporterBody.contains("CrdtReplicaManager.ensureReplicaForOpenDocument") &&
                reporterBody.contains("await = true") &&
                reporterBody.contains("await = false") &&
                reporterBody.contains("forceRefresh = true") &&
                reporterBody.contains("if (!replicaRefreshAccepted) return false") &&
                reporterBody.contains("if (drainEditorOps)"),
        )
    }

    @Test
    fun `stale local baseline recovery is coalesced across a typing burst`() {
        val replicaPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val replica = Files.readString(replicaPath)
        val forwardBody = replica
            .substringAfter("private fun forwardLocalDeltaFromShadow")
            .substringBefore("fun requestRemoteDrain")
        val recoveryBody = replica
            .substringAfter("private fun scheduleStaleBaselineRecovery")
            .substringBefore("fun requestRemoteDrain")

        assertTrue(
            "typing while recovery is pending must replace the quiet-period task without another native baseline read",
            forwardBody.contains("staleBaselineRecoveryTasks.containsKey(filePath)") &&
                forwardBody.contains("scheduleStaleBaselineRecovery(filePath, document)") &&
                forwardBody.contains("recovery=coalesced_exact_editor_adopt_after_quiet"),
        )
        assertTrue(
            "the coalesced recovery must adopt one exact live editor cut and cancel its predecessor",
            recoveryBody.contains("tryReadDocumentText(document)") &&
                recoveryBody.contains("reason = \"coalesced-local-delta-baseline-diverged\"") &&
                recoveryBody.contains("staleBaselineRecoveryTasks.put(filePath, scheduled)?.cancel(false)"),
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
        val socketApplyBody = watcher.substringAfter("val stateGeneration = StateProjectionBridge.recordEditorPatchQueued")
            .substringBefore("if (applied || wasNoOp)")
        val refreshFunction = watcher.substringAfter("private fun refreshContentViaDocument")
            .substringBefore("private fun repositionBoundaryViaDocument")
        val repositionFunction = watcher.substringAfter("private fun repositionBoundaryViaDocument")
            .substringBefore("private fun scheduleRepositionRetry")
        val nativeSideEffectBody = watcher.substringAfter("private fun runNativeSideEffectOffEdt")
            .substringBefore("private fun appendOpsLog")
        val disposeBody = watcher.substringAfter("override fun dispose()")
            .substringBefore("companion object")

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
        assertTrue(
            "native op-capture fencing must precede every socket-driven EDT mutation",
            socketApplyBody.indexOf("prepareNonOperatorEditorMutationOnWorker(patch.file)") in
                0 until socketApplyBody.indexOf("invokeAndWait") &&
                refreshFunction.indexOf("prepareNonOperatorEditorMutationOnWorker(filePath)") in
                0 until refreshFunction.indexOf("invokeAndWait") &&
                repositionFunction.indexOf("prepareNonOperatorEditorMutationOnWorker(filePath)") in
                0 until repositionFunction.indexOf("invokeLater"),
        )
        assertTrue(
            "reposition native computation must leave the EDT before crossing FFI",
            repositionFunction.indexOf("executeOnPooledThread") in
                0 until repositionFunction.indexOf("NativePatching.repositionBoundaryToEnd"),
        )
        assertTrue(
            "editor telemetry and lifecycle native effects must dispatch off the EDT",
            nativeSideEffectBody.contains("SwingUtilities.isEventDispatchThread()") &&
                nativeSideEffectBody.contains("executeOnPooledThread(block)") &&
                disposeBody.contains("runNativeSideEffectOffEdt"),
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
        val prepareMutationBody = source
            .substringAfter("fun prepareNonOperatorEditorMutationOnWorker")
            .substringBefore("fun forceRefreshOpenDocumentReplica")
        val localMutationEpochBody = source
            .substringAfter("private fun advanceNonOperatorMutationEpoch")
            .substringBefore("\n        }")
        val remoteEditorApplyBody = source
            .substringAfter("private fun applyRemoteTextOnEdt")
            .substringBefore("private fun readRawDiskText")
        val postRegisterReplayBody = source
            .substringAfter("private fun replayDeferredWriteAfterRegistration")
            .substringBefore("private fun installListener")
        val replaceDeliveryBody = source
            .substringAfter("private fun applyReplaceDelivery")
            .substringBefore("private fun queueRemoteTextApply")
        val queueRemoteApplyBody = source
            .substringAfter("private fun queueRemoteTextApply")
            .substringBefore("private fun scheduleRemoteEditorApply")

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
                source.contains("replaceCached = forceRefresh") &&
                source.contains("expectedEditorTextAtSwap = if (forceRefresh) registrationText else null") &&
                source.contains("executor.submit<Boolean> { attach() }") &&
                source.contains(".get(CRDT_AWAIT_ATTACH_TIMEOUT_MS, TimeUnit.MILLISECONDS)") &&
                source.contains("private const val CRDT_AWAIT_ATTACH_TIMEOUT_MS = 750L") &&
                source.contains("executor.execute { attach() }") &&
                source.contains("forwarder.ensureEditorText(initialEditorText)"),
        )
        val forceRefreshAttachBody = source.substringAfter("fun ensureOpenDocumentReplica(")
            .substringBefore("private fun publishClosingDocumentCut(")
        assertFalse(
            "forced refresh must not replace the visible editor from a retained whole-document target",
            forceRefreshAttachBody.contains("deferredWriteReconnectContent(") ||
                forceRefreshAttachBody.contains("applyMinimalDocumentEditUtil(") ||
                forceRefreshAttachBody.contains("persistRemoteCrdtTextIfSafe("),
        )
        assertTrue(
            "forced refresh must register from the exact editor cut and fence a raced swap",
            forceRefreshAttachBody.contains("val registrationText = text") &&
                forceRefreshAttachBody.contains("deferredWriteReconnectPropagated(filePath, registrationText)") &&
                forceRefreshAttachBody.contains("replaceCached = forceRefresh") &&
                source.contains("editorBufferText(filePath) != expectedEditorTextAtSwap") &&
                source.contains("forwarder.deregister()") &&
                source.contains("return null"),
        )
        assertTrue(
            "every non-operator editor projection must close the prior native op-capture epoch on a worker before EDT dispatch",
            prepareMutationBody.contains("check(!javax.swing.SwingUtilities.isEventDispatchThread())") &&
                prepareMutationBody.contains("lib.agent_doc_clear_editor_op_epoch(filePath) == 1") &&
                source.contains("prepareNonOperatorEditorMutationOnWorker(filePath)") &&
                source.contains("advanceNonOperatorMutationEpoch(filePath)"),
        )
        assertFalse(
            "the EDT-local mutation epoch must not cross the native ABI",
            localMutationEpochBody.contains("AgentDocLib") ||
                localMutationEpochBody.contains("agent_doc_clear_editor_op_epoch"),
        )
        assertFalse(
            "the queued remote editor apply must not cross the native ABI from the EDT",
            remoteEditorApplyBody.contains("AgentDocLib") ||
                remoteEditorApplyBody.contains("agent_doc_clear_editor_op_epoch"),
        )
        assertTrue(
            "native op-capture fencing must precede every CRDT-driven EDT mutation",
            postRegisterReplayBody.indexOf("prepareNonOperatorEditorMutationOnWorker(filePath)") in
                0 until postRegisterReplayBody.indexOf("invokeAndWait") &&
                replaceDeliveryBody.indexOf("prepareNonOperatorEditorMutationOnWorker(filePath)") in
                0 until replaceDeliveryBody.indexOf("invokeAndWait") &&
                queueRemoteApplyBody.indexOf("prepareNonOperatorEditorMutationOnWorker(filePath)") in
                0 until queueRemoteApplyBody.indexOf("remoteEditorApplies.ingress"),
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
        val localDeltaBody = source.substringAfter("private fun forwardLocalDeltaFromShadow(")
            .substringBefore("private fun requestRemoteDrain(")
        assertTrue(
            "a local editor delta must verify the native shadow frontier or coalesce exact-editor adoption",
            localDeltaBody.contains("shouldForwardLocalDeltaUtil(replicaText, beforeText)") &&
                localDeltaBody.contains("staleBaselineRecoveryTasks.containsKey(filePath)") &&
                localDeltaBody.contains("scheduleStaleBaselineRecovery(filePath, document)") &&
                source.contains("reason = \"coalesced-local-delta-baseline-diverged\""),
        )
        assertTrue(
            "forced refresh must register and atomically swap before retiring the cached client",
            source.contains("if (bypassRegisterBackoff)") &&
                source.contains("forwarders.replace(filePath, cached, forwarder)") &&
                source.contains("cached.deregister()") &&
                source.contains("replacement register failed") &&
                source.contains("retained cached forwarder"),
        )
        val forwarderForBody = source.substringAfter("private fun forwarderFor(")
            .substringBefore("private fun refreshReplicaAfterTransportLoss(")
        assertTrue(
            "replica replacement must fence the exact editor before publication and revalidate it at the swap boundary",
            forwarderForBody.contains("expectedEditorTextAtSwap") &&
                forwarderForBody.contains("editorBufferText(filePath) != expectedEditorTextAtSwap") &&
                forwarderForBody.indexOf("editorBufferText(filePath) != expectedEditorTextAtSwap") <
                forwarderForBody.indexOf("forwarder.ensureEditorText(initialEditorText)") &&
                forwarderForBody.lastIndexOf("editorBufferText(filePath) != expectedEditorTextAtSwap") >
                forwarderForBody.indexOf("forwarder.ensureEditorText(initialEditorText)") &&
                forwarderForBody.indexOf("editorBufferText(filePath) != expectedEditorTextAtSwap") <
                forwarderForBody.indexOf("forwarders.replace(filePath, cached, forwarder)"),
        )
        assertTrue(
            "visible editor applies must retain failed delivery ACKs and replay them from the current buffer",
            source.contains("pendingRemoteAckReplays") &&
                source.contains("rememberPendingRemoteAcks(pending.filePath, pending.acknowledgements)") &&
                source.contains("replayPendingRemoteAcks(filePath, forwarder)") &&
                source.contains("shouldPullRemoteDeliveryAfterAckReplayUtil(pendingAckCount)") &&
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
            .substringBefore("private fun recoverRejectedRemoteCanonical")
        assertFalse(
            "remote CRDT editor apply must not call native template normalization inside invokeAndWait",
            remoteApplyBody.contains("NativePatching.normalizeTemplateStructure"),
        )
        assertTrue(
            "remote CRDT template normalization should run on the replica worker before the EDT apply",
            source.contains("private fun templateStructureState(") &&
                source.substringAfter("private fun templateStructureState(").contains("NativePatching.normalizeTemplateStructure(text)"),
        )
        val guardRecoveryBody = source.substringAfter("private fun recoverRejectedRemoteCanonical")
            .substringBefore("private fun scheduleTemplateGuardRecoveryRetry")
        assertTrue(
            "template-guard recovery must fence exact editor authority before bounded adopt and atomic replacement",
            guardRecoveryBody.contains("editorText == expectedText") &&
                guardRecoveryBody.contains("editorBufferText(filePath) != editorText") &&
                guardRecoveryBody.contains("staleForwarder.pushTextAdopt(editorText)") &&
                guardRecoveryBody.contains("replaceCached = true"),
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
        val backoffBody = source.substringAfter("private fun scheduleRemoteDrainAfterBackoff(")
            .substringBefore("private fun drainRemoteUpdates(")
        assertTrue(
            "a backoff resume must drain retained flags without manufacturing a workspace-wide request",
            backoffBody.contains("queueRemoteDrain(CRDT_DRAIN_BACKOFF_REASON)") &&
                !backoffBody.contains("requestRemoteDrain("),
        )
        assertTrue(
            "drain-all must consume already-covered per-file requests instead of rearming forever",
            source.substringAfter("val paths = if (drainAll)")
                .substringBefore("if (paths.isEmpty())")
                .contains("drainRequestedPaths.clear()"),
        )
        assertTrue(
            "backoff diagnostics must use a bounded reason token",
            source.contains("CRDT_DRAIN_BACKOFF_REASON = \"backoff-resume\"") &&
                !backoffBody.contains("reason-backoff"),
        )
        assertTrue(
            "CRDT timing logs should identify slow native/socket/EDT operations",
            source.contains("[crdt-perf]") &&
                source.contains("remote-apply-edt") &&
                source.contains("template-normalize-worker"),
        )
    }
}
