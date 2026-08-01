package com.github.btakita.agentdoc

import com.google.gson.JsonArray
import com.google.gson.JsonObject
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
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

object EditorIdentity {
    val id: String = "jetbrains-${ProcessHandle.current().pid()}-${UUID.randomUUID()}"
}

internal data class PendingEditorOp(
    val offset: Int,
    val oldFragment: String,
    val newFragment: String,
    val nonOperatorMutation: Boolean,
)

internal data class PreparedEditorOp(
    val opKind: String,
    val byteOffset: Long,
    val insertText: String?,
    val deleteBytes: Long,
)

private const val OPERATOR_TEXT_AUTHORITY_CAPABILITY = "operator_text_authority_v1"
private const val LAZILY_TRANSPORT_RECEIPTS_CAPABILITY = "lazily_transport_receipts_v1"
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

internal fun prepareEditorOpReports(
    finalText: String,
    ops: List<PendingEditorOp>,
): List<PreparedEditorOp> {
    if (ops.isEmpty()) return emptyList()

    var shadow = reverseApplyEditorOps(finalText, ops) ?: return emptyList()
    val reports = mutableListOf<PreparedEditorOp>()
    for (op in ops) {
        val offset = op.offset
        if (offset < 0 || offset > shadow.length) return emptyList()
        val oldEnd = offset + op.oldFragment.length
        if (oldEnd > shadow.length) return emptyList()
        if (shadow.substring(offset, oldEnd) != op.oldFragment) return emptyList()

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

    if (shadow != finalText) return emptyList()
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
            if (SwingUtilities.isEventDispatchThread() || application.isReadAccessAllowed) {
                FileDocumentManager.getInstance().getDocument(file)
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
    private fun tryReadDocumentText(document: Document): String? {
        val application =
            com.intellij.openapi.application.ApplicationManager.getApplication() as? ApplicationEx
                ?: return null
        if (application.isReadAccessAllowed) return document.text
        val textRef = AtomicReference<String?>()
        return if (application.tryRunReadAction { textRef.set(document.text) }) {
            textRef.get()
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
            val text = tryReadDocumentText(document)
            if (text == null) {
                if (!requireReplica) {
                    scheduleFullContentReport(filePath, document)
                }
                return false
            }
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
            val opReports = prepareEditorOpReports(text, drainPendingEditorOps(filePath))
            reportEditorOps(lib, filePath, opReports)
        }
            true
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
 * #qnodemerge4wire Phase 4: report a coalesced editor burst as byte-offset
 * operations in one bounded native transaction. IntelliJ `DocumentEvent`
 * offsets/fragments are UTF-16; [prepareEditorOpReports] replays the burst
 * against each op's pre-edit shadow so the FFI receives UTF-8 byte units.
 */
private fun reportEditorOps(
    lib: AgentDocLib,
    filePath: String,
    ops: List<PreparedEditorOp>,
) {
    if (ops.isEmpty()) return
    // Resolve the base hash captured ops must align to; skip (diff-guess
    // fallback) when unavailable. One burst resolves this once, rather than
    // making a native base-hash call per keystroke.
    val baseHashPtr = lib.agent_doc_document_base_hash(filePath) ?: return
    val baseHash = try {
        baseHashPtr.getString(0)
        } finally {
            lib.agent_doc_free_string(baseHashPtr)
    }
    if (baseHash.isNullOrEmpty()) return

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
    lib.agent_doc_record_editor_ops_json(filePath, baseHash, batch.toString())
}

}
