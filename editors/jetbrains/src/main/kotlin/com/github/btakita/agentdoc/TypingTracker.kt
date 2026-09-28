package com.github.btakita.agentdoc

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.application.ex.ApplicationEx
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import io.github.lazily.DebounceCore
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.UUID
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

object EditorIdentity {
    val id: String = "jetbrains-${ProcessHandle.current().pid()}-${UUID.randomUUID()}"
    private val replicaConnectionEpoch = AtomicLong(0)

    /**
     * Allocate a distinct transport identity for every native replica incarnation.
     *
     * The editor id is stable across an in-process native reload, while an old
     * manager can finish deregistering after its replacement has registered. A
     * generation suffix lets the relay recognize that late close as belonging to
     * the retired member instead of removing the replacement's identical client id.
     */
    internal fun nextReplicaConnectionIdentity(filePath: String): String =
        "$id:$filePath:refresh-${replicaConnectionEpoch.incrementAndGet()}"
}

internal data class PendingEditorOp(
    val offset: Int,
    val oldFragment: String,
    val newFragment: String,
    val nonOperatorMutation: Boolean,
    /**
     * `Document.modificationStamp` immediately after this change landed.
     * [UNKNOWN_DOCUMENT_STAMP] when the capturing caller has no stamp (offset-math
     * tests); the reporter always records the real stamp so the drain can tell a
     * quiet burst from one the document outran.
     */
    val docStamp: Long = UNKNOWN_DOCUMENT_STAMP,
)

/** Sentinel for "no document stamp was captured with this op". */
internal const val UNKNOWN_DOCUMENT_STAMP = -1L

/**
 * True when [snapshotStamp] is still the stamp the last captured op left behind.
 *
 * `#opcapturedormant`: the reporter reads the full buffer and drains the pending
 * burst as two separate steps off the EDT, so a change landing between them
 * produces a text that no replay of the drained ops can reach — and
 * [prepareEditorOpReports] then discards the whole burst. Comparing stamps makes
 * that race observable so the ops can be requeued for the next quiet boundary
 * instead of silently dropped.
 */
internal fun capturedBurstMatchesSnapshotUtil(
    ops: List<PendingEditorOp>,
    snapshotStamp: Long,
): Boolean {
    val lastStamp = ops.lastOrNull()?.docStamp ?: return true
    if (lastStamp == UNKNOWN_DOCUMENT_STAMP || snapshotStamp == UNKNOWN_DOCUMENT_STAMP) return true
    return lastStamp == snapshotStamp
}

/** Stable `reason=` tokens for `editor_op_capture_refused` receipts. */
internal object OpCaptureRefusal {
    const val ALL_OPS_NON_OPERATOR = "all_ops_non_operator"
    const val SHADOW_REPLAY_MISMATCH = "shadow_replay_mismatch"
    const val DOC_ADVANCED_DURING_DRAIN = "doc_advanced_during_drain"
    const val BASE_HASH_UNAVAILABLE = "base_hash_unavailable"
    const val BASE_HASH_RETRIES_EXHAUSTED = "base_hash_retries_exhausted"
}

/**
 * How many quiet boundaries a burst may wait for a resolvable merge base before
 * it is dropped (`#basehashdropsops`).
 *
 * A null base hash is a property of the *reader*, not of the operator's typing:
 * the native resolver failed to project document state this once. Dropping the
 * burst on the first failure destroys captured operator text that a later
 * boundary could still have stamped. Retrying forever instead grows the pending
 * ledger without bound whenever the failure is permanent, so the wait is bounded
 * and its exhaustion is its own receipt rather than a silent discard.
 */
private const val BASE_HASH_RETRY_LIMIT = 5

/**
 * Whether a refused hand-over may be retried with the same captured burst.
 *
 * `RETRYABLE` states that the burst itself is still valid and only the reader's
 * side failed; `TERMINAL` states that this burst can never be handed over (it was
 * reported, or replay disagreed, or nothing in it was operator text).
 */
internal enum class ReportOutcome { TERMINAL, RETRYABLE }

/**
 * Whether a burst that has waited `attempts` quiet boundaries for a resolvable
 * merge base must now be given up on.
 *
 * Separate from the drain so the bound is assertable without an IDE, a native
 * library, or a live controller.
 */
internal fun baseHashRetriesExhaustedUtil(attempts: Int): Boolean =
    attempts >= BASE_HASH_RETRY_LIMIT

/** A buffer text and the modification stamp it was read with, from one read action. */
internal data class DocumentSnapshot(
    val text: String,
    val stamp: Long,
)

internal data class PreparedEditorOp(
    val opKind: String,
    val byteOffset: Long,
    val insertText: String?,
    val deleteBytes: Long,
)

private const val OPERATOR_TEXT_AUTHORITY_CAPABILITY = "operator_text_authority_v1"
private const val LAZILY_TRANSPORT_RECEIPTS_CAPABILITY = "lazily_transport_receipts_v1"
private const val BOUNDED_EDITOR_SPLICES_CAPABILITY = "bounded_editor_splices_v1"
// #lzlosstree Phase 4: advertise that this plugin can exchange lossless-tree frames
// (it binds agent_doc_lossless_tree_render/project via LosslessTreeFrames). Kept in
// sync with agent_doc_debounce::LOSSLESS_TREE_CRDT_CAPABILITY on the binary side.
private const val LOSSLESS_TREE_CRDT_CAPABILITY = "lossless_tree_crdt_v1"
private const val NATIVE_HOT_RELOAD_CAPABILITY = "native_hot_reload_generation_v1"
// #ctrlkillreregister Tier 3: this plugin calls agent_doc_peer_replicas_missing about
// itself on startup and on detected controller-transport recovery. This is
// complementary to the controller's targeted restart push: a transparent controller
// restart does not trip the transport-recovery hook. Kept in sync with
// agent_doc_document_realtime::editor_contract::PEER_REPLICA_PULL_CAPABILITY.
private const val PEER_REPLICA_PULL_CAPABILITY = "peer_replica_pull_v1"
internal val EDITOR_CAPABILITIES = buildList {
    add(OPERATOR_TEXT_AUTHORITY_CAPABILITY)
    add(LAZILY_TRANSPORT_RECEIPTS_CAPABILITY)
    add(BOUNDED_EDITOR_SPLICES_CAPABILITY)
    add(LOSSLESS_TREE_CRDT_CAPABILITY)
    add(PEER_REPLICA_PULL_CAPABILITY)
    if (System.getProperty("os.name").lowercase().contains("linux")) {
        add(NATIVE_HOT_RELOAD_CAPABILITY)
    }
}.joinToString(",")

// #stale-plugin-detect: report the real plugin version over FFI so the binary's
// stale-plugin detection is not blind. The IntelliJ plugin descriptor (patched
// plugin.xml <version>) is the reliable source; the jar-manifest read is a
// fallback for contexts where the descriptor is unavailable.
internal fun pluginVersion(): String =
    com.intellij.ide.plugins.PluginManager.getPluginByClass(TypingTracker::class.java)?.version
        ?: TypingTracker::class.java.`package`?.implementationVersion
        ?: "unknown"

/**
 * Replay a coalesced burst into byte-offset reports, or `null` when the burst
 * cannot be replayed against [finalText].
 *
 * `#opcapturedormant`: `null` and an empty list are different diagnoses. `null`
 * means the recorded ops do not reconstruct the buffer we are reporting, so the
 * burst is unusable; an empty list means the burst held no operator-attributable
 * op at all. Collapsing both into `emptyList()` made every drop silent.
 */
internal fun prepareEditorOpReports(
    finalText: String,
    ops: List<PendingEditorOp>,
): List<PreparedEditorOp>? {
    if (ops.isEmpty()) return emptyList()

    var shadow = reverseApplyEditorOps(finalText, ops) ?: return null
    val reports = mutableListOf<PreparedEditorOp>()
    for (op in ops) {
        val offset = op.offset
        if (offset < 0 || offset > shadow.length) return null
        val oldEnd = offset + op.oldFragment.length
        if (oldEnd > shadow.length) return null
        if (shadow.substring(offset, oldEnd) != op.oldFragment) return null

        val byteOffset = shadow
            .substring(0, offset)
            .toByteArray(Charsets.UTF_8)
            .size
            .toLong()

        if (!op.nonOperatorMutation) {
            if (op.oldFragment.isNotEmpty()) {
                reports.add(
                    PreparedEditorOp(
                        opKind = "delete",
                        byteOffset = byteOffset,
                        insertText = null,
                        deleteBytes = op.oldFragment.toByteArray(Charsets.UTF_8).size.toLong(),
                    )
                )
            }
            if (op.newFragment.isNotEmpty()) {
                reports.add(
                    PreparedEditorOp(
                        opKind = "insert",
                        byteOffset = byteOffset,
                        insertText = op.newFragment,
                        deleteBytes = 0L,
                    )
                )
            }
        }

        shadow = shadow.substring(0, offset) + op.newFragment + shadow.substring(oldEnd)
    }

    if (shadow != finalText) return null
    return reports
}

private fun reverseApplyEditorOps(finalText: String, ops: List<PendingEditorOp>): String? {
    var shadow = finalText
    for (op in ops.asReversed()) {
        val offset = op.offset
        if (offset < 0 || offset > shadow.length) return null
        val newEnd = offset + op.newFragment.length
        if (newEnd > shadow.length) return null
        if (shadow.substring(offset, newEnd) != op.newFragment) return null
        shadow = shadow.substring(0, offset) + op.oldFragment + shadow.substring(newEnd)
    }
    return shadow
}

/**
 * Tracks document changes and reports Lazily current-document observations.
 *
 * On every .md document change, queues the coalesced current-document report off
 * the document listener path. Lazily owns edit ordering and current authority.
 *
 * Registered as a bulk DocumentListener in PluginLifecycleListener.
 */
object TypingTracker : DocumentListener {

    private const val CONTENT_REPORT_DELAY_MS = 75L
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(TypingTracker::class.java)
    private val contentReportExecutor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-current-document-report").apply { isDaemon = true }
    }
    /**
     * Per-document lazily rate-shape state. The compute core owns latest-value
     * coalescence; the scheduled future is only the logical-clock driver. This
     * prevents an older task's cleanup from deleting a newer report generation.
     */
    private class ContentReportState {
        val debounce = DebounceCore<Document>(CONTENT_REPORT_DELAY_MS)
        var future: ScheduledFuture<*>? = null
    }

    private val pendingContentReports = ConcurrentHashMap<String, ContentReportState>()
    private val pendingEditorOps = ConcurrentHashMap<String, MutableList<PendingEditorOp>>()

    // `#basehashdropsops`: consecutive quiet boundaries at which the native
    // resolver returned no merge base for this path. Reset whenever a base hash
    // does resolve, so a transient failure never counts toward the bound.
    private val baseHashRetries = ConcurrentHashMap<String, Int>()

    // #falsetyping-guard: paths with an unsaved *local operator* edit ahead of
    // disk. Set only when an operator-attributable document change lands; cleared whenever
    // the document is observed clean (fully flushed to disk) or closed. The CLI
    // visible-write guard re-merges on replica churn only when the reporting
    // editor proves there is no unsaved operator text — otherwise it fails closed
    // so operator text stays authoritative.
    private val unsyncedLocalEditPaths = ConcurrentHashMap.newKeySet<String>()

    override fun documentChanged(event: DocumentEvent) {
        val vFile = FileDocumentManager.getInstance().getFile(event.document) ?: return
        if (!vFile.name.endsWith(".md")) return
        val filePath = vFile.path

        // CP/agent projections and whole-buffer file-cache reloads are not
        // operator typing. They may be reported as visibility observations, but
        // they must never originate editor -> CP document operations.
        val operatorEdit = CrdtReplicaManager.isOperatorDocumentEvent(filePath, event)
        val nonOperatorMutation = !operatorEdit
        if (operatorEdit) {
            // #falsetyping-guard: a genuine local operator edit is now ahead of
            // disk until saved. CP projection/cache churn must NOT set this flag.
            unsyncedLocalEditPaths.add(filePath)
        }

        val op = PendingEditorOp(
            offset = event.offset,
            oldFragment = event.oldFragment.toString(),
            newFragment = event.newFragment.toString(),
            nonOperatorMutation = nonOperatorMutation,
            docStamp = event.document.modificationStamp,
        )
        recordPendingEditorOp(filePath, op)
        scheduleFullContentReport(filePath, event.document)
        LOG.debug("[native] document_changed queued content report: ${vFile.name} (operatorEdit=$operatorEdit)")
    }

    private fun recordPendingEditorOp(filePath: String, op: PendingEditorOp) {
        pendingEditorOps.compute(filePath) { _, existing ->
            (existing ?: mutableListOf()).also { it.add(op) }
        }
    }

    /**
     * Put a drained burst back at the head of the pending list.
     *
     * `#opcapturedormant`: a drain that cannot be reported must not destroy the
     * operator's captured ops — the next quiet boundary can still record them
     * against a consistent buffer snapshot.
     */
    private fun requeuePendingEditorOps(filePath: String, ops: List<PendingEditorOp>) {
        if (ops.isEmpty()) return
        pendingEditorOps.compute(filePath) { _, existing ->
            (existing ?: mutableListOf()).also { it.addAll(0, ops) }
        }
    }

    /**
     * State the four facts a handed-over burst proved (`#opcaptureliveread`).
     *
     * The refusal receipt made a dormant ledger diagnosable; this is its positive
     * counterpart. Without it, a burst that DID reach the record FFI proved the live
     * epoch generation, the operator/non-operator classification, shadow-replay
     * agreement, and merge-base availability only by the ABSENCE of a refusal — and
     * a reporter whose listener never ran writes neither, so reading "no refusal" as
     * "all four held" is the same absence-is-evidence inversion
     * `#idlerevisionreactive` names.
     */
    private fun logOpCaptureProof(
        lib: AgentDocLib,
        filePath: String,
        operatorOps: Int,
        nonOperatorOps: Int,
        shadowReplayAgreed: Boolean,
        baseHash: String?,
    ) {
        try {
            lib.agent_doc_log_editor_op_capture_proof(
                filePath,
                CrdtReplicaManager.liveOpCaptureEpochGeneration(filePath),
                operatorOps.toLong(),
                nonOperatorOps.toLong(),
                if (shadowReplayAgreed) 1 else 0,
                baseHash,
            )
        } catch (_: UnsatisfiedLinkError) {
            // older cdylib without the proof-receipt ABI; capture still proceeds
        } catch (_: NoSuchMethodError) {
            // older cdylib without the proof-receipt ABI; capture still proceeds
        } catch (e: Throwable) {
            LOG.debug("[op-capture] proof receipt skipped: ${e.message}")
        }
    }

    private fun logOpCaptureRefusal(
        lib: AgentDocLib,
        filePath: String,
        reason: String,
        detail: String,
    ) {
        try {
            lib.agent_doc_log_editor_op_capture_refusal(filePath, reason, detail)
        } catch (_: UnsatisfiedLinkError) {
            // older cdylib without the refusal-receipt ABI; the debug log still names it
        } catch (_: NoSuchMethodError) {
            // older cdylib without the refusal-receipt ABI; the debug log still names it
        } catch (e: Throwable) {
            LOG.debug("[native] op-capture refusal receipt skipped: ${e.message}")
        }
        LOG.debug("[native] op capture refused for $filePath: reason=$reason detail=$detail")
    }

    private fun drainPendingEditorOps(filePath: String): List<PendingEditorOp> {
        var drained: List<PendingEditorOp> = emptyList()
        pendingEditorOps.compute(filePath) { _, existing ->
            if (existing != null) {
                drained = existing.toList()
            }
            null
        }
        return drained
    }

    fun reportOpenMarkdownDocuments(project: Project) {
        for (file in FileEditorManager.getInstance(project).openFiles) {
            scheduleOpenDocumentReport(file)
        }
    }

    fun scheduleOpenDocumentReport(file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        val document = FileDocumentManager.getInstance().getDocument(file) ?: return
        scheduleFullContentReport(file.path, document)
    }

    fun clearOpenDocumentReport(file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        val filePath = file.path
        val application = com.intellij.openapi.application.ApplicationManager.getApplication()
        val closingDocument =
            if (application.isReadAccessAllowed) {
                FileDocumentManager.getInstance().getDocument(file)
            } else if (SwingUtilities.isEventDispatchThread()) {
                // `#edt-no-implicit-read`: being ON the EDT is NOT read access.
                // Older platforms granted the EDT an implicit read lock, so
                // `isEventDispatchThread()` was a valid stand-in; modern ones
                // require an explicit read (or write-intent) action and
                // `softAssertReadAccess` throws instead. Operator-reported
                // 2026-09-28 closing a tab: `fileClosed` is delivered on the EDT,
                // took this branch with `isReadAccessAllowed == false`, and
                // `FileDocumentManagerBase.getDocument` raised
                // `Read access is allowed from inside read-action only`.
                // Take the read action explicitly rather than asserting one.
                ReadAction.compute<Document?, RuntimeException> {
                    FileDocumentManager.getInstance().getDocument(file)
                }
            } else {
                val applicationEx = application as? ApplicationEx
                val document = AtomicReference<Document?>()
                if (
                    applicationEx?.tryRunReadAction {
                        document.set(FileDocumentManager.getInstance().getDocument(file))
                    } == true
                ) {
                    document.get()
                } else {
                    null
                }
            }
        pendingContentReports.computeIfPresent(filePath) { _, state ->
            synchronized(state) {
                state.future?.cancel(false)
                state.future = null
            }
            null
        }
        contentReportExecutor.execute {
            val lib = AgentDocLib.get() ?: return@execute
            // Closing a tab does not imply saving its Document. Publish the
            // exact final cut through the serialized Lazily replica worker
            // before releasing liveness; otherwise a queued deletion can be
            // resurrected from disk during retained-response recovery.
            if (closingDocument != null &&
                !CrdtReplicaManager.publishClosingDocumentCut(filePath, closingDocument)
            ) {
                LOG.warn(
                    "[native] final Lazily editor cut was not published for $filePath; " +
                        "retaining editor authority instead of emitting a lossy close",
                )
                return@execute
            }
            try {
                lib.agent_doc_document_closed_for_editor(filePath, EditorIdentity.id)
            } catch (_: UnsatisfiedLinkError) {
            // Older cdylib without per-editor reliable-sync close support.
            } catch (_: NoSuchMethodError) {
                // Older cdylib without per-editor reliable-sync close support.
            }
            unsyncedLocalEditPaths.remove(filePath)
        }
    }

    fun observeLazilyCurrentNow(filePath: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        val file = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return false
        if (!file.name.endsWith(".md")) return false
        val documentRef = AtomicReference<Document?>()
        val application =
            com.intellij.openapi.application.ApplicationManager.getApplication() as? ApplicationEx
                ?: return false
        val readAccepted =
            application.tryRunReadAction {
                documentRef.set(FileDocumentManager.getInstance().getDocument(file))
            }
        if (!readAccepted) return false
        val document = documentRef.get() ?: return false
        return reportFullContentNow(
            lib = lib,
            filePath = filePath,
            document = document,
            drainEditorOps = false,
            requireReplica = true,
        )
    }

    fun hasUnsyncedOperatorEdits(filePath: String): Boolean =
        filePath in unsyncedLocalEditPaths

    fun rekeyDocumentPath(oldPath: String, newPath: String, document: Document) {
        if (oldPath == newPath) return
        pendingContentReports.remove(oldPath)?.let { state ->
            synchronized(state) {
                state.future?.cancel(false)
                state.future = null
            }
        }
        pendingEditorOps.remove(oldPath)?.let { oldOps ->
            pendingEditorOps.compute(newPath) { _, existing ->
                (existing ?: mutableListOf()).also { it.addAll(0, oldOps) }
            }
        }
        if (unsyncedLocalEditPaths.remove(oldPath)) {
            unsyncedLocalEditPaths.add(newPath)
        }
        scheduleFullContentReport(newPath, document)
    }

    private fun scheduleFullContentReport(
        filePath: String,
        document: Document,
    ) {
        pendingContentReports.compute(filePath) { _, existing ->
            val state = existing ?: ContentReportState()
            synchronized(state) {
                state.debounce.input(monotonicMillis(), document)
                state.future?.cancel(false)
                state.future = contentReportExecutor.schedule(
                    { drainFullContentReport(filePath, state) },
                    CONTENT_REPORT_DELAY_MS,
                    TimeUnit.MILLISECONDS,
                )
            }
            state
        }
    }

    private fun drainFullContentReport(filePath: String, state: ContentReportState) {
        var document: Document? = null
        pendingContentReports.compute(filePath) { _, current ->
            if (current !== state) return@compute current
            synchronized(state) {
                val emitted = state.debounce.tick(monotonicMillis())
                if (emitted == null) {
                    // Scheduled executors may wake slightly before the logical quiet
                    // boundary. Keep one coalesced driver rather than dropping work.
                    state.future = contentReportExecutor.schedule(
                        { drainFullContentReport(filePath, state) },
                        CONTENT_REPORT_DELAY_MS,
                        TimeUnit.MILLISECONDS,
                    )
                    state
                } else {
                    state.future = null
                    document = emitted
                    null
                }
            }
        }
        val emittedDocument = document ?: return

        try {
            val lib = AgentDocLib.get() ?: return
            reportFullContentNow(
                lib = lib,
                filePath = filePath,
                document = emittedDocument,
                drainEditorOps = true,
                requireReplica = false,
            )
        } catch (_: UnsatisfiedLinkError) {
            // older cdylib without the compatibility content-report ABI; skip
        } catch (_: NoSuchMethodError) {
            // older cdylib without the compatibility content-report ABI; skip
        } catch (e: Throwable) {
            LOG.debug("[native] content report skipped: ${e.message}")
        }
    }

    private fun monotonicMillis(): Long = System.nanoTime() / 1_000_000L

    /**
     * Native listener callbacks must never queue behind IDEA's write-intent
     * permit. A blocking read here prevents listener shutdown, which in turn
     * prevents native reload and can retain the callback thread indefinitely.
     */
    private fun tryReadDocumentText(document: Document): String? =
        tryReadDocumentSnapshot(document)?.text

    /**
     * Read the buffer text and its modification stamp inside ONE read action.
     *
     * `#opcapturedormant`: the stamp is only a usable race detector if it is
     * consistent with the text it is paired with, so both have to come out of the
     * same read action.
     */
    private fun tryReadDocumentSnapshot(document: Document): DocumentSnapshot? {
        val application =
            com.intellij.openapi.application.ApplicationManager.getApplication() as? ApplicationEx
                ?: return null
        if (application.isReadAccessAllowed) {
            return DocumentSnapshot(document.text, document.modificationStamp)
        }
        val snapshotRef = AtomicReference<DocumentSnapshot?>()
        return if (
            application.tryRunReadAction {
                snapshotRef.set(DocumentSnapshot(document.text, document.modificationStamp))
            }
        ) {
            snapshotRef.get()
        } else {
            null
        }
    }

    private fun reportFullContentNow(
        lib: AgentDocLib,
        filePath: String,
        document: com.intellij.openapi.editor.Document,
        drainEditorOps: Boolean,
        requireReplica: Boolean,
    ): Boolean {
        return try {
            val snapshot = tryReadDocumentSnapshot(document)
            if (snapshot == null) {
                if (!requireReplica) {
                    scheduleFullContentReport(filePath, document)
                }
                return false
            }
            val text = snapshot.text
            // `#opcapturedormant`: drain the burst adjacent to the snapshot it will
            // be replayed against. The observed-content FFI and the replica ensure
            // below can each block, and every editor change landing in that window
            // used to poison the whole batch on the way out.
            val drainedOps =
                if (drainEditorOps) drainPendingEditorOps(filePath) else emptyList()
            var burstHandedOff = false
            try {
            // #falsetyping-guard: derive replica-churn provenance. A document that
            // is fully flushed to disk has no unsaved edits at all, so clear any
            // stale local-edit marker. Otherwise the buffer is unsaved: the edits
            // are operator text only if an operator-attributable change landed
            // since the last clean observation.
            val unsaved = FileDocumentManager.getInstance().isDocumentUnsaved(document)
            if (!unsaved) {
                unsyncedLocalEditPaths.remove(filePath)
            }
            val noUnsavedOperatorEdits = !unsaved || filePath !in unsyncedLocalEditPaths
            if (requireReplica) {
                val replicaAvailable = CrdtReplicaManager.ensureReplicaForOpenDocument(
                    filePath = filePath,
                    document = document,
                    editorText = text,
                    await = true,
                    forceRefresh = false,
                )
                if (!replicaAvailable) return false
            }
            lib.agent_doc_lazily_current_observed_v1(
                filePath,
                text,
                EditorIdentity.id,
                "jetbrains",
                pluginVersion(),
                EDITOR_CAPABILITIES,
                if (noUnsavedOperatorEdits) 1 else 0,
            )
            if (!requireReplica) {
                CrdtReplicaManager.ensureReplicaForOpenDocument(
                    filePath = filePath,
                    document = document,
                    editorText = text,
                    await = false,
                    forceRefresh = false,
                )
            }
            LOG.debug("[native] document_changed content reported: $filePath")
            if (drainEditorOps) {
                burstHandedOff = true
                reportDrainedEditorOps(lib, filePath, document, snapshot, drainedOps)
            }
            true
            } finally {
                // Any exit that never reached the reporter keeps the operator's
                // captured ops for the next quiet boundary (`#opcapturedormant`).
                if (!burstHandedOff) requeuePendingEditorOps(filePath, drainedOps)
            }
        } catch (_: UnsatisfiedLinkError) {
            false
        } catch (_: NoSuchMethodError) {
            false
        } catch (e: Throwable) {
            LOG.debug("[native] content report skipped: ${e.message}")
            false
        }
    }

    /**
     * Resolve a drained burst into captured ops, naming every refusal.
     *
     * `#opcapturedormant`: each outcome here used to be an unlogged early return, so
     * a permanently dormant ledger and an operator who never typed produced the same
     * evidence — zero `editor_ops_recorded` AND zero `editor_ops_record_failed`.
     */
    private fun reportDrainedEditorOps(
        lib: AgentDocLib,
        filePath: String,
        document: Document,
        snapshot: DocumentSnapshot,
        drainedOps: List<PendingEditorOp>,
    ) {
        if (drainedOps.isEmpty()) return
        if (!capturedBurstMatchesSnapshotUtil(drainedOps, snapshot.stamp)) {
            // The buffer moved on after the snapshot: keep the ops and retry at the
            // next quiet boundary rather than replaying them against stale text.
            requeuePendingEditorOps(filePath, drainedOps)
            scheduleFullContentReport(filePath, document)
            logOpCaptureRefusal(
                lib,
                filePath,
                OpCaptureRefusal.DOC_ADVANCED_DURING_DRAIN,
                "ops=${drainedOps.size} requeued=true",
            )
            return
        }
        val opReports = prepareEditorOpReports(snapshot.text, drainedOps)
        if (opReports == null) {
            logOpCaptureRefusal(
                lib,
                filePath,
                OpCaptureRefusal.SHADOW_REPLAY_MISMATCH,
                "ops=${drainedOps.size} operator_ops=${drainedOps.count { !it.nonOperatorMutation }}",
            )
            return
        }
        if (opReports.isEmpty()) {
            logOpCaptureRefusal(
                lib,
                filePath,
                OpCaptureRefusal.ALL_OPS_NON_OPERATOR,
                "ops=${drainedOps.size}",
            )
            return
        }
        val operatorOps = drainedOps.count { !it.nonOperatorMutation }
        val outcome =
            reportEditorOps(lib, filePath, opReports, operatorOps, drainedOps.size - operatorOps)
        if (outcome == ReportOutcome.RETRYABLE) {
            // `#basehashdropsops`: the burst is intact and still replayable — only
            // the merge base was unresolvable at this boundary. Keep it, exactly as
            // a drain that raced the buffer does, and let the next quiet boundary
            // try again against a resolver that may have recovered.
            val attempts = baseHashRetries.merge(filePath, 1, Int::plus) ?: 1
            if (baseHashRetriesExhaustedUtil(attempts)) {
                baseHashRetries.remove(filePath)
                logOpCaptureRefusal(
                    lib,
                    filePath,
                    OpCaptureRefusal.BASE_HASH_RETRIES_EXHAUSTED,
                    "ops=${drainedOps.size} attempts=$attempts",
                )
                return
            }
            requeuePendingEditorOps(filePath, drainedOps)
            scheduleFullContentReport(filePath, document)
            return
        }
        baseHashRetries.remove(filePath)
    }

/**
 * #qnodemerge4wire Phase 4: report a coalesced editor burst as byte-offset
 * operations in one bounded native transaction. IntelliJ `DocumentEvent`
 * offsets/fragments are UTF-16; [prepareEditorOpReports] replays the burst
 * against each op's pre-edit shadow so the FFI receives UTF-8 byte units.
 */
private fun reportEditorOps(
    lib: AgentDocLib,
    filePath: String,
    ops: List<PreparedEditorOp>,
    operatorOps: Int,
    nonOperatorOps: Int,
): ReportOutcome {
    if (ops.isEmpty()) return ReportOutcome.TERMINAL
    // Resolve the base hash captured ops must align to; skip (diff-guess
    // fallback) when unavailable. One burst resolves this once, rather than
    // making a native base-hash call per keystroke.
    val baseHashPtr = lib.agent_doc_document_base_hash(filePath)
    if (baseHashPtr == null) {
        logOpCaptureRefusal(
            lib,
            filePath,
            OpCaptureRefusal.BASE_HASH_UNAVAILABLE,
            "ops=${ops.size} base_hash=null",
        )
        return ReportOutcome.RETRYABLE
    }
    val baseHash = try {
        baseHashPtr.getString(0)
        } finally {
            lib.agent_doc_free_string(baseHashPtr)
    }
    if (baseHash.isNullOrEmpty()) {
        logOpCaptureRefusal(
            lib,
            filePath,
            OpCaptureRefusal.BASE_HASH_UNAVAILABLE,
            "ops=${ops.size} base_hash=empty",
        )
        return ReportOutcome.RETRYABLE
    }

    val batch = JsonArray()
    for (op in ops) {
        batch.add(JsonObject().apply {
            addProperty("kind", op.opKind)
            addProperty("offset", op.byteOffset)
            if (op.opKind == "insert") {
                addProperty("text", op.insertText ?: "")
            } else {
                addProperty("len", op.deleteBytes)
            }
        })
    }
    // `#opcaptureliveread`: state the four facts BEFORE the record call, so a proof
    // receipt with no `editor_ops_recorded` beside it is itself the diagnosis —
    // the reporter got all the way here and the FFI still wrote nothing.
    logOpCaptureProof(
        lib,
        filePath,
        operatorOps = operatorOps,
        nonOperatorOps = nonOperatorOps,
        shadowReplayAgreed = true,
        baseHash = baseHash,
    )
    lib.agent_doc_record_editor_ops_json(filePath, baseHash, batch.toString())
    return ReportOutcome.TERMINAL
}

}
