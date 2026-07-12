package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile

/**
 * Reports this editor's open-set to the reliable-sync liveness plane
 * (sidecar-retirement Phase 3C, design B).
 *
 * Thin-plugin contract: this listener only observes IDE open/close events. The
 * reactive liveness state lives in [ReliableSyncLivenessGraph] (a real lazily-kt
 * graph), and all durability + the controller socket live in the Rust FFI
 * ([AgentDocLib.agent_doc_reliable_sync_liveness_enqueue] /
 * [AgentDocLib.agent_doc_reliable_sync_liveness_flush]). The FFI enqueue is a
 * no-op unless the controller dual-run flag is on, so this is safe on every
 * install: the sidecars stay authoritative until the operator opts into the
 * cutover.
 *
 * The whole-editor-death signal (`Alive{false}`) is NOT reported here — a dead
 * editor cannot report — it is injected controller-side by the S4b OS
 * process-exit watcher.
 */
class ReliableSyncLivenessListener(private val project: Project) : FileEditorManagerListener {
    private val pid: Long = ProcessHandle.current().pid()
    private val graph = ReliableSyncLivenessGraph(pid)

    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
        val root = project.basePath ?: return
        reportOnPool(root, file.path) { documentHash -> graph.open(documentHash) }
    }

    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        val root = project.basePath ?: return
        reportOnPool(root, file.path) { documentHash -> graph.close(documentHash) }
    }

    /**
     * Resolve the canonical `document_hash`, derive the op batch from the reactive
     * graph, and push it — all off the EDT (the flush may do a controller RPC).
     * [buildOps] returns `null` to skip (e.g. a close of a never-opened file).
     */
    private fun reportOnPool(projectRoot: String, filePath: String, buildOps: (String) -> String?) {
        ApplicationManager.getApplication().executeOnPooledThread {
            val lib = AgentDocLib.get() ?: return@executeOnPooledThread
            val documentHash = resolveDocumentHash(lib, filePath) ?: return@executeOnPooledThread
            val opsJson = buildOps(documentHash) ?: return@executeOnPooledThread
            if (lib.agent_doc_reliable_sync_liveness_enqueue(projectRoot, documentHash, opsJson) == 0) {
                lib.agent_doc_reliable_sync_liveness_flush(projectRoot, documentHash)
            }
        }
    }

    private fun resolveDocumentHash(lib: AgentDocLib, filePath: String): String? {
        val ptr = lib.agent_doc_document_id_for_path(filePath) ?: return null
        return try {
            ptr.getString(0).takeUnless { it.isNullOrEmpty() }
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }
}
