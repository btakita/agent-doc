package com.github.btakita.agentdoc

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap

internal class PathTransitionFrameLedger {
    private val pending = ConcurrentHashMap<String, String>()

    fun retain(key: String, produce: () -> String): String =
        pending.computeIfAbsent(key) { produce() }

    fun acknowledge(key: String, frame: String) {
        pending.remove(key, frame)
    }
}

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
    enum class PathTransitionOutcome {
        Projected,
        NotSessionDocument,
        NoLiveEditor,
        Retry,
    }
    private val pid: Long = ProcessHandle.current().pid()
    private val graph = ReliableSyncLivenessGraph(pid)
    private val projectRoots = ConcurrentHashMap<String, String>()
    private val pathTransitionFrames = PathTransitionFrameLedger()

    init {
        instances[project] = this
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
        val fallbackRoot = project.basePath ?: return
        val filePath = file.path
        ApplicationManager.getApplication().executeOnPooledThread {
            val lib = AgentDocLib.get() ?: return@executeOnPooledThread
            // Scope liveness to agent-doc session documents only: a plain source file
            // opened as a tab must not enter the plane (it would over-count the
            // session-document scope). This disk read
            // is appropriate at open time — it is the moment we decide whether to
            // start tracking a possibly-random `.md` tab at all.
            if (lib.agent_doc_is_session_document(filePath) != 1) return@executeOnPooledThread
            val documentHash = resolveDocumentHash(lib, filePath) ?: return@executeOnPooledThread
            // The owning controller is the nearest agent-doc root, not
            // necessarily the IntelliJ project base. A nested submodule has its
            // own controller; publishing `Open` to the outer project while CRDT
            // registration goes to the nested root leaves the nested controller
            // at detached authority and lets Compact Exchange write behind the
            // open editor.
            val root = NativePatching.resolveProjectPath(filePath)?.first ?: fallbackRoot
            projectRoots[documentHash] = root
            val opsJson = graph.open(
                documentHash,
                filePath,
                EditorIdentity.id,
                "jetbrains",
                pluginVersion(),
                EDITOR_CAPABILITIES,
            ) ?: return@executeOnPooledThread
            push(lib, root, documentHash, opsJson)
        }
    }

    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        val fallbackRoot = project.basePath ?: return
        val filePath = file.path
        ApplicationManager.getApplication().executeOnPooledThread {
            val lib = AgentDocLib.get() ?: return@executeOnPooledThread
            val documentHash = resolveDocumentHash(lib, filePath) ?: return@executeOnPooledThread
            val root =
                projectRoots.remove(documentHash)
                    ?: NativePatching.resolveProjectPath(filePath)?.first
                    ?: fallbackRoot
            // `#lzsync-close-no-disk-regate`: do NOT re-check `agent_doc_is_session_document`
            // here — it reads the file from disk, and a file can legitimately become
            // unreadable at close time (deleted, renamed, mid-git-checkout, or a
            // project tearing down) even though this editor genuinely opened it as a
            // tracked session document earlier. Re-gating on a disk read would
            // silently drop the compensating `Close` op, leaving the plane's OrSet
            // permanently "present". [ReliableSyncLivenessGraph.close] is itself the correct gate: it
            // returns null when this editor never opened the doc, which is exactly
            // the case a disk-read gate was trying to approximate.
            val opsJson = graph.close(documentHash) ?: return@executeOnPooledThread
            push(lib, root, documentHash, opsJson)
        }
    }

    private fun reportMoveNow(oldPath: String, newPath: String): PathTransitionOutcome {
        val lib = AgentDocLib.get() ?: return PathTransitionOutcome.Retry
        if (lib.agent_doc_is_session_document(newPath) != 1) {
            return PathTransitionOutcome.NotSessionDocument
        }
        val oldDocumentHash =
            if (File(oldPath).exists()) {
                resolveDocumentHash(lib, oldPath)
            } else {
                sha256Text(File(oldPath).absoluteFile.toPath().normalize().toString())
            } ?: return PathTransitionOutcome.Retry
        val newDocumentHash =
            resolveDocumentHash(lib, newPath) ?: return PathTransitionOutcome.Retry
        if (!graph.isOpen(oldDocumentHash)) {
            return if (graph.isOpen(newDocumentHash)) {
                PathTransitionOutcome.Projected
            } else {
                // A rename event is project-wide and also fires for closed
                // files. Durable identity still moves through the controller,
                // but the plugin must not manufacture liveness or a CRDT
                // member for a document it does not have open.
                PathTransitionOutcome.NoLiveEditor
            }
        }
        val fallbackRoot = project.basePath ?: return PathTransitionOutcome.Retry
        val root =
            projectRoots.remove(oldDocumentHash)
                ?: NativePatching.resolveProjectPath(newPath)?.first
                ?: fallbackRoot
        projectRoots[newDocumentHash] = root
        val transitionKey = "$oldDocumentHash\u0000$newDocumentHash"
        val opsJson =
            pathTransitionFrames.retain(transitionKey) {
                graph.move(
                    oldDocumentHash,
                    newDocumentHash,
                    newPath,
                    EditorIdentity.id,
                    "jetbrains",
                    pluginVersion(),
                    EDITOR_CAPABILITIES,
                ).orEmpty()
            }
        if (opsJson.isEmpty()) return PathTransitionOutcome.Projected
        return if (push(lib, root, newDocumentHash, opsJson)) {
            pathTransitionFrames.acknowledge(transitionKey, opsJson)
            PathTransitionOutcome.Projected
        } else {
            // The graph has already advanced to the new identity. Retain the
            // exact frame (including its original OR-set tags) so an enqueue
            // failure cannot lose the compensating old-path Close on retry.
            PathTransitionOutcome.Retry
        }
    }

    private fun push(
        lib: AgentDocLib,
        projectRoot: String,
        documentHash: String,
        opsJson: String,
    ): Boolean {
        if (lib.agent_doc_reliable_sync_liveness_enqueue(projectRoot, documentHash, opsJson) != 0) {
            return false
        }
        return lib.agent_doc_reliable_sync_liveness_flush(projectRoot, documentHash) >= 0
    }

    private fun resolveDocumentHash(lib: AgentDocLib, filePath: String): String? {
        val ptr = lib.agent_doc_document_id_for_path(filePath) ?: return null
        return try {
            ptr.getString(0).takeUnless { it.isNullOrEmpty() }
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    companion object {
        private val instances = ConcurrentHashMap<Project, ReliableSyncLivenessListener>()

        fun reportDocumentPathTransition(
            project: Project,
            oldPath: String,
            newPath: String,
        ): PathTransitionOutcome =
            instances[project]?.reportMoveNow(oldPath, newPath) ?: PathTransitionOutcome.Retry

        fun disposeProject(project: Project) {
            instances.remove(project)
        }

        private fun sha256Text(text: String): String =
            MessageDigest.getInstance("SHA-256")
                .digest(text.toByteArray(Charsets.UTF_8))
                .joinToString("") { "%02x".format(it.toInt() and 0xff) }
    }
}
