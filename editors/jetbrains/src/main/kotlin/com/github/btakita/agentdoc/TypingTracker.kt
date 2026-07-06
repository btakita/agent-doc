package com.github.btakita.agentdoc

import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.UUID

object EditorIdentity {
    val id: String = "jetbrains-${ProcessHandle.current().pid()}-${UUID.randomUUID()}"
}

internal data class PendingEditorOp(
    val offset: Int,
    val oldFragment: String,
    val newFragment: String,
    val remoteCrdtApply: Boolean,
)

internal data class PreparedEditorOp(
    val opKind: String,
    val byteOffset: Long,
    val insertText: String?,
    val deleteBytes: Long,
)

private const val OPERATOR_TEXT_AUTHORITY_CAPABILITY = "operator_text_authority_v1"
private const val LAZILY_TRANSPORT_RECEIPTS_CAPABILITY = "lazily_transport_receipts_v1"
private val EDITOR_CAPABILITIES = listOf(
    OPERATOR_TEXT_AUTHORITY_CAPABILITY,
    LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
).joinToString(",")

// #stale-plugin-detect: report the real plugin version over FFI so the binary's
// stale-plugin detection is not blind. The IntelliJ plugin descriptor (patched
// plugin.xml <version>) is the reliable source; the jar-manifest read is a
// fallback for contexts where the descriptor is unavailable.
private fun pluginVersion(): String =
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

        if (!op.remoteCrdtApply) {
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
 * Tracks document changes and reports editor-buffer projections.
 *
 * On every .md document change, queues the lightweight
 * `agent_doc_document_changed()` marker and the coalesced live-buffer report off
 * the document listener path.
 *
 * Registered as a bulk DocumentListener in PluginLifecycleListener.
 */
object TypingTracker : DocumentListener {

    private const val CONTENT_REPORT_DELAY_MS = 75L
    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(TypingTracker::class.java)
    private val contentReportExecutor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-live-buffer-report").apply { isDaemon = true }
    }
    private val pendingContentReports = ConcurrentHashMap<String, ScheduledFuture<*>>()
    private val pendingEditorOps = ConcurrentHashMap<String, MutableList<PendingEditorOp>>()
    private val pendingNativeChangeMarkers = ConcurrentHashMap.newKeySet<String>()
    private val nativeChangeDrainQueued = AtomicBoolean(false)

    // #falsetyping-guard: paths with an unsaved *local operator* edit ahead of
    // disk. Set when a non-remoteCrdtApply document change lands; cleared whenever
    // the document is observed clean (fully flushed to disk) or closed. The CLI
    // visible-write guard re-merges on replica churn only when the reporting
    // editor proves there is no unsaved operator text — otherwise it fails closed
    // so operator text stays authoritative.
    private val unsyncedLocalEditPaths = ConcurrentHashMap.newKeySet<String>()

    override fun documentChanged(event: DocumentEvent) {
        val vFile = FileDocumentManager.getInstance().getFile(event.document) ?: return
        if (!vFile.name.endsWith(".md")) return
        val filePath = vFile.path

        // A remote CRDT-replica apply is replica churn, not operator typing. Report
        // the changed buffer, but do not mark it as an unsynced local edit.
        val remoteCrdtApply = CrdtReplicaManager.isApplyingRemote(filePath)
        if (!remoteCrdtApply) {
            // #falsetyping-guard: a genuine local operator edit is now ahead of
            // disk until saved. A remoteCrdtApply is replica churn, not operator
            // text, so it must NOT set this flag.
            unsyncedLocalEditPaths.add(filePath)
        }

        if (!remoteCrdtApply) {
            requestNativeDocumentChanged(filePath)
        }
        val op = PendingEditorOp(
            offset = event.offset,
            oldFragment = event.oldFragment.toString(),
            newFragment = event.newFragment.toString(),
            remoteCrdtApply = remoteCrdtApply,
        )
        recordPendingEditorOp(filePath, op)
        scheduleFullContentReport(filePath, event.document)
        LOG.debug("[native] document_changed queued content report: ${vFile.name} (remoteCrdtApply=$remoteCrdtApply)")
    }

    private fun requestNativeDocumentChanged(filePath: String) {
        pendingNativeChangeMarkers.add(filePath)
        scheduleNativeChangeDrain()
    }

    private fun scheduleNativeChangeDrain() {
        if (!nativeChangeDrainQueued.compareAndSet(false, true)) return
        contentReportExecutor.execute {
            try {
                drainNativeChangeMarkers()
            } finally {
                nativeChangeDrainQueued.set(false)
                if (pendingNativeChangeMarkers.isNotEmpty()) {
                    scheduleNativeChangeDrain()
                }
            }
        }
    }

    private fun drainNativeChangeMarkers() {
        val paths = pendingNativeChangeMarkers.toList()
        paths.forEach { pendingNativeChangeMarkers.remove(it) }
        if (paths.isEmpty()) return
        val lib = AgentDocLib.get() ?: return
        for (filePath in paths) {
            try {
                lib.agent_doc_document_changed(filePath)
            } catch (_: UnsatisfiedLinkError) {
                // older cdylib without the lightweight marker; fall back to local debounce
            } catch (_: NoSuchMethodError) {
                // older cdylib without the lightweight marker; fall back to local debounce
            }
        }
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

    /**
     * Acquire / refresh the plugin-owner lease for this editor with our LIVE pid.
     * The JetBrains plugin previously never called the acquire FFI (only VS Code
     * did, on patch handling), so it never registered a live lease — after an IDE
     * restart the stale lease kept a dead pid and the document read as headless
     * (not editor-attached), so the realtime/CRDT paths never engaged. Called on
     * document open + on each debounced buffer report so a restart re-establishes a
     * fresh lease. Best-effort: an older cdylib without the symbol is a no-op.
     */
    private fun refreshPluginOwner(lib: AgentDocLib, filePath: String) {
        try {
            lib.agent_doc_plugin_owner_try_acquire(
                filePath,
                EditorIdentity.id,
                ProcessHandle.current().pid(),
            )
        } catch (_: UnsatisfiedLinkError) {
        } catch (_: NoSuchMethodError) {
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
        // #falsetyping-guard: a closed document has no unsaved operator edits to
        // protect; drop any stale local-edit marker so a reopened buffer starts
        // from the conservative-but-current provenance.
        unsyncedLocalEditPaths.remove(filePath)
        contentReportExecutor.execute {
            val lib = AgentDocLib.get() ?: return@execute
            try {
                lib.agent_doc_plugin_owner_release(filePath, EditorIdentity.id)
            } catch (_: UnsatisfiedLinkError) {
            } catch (_: NoSuchMethodError) {
            }
            try {
                lib.agent_doc_document_closed_for_editor(filePath, EditorIdentity.id)
            } catch (_: UnsatisfiedLinkError) {
                // older cdylib without per-editor close support; stale sidecar cleanup
                // falls back to PID liveness checks in the binary.
            } catch (_: NoSuchMethodError) {
                // older cdylib without per-editor close support; stale sidecar cleanup
                // falls back to PID liveness checks in the binary.
            }
        }
    }

    fun publishLiveBufferNow(filePath: String): Boolean {
        val lib = AgentDocLib.get() ?: return false
        val file = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return false
        if (!file.name.endsWith(".md")) return false
        val document = com.intellij.openapi.application.ApplicationManager.getApplication()
            .runReadAction<com.intellij.openapi.editor.Document?> {
                FileDocumentManager.getInstance().getDocument(file)
            } ?: return false
        return reportFullContentNow(
            lib = lib,
            filePath = filePath,
            document = document,
            drainEditorOps = false,
            requireAuthority = true,
        )
    }

    private fun scheduleFullContentReport(
        filePath: String,
        document: com.intellij.openapi.editor.Document,
    ) {
        pendingContentReports.remove(filePath)?.cancel(false)
        val task = contentReportExecutor.schedule({
            try {
                val lib = AgentDocLib.get() ?: return@schedule
                reportFullContentNow(
                    lib = lib,
                    filePath = filePath,
                    document = document,
                    drainEditorOps = true,
                    requireAuthority = false,
                )
            } catch (_: UnsatisfiedLinkError) {
                // older cdylib without the compatibility content-report ABI; skip
            } catch (_: NoSuchMethodError) {
                // older cdylib without the compatibility content-report ABI; skip
            } catch (e: Throwable) {
                LOG.debug("[native] content report skipped: ${e.message}")
            } finally {
                pendingContentReports.remove(filePath)
            }
        }, CONTENT_REPORT_DELAY_MS, TimeUnit.MILLISECONDS)
        pendingContentReports[filePath] = task
    }

    private fun reportFullContentNow(
        lib: AgentDocLib,
        filePath: String,
        document: com.intellij.openapi.editor.Document,
        drainEditorOps: Boolean,
        requireAuthority: Boolean,
    ): Boolean {
        return try {
            // Heartbeat the plugin-owner lease with our live pid on each debounced
            // buffer report so the document stays editor-attached while open.
            refreshPluginOwner(lib, filePath)
            val text = com.intellij.openapi.application.ApplicationManager.getApplication()
                .runReadAction<String> { document.text }
            // #falsetyping-guard: derive replica-churn provenance. A document that
            // is fully flushed to disk has no unsaved edits at all, so clear any
            // stale local-edit marker. Otherwise the buffer is unsaved: the edits
            // are operator text only if a local (non-remoteCrdtApply) change landed
            // since the last clean observation.
            val unsaved = FileDocumentManager.getInstance().isDocumentUnsaved(document)
            if (!unsaved) {
                unsyncedLocalEditPaths.remove(filePath)
            }
            val noUnsavedOperatorEdits = !unsaved || filePath !in unsyncedLocalEditPaths
            if (requireAuthority) {
                val replicaRefreshAccepted = CrdtReplicaManager.ensureReplicaForOpenDocument(
                    filePath = filePath,
                    document = document,
                    editorText = text,
                    await = true,
                    forceRefresh = true,
                )
                if (!replicaRefreshAccepted) return false
            }
            val reported = try {
                lib.agent_doc_document_changed_digest_content_for_editor_v3(
                    filePath,
                    text,
                    EditorIdentity.id,
                    "jetbrains",
                    pluginVersion(),
                    EDITOR_CAPABILITIES,
                    if (noUnsavedOperatorEdits) 1 else 0,
                )
                true
            } catch (_: UnsatisfiedLinkError) {
                reportLiveBufferContentV2OrV1(lib, filePath, text, requireAuthority)
            } catch (_: NoSuchMethodError) {
                reportLiveBufferContentV2OrV1(lib, filePath, text, requireAuthority)
            }
            if (!reported) return false
            if (!requireAuthority) {
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
                for (op in opReports) {
                    reportEditorOp(lib, filePath, op)
                }
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
     * #falsetyping-guard: fall back from the v3 (replica-churn provenance) content
     * report to v2, then to v1, when running against an older cdylib that lacks
     * the newer symbol. Returns whether a report was delivered. Older binaries
     * simply omit the provenance flag (conservative fail-closed default), so this
     * degrades safely.
     */
    private fun reportLiveBufferContentV2OrV1(
        lib: AgentDocLib,
        filePath: String,
        text: String,
        requireAuthority: Boolean,
    ): Boolean {
        return try {
            lib.agent_doc_document_changed_digest_content_for_editor_v2(
                filePath,
                text,
                EditorIdentity.id,
                "jetbrains",
                pluginVersion(),
                EDITOR_CAPABILITIES,
            )
            true
        } catch (_: UnsatisfiedLinkError) {
            if (requireAuthority) false else {
                reportLiveBufferContentV1(lib, filePath, text)
                true
            }
        } catch (_: NoSuchMethodError) {
            if (requireAuthority) false else {
                reportLiveBufferContentV1(lib, filePath, text)
                true
            }
        }
    }

    private fun reportLiveBufferContentV1(lib: AgentDocLib, filePath: String, text: String) {
        try {
            lib.agent_doc_document_changed_digest_content_for_editor(
                filePath,
                text,
                EditorIdentity.id,
            )
        } catch (_: UnsatisfiedLinkError) {
            lib.agent_doc_document_changed_digest_content(filePath, text)
        } catch (_: NoSuchMethodError) {
            lib.agent_doc_document_changed_digest_content(filePath, text)
        }
    }

    /**
     * #qnodemerge4wire Phase 4: report a single editor change as byte-offset
     * [agent_doc_record_editor_op] op(s). IntelliJ `DocumentEvent` offsets/fragments
     * are UTF-16; [prepareEditorOpReports] replays the coalesced burst against
     * each op's pre-edit shadow so the FFI receives UTF-8 byte units.
     */
    private fun reportEditorOp(
        lib: AgentDocLib,
        filePath: String,
        op: PreparedEditorOp,
    ) {
        // Resolve the base hash captured ops must align to; skip (diff-guess
        // fallback) when unavailable.
        val baseHashPtr = lib.agent_doc_document_base_hash(filePath) ?: return
        val baseHash = try {
            baseHashPtr.getString(0)
        } finally {
            lib.agent_doc_free_string(baseHashPtr)
        }
        if (baseHash.isNullOrEmpty()) return

        lib.agent_doc_record_editor_op(
            filePath,
            baseHash,
            op.opKind,
            op.byteOffset,
            op.insertText,
            op.deleteBytes,
        )
    }

}
