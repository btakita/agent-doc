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
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/AgentDocProjectLifecycle.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/AgentDocProjectLifecycle.kt"),
        ).first { Files.exists(it) }
        val replicaPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val tracker = Files.readString(trackerPath)
        val lifecycle = Files.readString(lifecyclePath)
        val replica = Files.readString(replicaPath)

        assertTrue(
            "project startup must seed already-open markdown buffers with a Lazily registration",
            lifecycle.contains("TypingTracker.reportOpenMarkdownDocuments(project)") &&
                lifecycle.contains("CrdtReplicaManager.ensureOpenDocumentReplicas(project, \"plugin-startup\")"),
        )
        assertTrue(
            "file-open events must seed newly opened markdown buffers with a Lazily registration",
            lifecycle.contains("override fun fileOpened(source: FileEditorManager, file: VirtualFile)") &&
                lifecycle.contains("TypingTracker.scheduleOpenDocumentReport(file)") &&
                lifecycle.contains("CrdtReplicaManager.ensureOpenDocumentReplica("),
        )
        assertTrue(
            "file-close events must publish this editor's reliable-sync close once no client has the file open",
            lifecycle.contains("override fun fileClosed(source: FileEditorManager, file: VirtualFile)") &&
                lifecycle.contains("TypingTracker.onEditorClosed(project, source, file)") &&
                tracker.contains("onRelease = { clearOpenDocumentReport(file) }"),
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
        val closePublishBody = replica.substringAfter("private fun publishClosingDocumentCut")
            .substringBefore("private fun forwardLocalEditsFromShadow")
        assertTrue(
            "closing the last replica must wait for its serialized cut to finish",
            closePublishBody.contains("}.get()"),
        )
        assertFalse(
            "closing-cut resolution must not abandon a large publish on the ordinary two-second budget",
            closePublishBody.contains("CRDT_AWAIT_CLOSE_PUBLISH_TIMEOUT_MS") ||
                closePublishBody.contains("TimeoutException"),
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
            .substringBefore("else -> {")
        assertTrue(
            "JetBrains reload intents must enter the application-wide generation handoff",
            reloadIntentBody.contains("NativeReloadCoordinator.requestReload(") &&
                reloadIntentBody.contains("trigger = NATIVE_RELOAD_TRIGGER_IPC") &&
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
            "only a running wedged native call may poison its generation; queued work retains it",
            native.contains("future.get(timeoutMs, TimeUnit.MILLISECONDS)") &&
                native.contains("catch (_: TimeoutException)") &&
                native.contains("NativeCallQueueTimeoutException") &&
                native.contains("generation retained") &&
                native.contains("poisonGeneration(this, reason)") &&
                native.contains("disabled the wedged generation to keep the IDE responsive"),
        )
        assertTrue(
            "ordinary native calls must stay on an isolated caller lane so they cannot disable reactive ingress",
            native.contains("NativeCallLane.IsolatedCaller") &&
                native.contains("else -> NativeCallLane.IsolatedCaller") &&
                native.contains("native generation retained and unrelated reactive ingress remains available"),
        )
        assertFalse("JNA's path-cached Native.load shortcut must not own generations", native.contains("Native.load("))
        assertTrue(
            "mtime changes must schedule the same generation handoff",
            native.contains("requestReload(trigger = NATIVE_RELOAD_TRIGGER_MTIME)"),
        )
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
                publishBody.contains("requireReplica = true"),
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
                reporterBody.contains("forceRefresh = false") &&
                !reporterBody.contains("forceRefresh = true") &&
                reporterBody.contains("if (!replicaAvailable) return false") &&
                reporterBody.contains("if (drainEditorOps)"),
        )
    }

    @Test
    fun `stale local baseline projects controller canonical without editor adoption`() {
        val replicaPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val replica = Files.readString(replicaPath)
        val forwardBody = replica
            .substringAfter("private fun forwardLocalEditsFromShadow")
            .substringBefore("fun requestRemoteDrain")

        assertTrue(
            "a stale native baseline must retain the captured delta and rebootstrap its exact controller base",
            forwardBody.contains("retainedCanonicalProjectionPaths.add(filePath)") &&
                forwardBody.contains("expectedCanonicalTextAtSwap = canonical") &&
                forwardBody.contains("requestRemoteDrain(filePath, \"captured-local-delta-retry\")"),
        )
        assertFalse(
            "stale-baseline recovery must never adopt a whole editor buffer into canonical authority",
            replica.contains("scheduleStaleBaselineRecovery") ||
                forwardBody.contains("adoptExactEditorBaseline(") ||
                forwardBody.contains("coalesced_exact_editor_adopt_after_quiet"),
        )
    }

    @Test
    fun `patch watcher wraps editor document mutations as non-operator changes`() {
        val watcherPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"),
        ).first { Files.exists(it) }
        val watcher = Files.readString(watcherPath)
        val visualHighlighterPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/VisualHighlighterManager.kt"),
        ).first { Files.exists(it) }
        val visualHighlighter = Files.readString(visualHighlighterPath)
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
        val patchOrchestrationBody = watcher.substringAfter("private fun applyPatch(patch: IpcPatch)")
            .substringBefore("private fun capturePatchApplyOnEdt")
        val patchCaptureBody = watcher.substringAfter("private fun capturePatchApplyOnEdt")
            .substringBefore("private fun computePatchOnWorker")
        val patchComputationBody = watcher.substringAfter("private fun computePatchOnWorker")
            .substringBefore("private fun applyComputedPatchOnEdt")
        val computedPatchApplyBody = watcher.substringAfter("private fun applyComputedPatchOnEdt")
            .substringBefore("private fun patchConflictKey")
        val workerProjectionBody = watcher
            .substringAfter("if (applied || wasNoOp) {")
            .substringBefore("StateProjectionBridge.recordEditorPatchApplied")

        assertTrue(
            "agent-doc IPC patch writes should not set the unsynced-local-operator flag",
            componentApplyBody.contains("CrdtReplicaManager.withAgentAppliedEditorMutation(") &&
                // `#agentpatchlineage`: the patch identity travels with the mutation.
                componentApplyBody.contains("patchId = patch.patchId") &&
                componentApplyBody.contains("postText = result") &&
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
                0 until socketApplyBody.indexOf("applyPatch(patch)") &&
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
        assertFalse(
            "editor capture must not cross the native ABI from the EDT",
            patchCaptureBody.contains("NativePatching."),
        )
        assertFalse(
            "computed editor mutation must not cross the native ABI from the EDT",
            computedPatchApplyBody.contains("NativePatching."),
        )
        assertTrue(
            "socket patch orchestration must capture on EDT, compute on its worker, then apply on EDT",
            patchOrchestrationBody.indexOf("capturePatchApplyOnEdt(patch)") in
                0 until patchOrchestrationBody.indexOf("computePatchOnWorker(patch, captured)") &&
                patchOrchestrationBody.indexOf("computePatchOnWorker(patch, captured)") in
                0 until patchOrchestrationBody.indexOf("applyComputedPatchOnEdt(patch, captured, computation)") &&
                patchComputationBody.contains("applyComponentPatchNative") &&
                patchComputationBody.contains("native patch computation must run off the IDEA event-dispatch thread"),
        )
        assertTrue(
            "the socket worker must publish the immutable post-apply content before acknowledging",
            workerProjectionBody.contains("writeEditorContentProjection"),
        )
        assertTrue(
            "late visual refresh callbacks must become inert after plugin generation disposal",
            visualHighlighter.contains("private val disposed = AtomicBoolean(false)") &&
                visualHighlighter.contains("private val refreshLifecycleLock = Any()") &&
                visualHighlighter.contains("synchronized(refreshLifecycleLock)") &&
                visualHighlighter.contains("catch (_: RejectedExecutionException)") &&
                visualHighlighter.contains("if (!disposed.compareAndSet(false, true)) return"),
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
    fun `an attested burst names the exact text its offsets are relative to`() {
        // #keylogrebase: a compaction (a non-operator projection) lands, then the
        // operator types. The ledger must be labelled with the text the keystrokes
        // were typed into -- the compacted buffer -- never with the merge baseline
        // the reporter looked up, which still names the pre-compaction text.
        val compacted = "queue:\n"
        val burst = prepareEditorOpLineageUtil(
            finalText = "queue:\n- é fix\n",
            ops = listOf(
                PendingEditorOp(offset = 0, oldFragment = "long history\n", newFragment = "", nonOperatorMutation = true),
                PendingEditorOp(offset = 7, oldFragment = "", newFragment = "- é dix\n", nonOperatorMutation = false),
                PendingEditorOp(offset = 11, oldFragment = "d", newFragment = "f", nonOperatorMutation = false),
            ),
        )!!
        assertEquals(compacted, burst.preText)
        assertEquals("queue:\n- é fix\n", burst.postText)
        assertEquals(
            listOf(
                PreparedEditorOp(opKind = "insert", byteOffset = 7L, insertText = "- é dix\n", deleteBytes = 0L),
                PreparedEditorOp(opKind = "delete", byteOffset = 12L, insertText = null, deleteBytes = 1L),
                PreparedEditorOp(opKind = "insert", byteOffset = 12L, insertText = "f", deleteBytes = 0L),
            ),
            burst.reports,
        )
        val json = attestedEditorOpsJsonUtil(burst)
        assertEquals(contentHashUtil(compacted), json.get("pre_hash").asString)
        assertEquals(contentHashUtil("queue:\n- é fix\n"), json.get("post_hash").asString)
        assertEquals(3, json.getAsJsonArray("ops").size())
    }

    @Test
    fun `keystrokes before a projection inside one burst are never attested`() {
        // The projection fenced the epoch the earlier keystroke belonged to; it is
        // relative to no text the ledger can name.
        val burst = prepareEditorOpLineageUtil(
            finalText = "ab",
            ops = listOf(
                PendingEditorOp(offset = 0, oldFragment = "", newFragment = "a", nonOperatorMutation = false),
                PendingEditorOp(offset = 1, oldFragment = "", newFragment = "b", nonOperatorMutation = true),
            ),
        )!!
        assertTrue(burst.reports.isEmpty())
        assertEquals("ab", burst.preText)
    }

    @Test
    fun `the attested hash is the same function as the native content hash`() {
        assertEquals(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            contentHashUtil(""),
        )
        assertEquals(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            contentHashUtil("abc"),
        )
    }

    @Test
    fun `an unreplayable burst is refused explicitly instead of returning no ops`() {
        // #opcapturedormant: `null` (the ops do not reconstruct the reported buffer)
        // and `emptyList()` (the burst held no operator op) are different diagnoses.
        // Collapsing them is what made every drop silent.
        assertEquals(
            "a burst with no operator-attributable op is empty, not unreplayable",
            emptyList<PreparedEditorOp>(),
            prepareEditorOpReports(
                finalText = "abc",
                ops = listOf(
                    PendingEditorOp(offset = 0, oldFragment = "", newFragment = "abc", nonOperatorMutation = true),
                ),
            ),
        )
        assertEquals(
            "ops that cannot be replayed against the reported buffer must be refused, not emptied",
            null,
            prepareEditorOpReports(
                finalText = "totally different",
                ops = listOf(
                    PendingEditorOp(offset = 0, oldFragment = "", newFragment = "abc", nonOperatorMutation = false),
                ),
            ),
        )
    }

    @Test
    fun `a burst the document outran is detected from the event sequence`() {
        // The reporter reads the buffer and drains the burst as two steps off the
        // EDT. A change landing between them produces text no replay can reach, so
        // the event sequence is what separates "requeue" from a silent drop.
        val burst = listOf(
            PendingEditorOp(offset = 0, oldFragment = "", newFragment = "a", nonOperatorMutation = false, eventSeq = 7L),
            PendingEditorOp(offset = 1, oldFragment = "", newFragment = "b", nonOperatorMutation = false, eventSeq = 8L),
        )
        assertEquals(
            "a quiet burst ends at the snapshot sequence",
            BurstSnapshotRelation.CURRENT,
            classifyBurstAgainstSnapshotUtil(burst, snapshotEventSeq = 8L),
        )
        assertEquals(
            "an op recorded after the snapshot means a later event owns the retry",
            BurstSnapshotRelation.OUTRAN_BY_LATER_EVENT,
            classifyBurstAgainstSnapshotUtil(burst, snapshotEventSeq = 7L),
        )
        assertEquals(
            "a snapshot ahead of every drained op is owed no event and must not wait",
            BurstSnapshotRelation.SNAPSHOT_AHEAD_WITHOUT_PENDING_EVENT,
            classifyBurstAgainstSnapshotUtil(burst, snapshotEventSeq = 9L),
        )
        assertEquals(
            "an empty burst has nothing to invalidate",
            BurstSnapshotRelation.CURRENT,
            classifyBurstAgainstSnapshotUtil(emptyList(), snapshotEventSeq = 9L),
        )
        assertEquals(
            "an op captured without a sequence must not be discarded by the guard",
            BurstSnapshotRelation.CURRENT,
            classifyBurstAgainstSnapshotUtil(
                listOf(PendingEditorOp(offset = 0, oldFragment = "", newFragment = "a", nonOperatorMutation = false)),
                snapshotEventSeq = 9L,
            ),
        )
    }

    @Test
    fun `GH 242 an undo whose stamp moved without an event is handed over on the first drain`() {
        // Operator typed 11 bytes, then one undo deleted them, returning the buffer to
        // the pre-typing text. The platform restores the pre-typing
        // `modificationStamp` on undo WITHOUT a DocumentEvent, so the old stamp
        // guard never matched again and refused the same op 32,277 times. The event
        // sequence only moves with a DocumentEvent, so the drain sees a quiet burst.
        val preTyping = "# plan\n\nhello world\n"
        val typed = "0123456789!"
        val offset = preTyping.indexOf("world")
        // Event 1..11 were the typing (already reported); event 12 is the undo.
        val undo = PendingEditorOp(
            offset = offset,
            oldFragment = typed,
            newFragment = "",
            nonOperatorMutation = false,
            eventSeq = 12L,
        )
        val snapshot = DocumentSnapshot(text = preTyping, eventSeq = 12L)

        assertEquals(
            BurstSnapshotRelation.CURRENT,
            classifyBurstAgainstSnapshotUtil(listOf(undo), snapshot.eventSeq),
        )
        assertEquals(
            "the undo replays against the post-undo buffer and reaches the ledger",
            listOf(
                PreparedEditorOp(
                    opKind = "delete",
                    byteOffset = offset.toLong(),
                    insertText = null,
                    deleteBytes = typed.length.toLong(),
                ),
            ),
            prepareEditorOpReports(snapshot.text, listOf(undo)),
        )
    }

    @Test
    fun `GH 242 a requeued burst is retried only when a new event arrives`() {
        // Model the drain loop with the tracker's own decision function: a refusal may
        // requeue ONLY when a later event exists, and that event is what drives the
        // next drain. With no further typing the loop must terminate on its own —
        // the old reporter scheduled a 75 ms retry here with no bound.
        var pending = mutableListOf(
            PendingEditorOp(offset = 0, oldFragment = "", newFragment = "a", nonOperatorMutation = false, eventSeq = 1L),
        )
        var text = "a"
        var seq = 1L
        var snapshotSeq = 0L // the snapshot was read before event 1 landed (raced)
        var refusals = 0
        var handedOver = 0
        var scheduledBoundaries = 1
        while (scheduledBoundaries > 0) {
            scheduledBoundaries--
            val drained = pending.toList()
            pending = mutableListOf()
            when (classifyBurstAgainstSnapshotUtil(drained, snapshotSeq)) {
                BurstSnapshotRelation.OUTRAN_BY_LATER_EVENT -> {
                    refusals++
                    pending.addAll(0, drained)
                    // No scheduleFullContentReport here: only an event schedules.
                    if (refusals == 1) {
                        // The racing event: it lands, and ITS documentChanged schedules.
                        seq++
                        text += "b"
                        pending.add(
                            PendingEditorOp(offset = 1, oldFragment = "", newFragment = "b", nonOperatorMutation = false, eventSeq = seq),
                        )
                        scheduledBoundaries++
                    }
                }
                else -> {
                    assertTrue(prepareEditorOpReports(text, drained) != null)
                    handedOver++
                }
            }
            snapshotSeq = seq // the next boundary snapshots after every landed event
        }
        assertEquals("the race is refused once", 1, refusals)
        assertEquals("then the requeued burst and the new op are handed over together", 1, handedOver)
        assertTrue("nothing is left behind", pending.isEmpty())

        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(trackerPath)
        val outranBranch = source.substringAfter("if (relation == BurstSnapshotRelation.OUTRAN_BY_LATER_EVENT) {")
            .substringBefore("return\n")
        assertTrue(
            "the raced burst is requeued for the event that outran it",
            outranBranch.contains("requeuePendingEditorOps(filePath, drainedOps)"),
        )
        assertFalse(
            "the refusal must not schedule its own retry: no edge, no retry",
            outranBranch.contains("scheduleFullContentReport("),
        )
        assertFalse(
            "modificationStamp moves without a DocumentEvent and must not be the race clock",
            source.contains("docStamp") || source.contains("eventSeq = event.document.modificationStamp"),
        )
    }

    @Test
    fun `every reporter refusal writes a receipt and keeps the captured burst`() {
        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(trackerPath)

        assertTrue(
            "the buffer text and its stamp must come from one read action",
            source.contains("DocumentSnapshot(document.text, documentEventSeq(document).get())"),
        )
        assertTrue(
            "documentChanged must sequence each captured op so the drain can detect the race",
            source.contains("eventSeq = documentEventSeq(event.document).incrementAndGet()"),
        )
        assertTrue(
            "the burst must be drained adjacent to the snapshot it is replayed against",
            source.contains("if (drainEditorOps) drainPendingEditorOps(filePath) else emptyList()"),
        )
        assertTrue(
            "a burst that never reached the reporter must be requeued, not dropped",
            source.contains("if (!burstHandedOff) requeuePendingEditorOps(filePath, drainedOps)"),
        )
        val refusalBody = source.substringAfter("private fun reportDrainedEditorOps")
            .substringBefore("#qnodemerge4wire Phase 4")
        for (reason in listOf(
            "OpCaptureRefusal.DOC_ADVANCED_DURING_DRAIN",
            "OpCaptureRefusal.SHADOW_REPLAY_MISMATCH",
            "OpCaptureRefusal.ALL_OPS_NON_OPERATOR",
        )) {
            assertTrue(
                "every silent early return must now name itself: $reason",
                refusalBody.contains(reason),
            )
        }
        assertEquals(
            "an unavailable merge base must be reported for both the null and empty case",
            2,
            source.split("OpCaptureRefusal.BASE_HASH_UNAVAILABLE").size - 1,
        )
    }

    @Test
    fun `an unresolvable merge base keeps the burst for a bounded number of boundaries`() {
        // #basehashdropsops: a null base hash is a property of the READER — the
        // native resolver failed to project document state this once — not of the
        // operator's typing. Dropping the burst on the first failure destroys
        // captured operator text a later boundary could still have stamped, which
        // is what every observed `base_hash_unavailable` refusal did.
        assertFalse("the first failure must retry", baseHashRetriesExhaustedUtil(1))
        assertFalse("a burst may wait several boundaries", baseHashRetriesExhaustedUtil(4))
        assertTrue(
            "the wait is bounded so a permanent failure cannot grow the ledger forever",
            baseHashRetriesExhaustedUtil(5),
        )
        assertTrue("and stays exhausted past the bound", baseHashRetriesExhaustedUtil(9))

        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(trackerPath)
        val drainBody = source.substringAfter("private fun reportDrainedEditorOps")
            .substringBefore("#qnodemerge4wire Phase 4")
        assertTrue(
            "a retryable refusal must requeue the burst rather than drop it",
            drainBody.contains("ReportOutcome.RETRYABLE") &&
                drainBody.contains("requeuePendingEditorOps(filePath, drainedOps)"),
        )
        assertTrue(
            "giving up must be its own receipt, never a silent discard",
            drainBody.contains("OpCaptureRefusal.BASE_HASH_RETRIES_EXHAUSTED"),
        )
        assertTrue(
            "an unresolvable base is the only retryable refusal",
            source.split("return ReportOutcome.RETRYABLE").size - 1 == 2,
        )
    }

    @Test
    fun `a handed-over burst states the four facts verification depends on`() {
        // #opcaptureliveread: the refusal receipt made a dormant ledger diagnosable,
        // but a ledger that DOES record still proved the live epoch generation, the
        // isOperatorDocumentEvent split, shadow-replay agreement, and merge-base
        // availability only by the ABSENCE of a refusal. A reporter whose listener
        // never ran writes neither, so "no refusal" is not evidence that all four
        // held — the same absence-is-evidence inversion #idlerevisionreactive names.
        val trackerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"),
        ).first { Files.exists(it) }
        val managerPath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(trackerPath)
        val manager = Files.readString(managerPath)

        assertTrue(
            "the reporter must resolve the live op-capture epoch, not a stand-in",
            source.contains("CrdtReplicaManager.liveOpCaptureEpochGeneration(filePath)") &&
                manager.contains("fun liveOpCaptureEpochGeneration(filePath: String): Long"),
        )
        assertTrue(
            "an absent epoch must read as unknown, not as generation 0",
            manager.contains("remoteEditorEffectGenerations[filePath]?.get() ?: -1L"),
        )
        assertTrue(
            "the isOperatorDocumentEvent split must reach the proof, not be recomputed there",
            source.contains("val operatorOps = drainedOps.count { !it.nonOperatorMutation }") &&
                source.contains("reportEditorOps(lib, filePath, lineage, operatorOps, drainedOps.size - operatorOps)"),
        )
        val reportBody = source.substringAfter("private fun reportEditorOps")
        assertTrue(
            "the success path must state the four facts before handing the burst over",
            reportBody.indexOf("logOpCaptureProof(") in 0 until
                reportBody.indexOf("agent_doc_record_editor_ops_json("),
        )
        assertTrue(
            "the proof must carry the merge base the ops are actually stamped against",
            reportBody.contains("baseHash = baseHash"),
        )
        assertTrue(
            "an older cdylib without the proof ABI must not break capture",
            source.substringAfter("private fun logOpCaptureProof")
                .substringBefore("private fun logOpCaptureRefusal")
                .contains("catch (_: UnsatisfiedLinkError)"),
        )
        assertFalse(
            "the proof is the success receipt; a refusal path must not also claim it",
            source.substringAfter("private fun reportDrainedEditorOps")
                .substringBefore("#qnodemerge4wire Phase 4")
                .contains("logOpCaptureProof("),
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
            .substringBefore("fun ensureOpenDocumentReplica")
        val prepareMutationBody = source
            .substringAfter("fun prepareNonOperatorEditorMutationOnWorker")
            .substringBefore("fun forceRefreshOpenDocumentReplica")
        val localMutationEpochBody = source
            .substringAfter("private fun advanceNonOperatorMutationEpoch")
            .substringBefore("\n        }")
        val remoteEditorApplyBody = source
            .substringAfter("private fun applyRemoteTextOnEdt")
            .substringBefore("private fun readRawDiskText")
        val replaceDeliveryBody = source
            .substringAfter("private fun applyReplaceDelivery")
            .substringBefore("private fun queueRemoteTextApply")
        val queueRemoteApplyBody = source
            .substringAfter("private fun queueRemoteTextApply")
            .substringBefore("private fun scheduleRemoteEditorApply")

        assertTrue(
            "CRDT documentChanged should retain the exact editor splice and debounce its CRDT forwarding",
            listenerBody.contains("CapturedLocalEditorEdit(") &&
                listenerBody.contains("offsetUtf16 = event.offset") &&
                listenerBody.contains("scheduleLocalEditorFlush(filePath)"),
        )
        assertTrue(
            "the first local edit should reconstruct its pre-edit base and enter the ordinary delta queue",
            listenerBody.contains("reconstructLocalEditorBaseTextUtil(visibleText, edit)") &&
                listenerBody.contains("shadows.putIfAbsent(filePath, beforeText)") &&
                listenerBody.contains("recordLocalEditorEdit(filePath, edit)"),
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
            "controller-bootstrap projection must wait for the CRDT replica while ordinary open reports stay asynchronous",
                source.contains("fun ensureOpenDocumentReplica(") &&
                source.contains("forceRefresh: Boolean = false") &&
                source.contains("bypassRegisterBackoff = forceRefresh") &&
                source.contains("replaceCached = forceRefresh") &&
                source.contains("expectedEditorTextAtSwap = if (forceRefresh) registrationText else null") &&
                source.contains("documentWorkers.forDocument(filePath).submit<Boolean> { trackedAttach() }") &&
                source.contains(".get(CRDT_AWAIT_ATTACH_TIMEOUT_MS, TimeUnit.MILLISECONDS)") &&
                source.contains("private const val CRDT_AWAIT_ATTACH_TIMEOUT_MS = 750L") &&
                source.contains("documentWorkers.forDocument(filePath).execute { trackedAttach() }") &&
                source.contains("retainCanonicalProjectionAfterRegistration(filePath, forwarder)"),
        )
        val forceRefreshAttachBody = source.substringAfter("fun ensureOpenDocumentReplica(")
            .substringBefore("private fun scheduleDeferredWriteReplayAfterRegistration(")
        assertFalse(
            "forced refresh must not replace the visible editor from a retained whole-document target",
            forceRefreshAttachBody.contains("deferredWriteReconnectContent(") ||
                forceRefreshAttachBody.contains("applyMinimalDocumentEditUtil(") ||
                forceRefreshAttachBody.contains("persistRemoteCrdtTextIfSafe("),
        )
        assertTrue(
            "forced refresh must fence the observed editor cut without adopting it",
            forceRefreshAttachBody.contains("val registrationText = text") &&
                forceRefreshAttachBody.contains("scheduleDeferredWriteReplayAfterRegistration(") &&
                forceRefreshAttachBody.contains("replaceCached = forceRefresh") &&
                source.contains("editorBufferText(filePath) != expectedEditorTextAtSwap") &&
                source.contains("forwarder.deregister()") &&
                source.contains("return null"),
        )
        val localFlushBody = source.substringAfter("private fun scheduleLocalEditorFlush(")
            .substringBefore("fun ensureOpenDocumentReplica(")
        assertTrue(
            "typing bursts must retain one pending-local fence and cancel the superseded queued flush",
                localFlushBody.contains("localEditorFlushPendingPaths.add(filePath)") &&
                    localFlushBody.contains("LOCAL_EDITOR_FLUSH_QUIET_MS") &&
                    localFlushBody.contains("localEditorRetryDelayMsUtil(retryFailureCount)") &&
                    localFlushBody.contains("localEditorRetryFailureCounts.compute(filePath)") &&
                    localFlushBody.contains("retryVersion == null && retryFailureCount == 0") &&
                    localFlushBody.contains("localEditorFlushTasks.put(filePath, scheduled)?.cancel(false)"),
        )
        assertTrue(
            "typing bursts must use a human-scale quiet window before durable publication",
            source.contains("internal const val LOCAL_EDITOR_FLUSH_QUIET_MS = 250L"),
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
                source.contains("FileDocumentManagerListener.TOPIC") &&
                source.contains("override fun beforeFileContentReload") &&
                source.contains("fileContentReloadingPaths[filePath] = System.currentTimeMillis()") &&
                source.contains("isReloadingFileContent(filePath)") &&
                source.contains("wholeTextReplaced = event.isWholeTextReplaced") &&
                source.contains("isDocumentUnsaved(event.document)") &&
                source.contains("stale-operator-event-fenced"),
        )
        val localDeltaBody = source.substringAfter("private fun forwardLocalEditsFromShadow(")
            .substringBefore("fun requestRemoteDrain(")
        assertTrue(
            "a stale local baseline must retain its typed delta and rebootstrap from exact controller canonical",
            localDeltaBody.contains("prepareLocalEditorEditsUtil(beforeText, currentEdits)") &&
                localDeltaBody.contains("forwarder.forwardLocalEdits(batch)") &&
                localDeltaBody.contains("localReplicaBaselineDecisionUtil(replicaText, beforeText)") &&
                localDeltaBody.contains("retainedCanonicalProjectionPaths.add(filePath)") &&
                localDeltaBody.contains("expectedCanonicalTextAtSwap = canonical") &&
                localDeltaBody.contains("requestRemoteDrain(filePath, \"captured-local-delta-retry\")") &&
                localDeltaBody.contains("recovery=canonical-rebootstrap-captured-local-delta"),
        )
        assertFalse(
            "a stale local baseline must never schedule whole-editor adoption",
            localDeltaBody.contains("scheduleStaleBaselineRecovery") ||
                localDeltaBody.contains("adoptExactEditorBaseline") ||
                localDeltaBody.contains("tryReadDocumentText(document)") ||
                localDeltaBody.contains("coalescedLocalEditUtil(beforeText, editorText)"),
        )
        val openReplicaBody = source.substringAfter("fun ensureOpenDocumentReplica(")
            .substringBefore("/**\n     * Publish the exact closing editor cut")
        assertTrue(
            "a routine Lazily content observation must not enqueue a full attach or advance the CRDT shadow",
            openReplicaBody.contains("if (!forceRefresh && forwarders[filePath]?.attached == true)") &&
                openReplicaBody.indexOf("if (!forceRefresh && forwarders[filePath]?.attached == true)") <
                openReplicaBody.indexOf("val attach = attach@{"),
        )
        assertTrue(
            "the serialized local delta must install its exact resulting shadow in the forwarder projection",
            localDeltaBody.contains("forwarder.forwardLocalEdits(batch)") &&
                localDeltaBody.contains("shadows[filePath] = editorText"),
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
            "replica replacement must fence the observed editor before projecting the canonical bootstrap",
            forwarderForBody.contains("expectedEditorTextAtSwap") &&
                forwarderForBody.contains("editorBufferText(filePath) != expectedEditorTextAtSwap") &&
                forwarderForBody.lastIndexOf("editorBufferText(filePath) != expectedEditorTextAtSwap") >
                forwarderForBody.indexOf("editorBufferText(filePath) != expectedEditorTextAtSwap") &&
                forwarderForBody.lastIndexOf("editorBufferText(filePath) != expectedEditorTextAtSwap") <
                forwarderForBody.indexOf("forwarders.replace(filePath, cached, forwarder)") &&
                forwarderForBody.contains("retainCanonicalProjectionAfterRegistration(filePath, forwarder)") &&
                !forwarderForBody.contains("forwarder.ensureEditorText(initialEditorText)"),
        )
        assertTrue(
            "visible editor applies must publish complete state without an ACK sidecar",
            source.contains("projectSettledVisibleState(") &&
                source.contains("pending.effectToken.endpoint,") &&
                source.contains("outcome.diskPersisted") &&
                !source.contains("pendingRemoteAckReplays") &&
                !source.contains("rememberPendingRemoteAcks") &&
                !source.contains("replayPendingRemoteAcks"),
        )
        val remoteDrainBody = source.substringAfter("private fun drainRemoteUpdatesFor")
            .substringBefore("/**\n     * D2")
        assertTrue(
            "remote CRDT update bursts should be merged before one editor apply instead of one invokeAndWait per update",
            remoteDrainBody.contains("appliedRemoteUpdates") &&
                remoteDrainBody.contains("val projectionExpectedText") &&
                remoteDrainBody.contains("queueRemoteTextApply(") &&
                remoteDrainBody.contains("projectionExpectedText,"),
        )
        val remoteApplyBody = source.substringAfter("private fun queueRemoteTextApply")
            .substringBefore("private fun recoverRejectedRemoteCanonical")
        assertFalse(
            "remote CRDT editor apply must not call native template normalization inside invokeAndWait",
            remoteApplyBody.contains("NativePatching.normalizeTemplateStructure"),
        )
        assertTrue(
            "remote CRDT template validation should publish into the shared reactive plane before the EDT apply",
            source.contains("private fun templateStructureState(") &&
                source.substringAfter("private fun templateStructureState(")
                    .contains("templateValidation.publish(filePath, lane, text)") &&
                source.substringAfter("private fun templateStructureState(")
                    .contains("projection.state.also"),
        )
        val guardRecoveryBody = source.substringAfter("private fun recoverRejectedRemoteCanonical")
            .substringBefore("private fun scheduleTemplateGuardRecoveryRetry")
        assertTrue(
            "template-guard recovery must replace the rejected replica and rebuild from the unchanged exact editor baseline",
            guardRecoveryBody.contains("retainedCanonicalProjectionPaths.add(filePath)") &&
                guardRecoveryBody.contains("replaceCached = true") &&
                guardRecoveryBody.contains("bootstrapFromControllerCanonical = true") &&
                guardRecoveryBody.contains("replacement.ensureEditorText(editorText)") &&
                guardRecoveryBody.contains(
                    "projectSettledVisibleState(filePath, replacement, editorText)",
                ),
        )
        assertFalse(
            "template-guard recovery must rebuild through the replacement CRDT replica, not a direct whole-buffer adopt RPC",
            guardRecoveryBody.contains("pushTextAdopt"),
        )
        assertTrue(
            "remote CRDT editor apply should use RelayCell backpressure and schedule bounded EDT work",
            source.contains("KeyedCoalescingRelay<String, PendingRemoteEditorApply>(REMOTE_EDITOR_APPLY_MERGE)") &&
                remoteApplyBody.contains("scheduleRemoteEditorApply()") &&
                source.contains("GenerationWork.invokeLater"),
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
                source.contains("template-validation-computed"),
        )
    }
}
