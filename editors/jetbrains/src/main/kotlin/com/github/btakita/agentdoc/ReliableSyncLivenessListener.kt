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
 * [AgentDocLib.agent_doc_reliable_sync_liveness_flush]). The historical dual-run
 * flag can disable the channel for rollback; default-on delivery feeds the
 * authoritative, durably journaled controller projection.
 *
 * The whole-editor-death signal (`Alive{false}`) is NOT reported here — a dead
 * editor cannot report — it is injected controller-side by the S4b OS
 * process-exit watcher.
 */
class ReliableSyncLivenessListener(private val project: Project) : FileEditorManagerListener {
    private val pid: Long = ProcessHandle.current().pid()
    private val graph = ReliableSyncLivenessGraph(pid)

    init {
        // Project listeners can be created after the IDE restored its editor tabs;
        // seed that existing open set because no new fileOpened event is guaranteed.
        ApplicationManager.getApplication().invokeLater {
            if (!project.isDisposed) {
                FileEditorManager.getInstance(project).openFiles.forEach(::reportOpen)
            }
        }
    }

    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
        reportOpen(file)
    }

    private fun reportOpen(file: VirtualFile) {
        val root = project.basePath ?: return
        val filePath = file.path
        ApplicationManager.getApplication().executeOnPooledThread {
            val lib = AgentDocLib.get() ?: return@executeOnPooledThread
            // Scope liveness to agent-doc session documents only: a plain source file
            // opened as a tab must not enter the plane (it would over-count the
            // open-set vs the sidecar `open_agent_docs` ground truth). This disk read
            // is appropriate at open time — it is the moment we decide whether to
            // start tracking a possibly-random `.md` tab at all.
            if (lib.agent_doc_is_session_document(filePath) != 1) return@executeOnPooledThread
            val documentHash = resolveDocumentHash(lib, filePath) ?: return@executeOnPooledThread
            val opsJson = graph.open(documentHash) ?: return@executeOnPooledThread
            push(lib, root, documentHash, opsJson)
        }
    }

    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        val root = project.basePath ?: return
        val filePath = file.path
        ApplicationManager.getApplication().executeOnPooledThread {
            val lib = AgentDocLib.get() ?: return@executeOnPooledThread
            val documentHash = resolveDocumentHash(lib, filePath) ?: return@executeOnPooledThread
            // `#lzsync-close-no-disk-regate`: do NOT re-check `agent_doc_is_session_document`
            // here — it reads the file from disk, and a file can legitimately become
            // unreadable at close time (deleted, renamed, mid-git-checkout, or a
            // project tearing down) even though this editor genuinely opened it as a
            // tracked session document earlier. Re-gating on a disk read would
            // silently drop the compensating `Close` op, leaving the plane's OrSet
            // permanently "present" with no reaper (unlike the sidecar's `#lbreap`
            // scan). [ReliableSyncLivenessGraph.close] is itself the correct gate: it
            // returns null when this editor never opened the doc, which is exactly
            // the case a disk-read gate was trying to approximate.
            val opsJson = graph.close(documentHash) ?: return@executeOnPooledThread
            push(lib, root, documentHash, opsJson)
        }
    }

    private fun push(lib: AgentDocLib, projectRoot: String, documentHash: String, opsJson: String) {
        if (lib.agent_doc_reliable_sync_liveness_enqueue(projectRoot, documentHash, opsJson) == 0) {
            lib.agent_doc_reliable_sync_liveness_flush(projectRoot, documentHash)
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
