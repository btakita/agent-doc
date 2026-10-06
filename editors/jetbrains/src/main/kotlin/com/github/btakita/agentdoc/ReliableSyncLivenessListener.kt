package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
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
        // Project listeners can be created after the IDE restored its editor tabs;
        // seed that existing open set because no new fileOpened event is guaranteed.
        ApplicationManager.getApplication().invokeLater {
            if (!project.isDisposed) {
                EditorOpenFileSurface.snapshot(project).forEach(::reportOpen)
            }
        }
    }

    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
        reportOpen(file)
    }

    private fun reportOpen(file: VirtualFile) {
        reportOpenWithRetry(file.path, attempt = 0)
    }

    /**
     * `#rundocdispatchrobust`: an open report that cannot reach the owning controller is
     * retried, not dropped. After the 2026-09-29 18:12:38 dynamic reload the replacement
     * generation's report for every src/haiven-dev document was lost (the native library
     * was mid hot-reload), so that nested controller kept the retired generation as the
     * live endpoint and refused every registration as `replica_register_stale_editor_endpoint`.
     */
    private fun reportOpenWithRetry(filePath: String, attempt: Int) {
        val fallbackRoot = project.basePath ?: return
        ApplicationManager.getApplication().executeOnPooledThread {
            if (project.isDisposed || PluginGeneration.retired) return@executeOnPooledThread
            val lib = AgentDocLib.get()
            val outcome =
                if (lib == null) LivenessReportOutcome.Retry
                else reportOpenNow(lib, fallbackRoot, filePath, republish = attempt > 0)
            val delayMs = livenessReportRetryDelayMsUtil(outcome, attempt) ?: return@executeOnPooledThread
            com.intellij.util.concurrency.AppExecutorUtil.getAppScheduledExecutorService().schedule(
                { reportOpenWithRetry(filePath, attempt + 1) },
                delayMs,
                java.util.concurrent.TimeUnit.MILLISECONDS,
            )
        }
    }

    private fun reportOpenNow(
        lib: AgentDocLib,
        fallbackRoot: String,
        file: VirtualFile,
        republish: Boolean,
    ): Boolean = reportOpenNow(lib, fallbackRoot, file.path, republish) == LivenessReportOutcome.Published

    private fun reportOpenNow(
        lib: AgentDocLib,
        fallbackRoot: String,
        filePath: String,
        republish: Boolean,
    ): LivenessReportOutcome {
        // Scope liveness to agent-doc session documents only: a plain source file
        // opened as a tab must not enter the plane (it would over-count the
        // session-document scope). This disk read is appropriate at open time —
        // it is the moment we decide whether to track a possibly-random `.md` tab.
        if (lib.agent_doc_is_session_document(filePath) != 1) return LivenessReportOutcome.NotSessionDocument
        val documentHash = resolveDocumentHash(lib, filePath) ?: return LivenessReportOutcome.Retry
        // The owning controller is the nearest agent-doc root, not necessarily
        // the IntelliJ project base. A nested submodule has its own controller.
        val root = NativePatching.resolveProjectPath(filePath)?.first ?: fallbackRoot
        projectRoots[documentHash] = root
        val opsJson =
            if (republish) {
                graph.republishOpen(
                    documentHash,
                    filePath,
                    EditorIdentity.id,
                    "jetbrains",
                    pluginVersion(),
                    EDITOR_CAPABILITIES,
                )
            } else {
                graph.open(
                    documentHash,
                    filePath,
                    EditorIdentity.id,
                    "jetbrains",
                    pluginVersion(),
                    EDITOR_CAPABILITIES,
                ) ?: return LivenessReportOutcome.Published
            }
        return if (push(lib, root, documentHash, opsJson)) {
            LivenessReportOutcome.Published
        } else {
            LivenessReportOutcome.Retry
        }
    }

    private fun republishOpenDocumentsAfterNativeReload(files: Collection<VirtualFile>): Int {
        if (project.isDisposed) return 0
        val fallbackRoot = project.basePath ?: return 0
        val lib = AgentDocLib.get() ?: return 0
        return files.count { file ->
            reportOpenNow(lib, fallbackRoot, file, republish = true)
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

        /**
         * Install this plugin generation's listener under its project lifecycle.
         *
         * IntelliJ does not replay XML-declared project-listener construction for
         * projects that survive a dynamic plugin upgrade. Explicit installation
         * makes the replacement generation republish every open document with its
         * current editor version and replaces the stale PID-scoped save endpoint.
         */
        fun install(project: Project, lifecycle: Disposable): ReliableSyncLivenessListener =
            instances.computeIfAbsent(project) {
                ReliableSyncLivenessListener(project).also { listener ->
                    project.messageBus
                        .connect(lifecycle)
                        .subscribe(
                            FileEditorManagerListener.FILE_EDITOR_MANAGER,
                            listener,
                        )
                }
            }

        fun reportDocumentPathTransition(
            project: Project,
            oldPath: String,
            newPath: String,
        ): PathTransitionOutcome =
            instances[project]?.reportMoveNow(oldPath, newPath) ?: PathTransitionOutcome.Retry

        /**
         * `#rundocdispatchrobust`: republish this generation's liveness for one document. Called
         * when a registration is refused as a stale editor endpoint, which means the owning
         * controller never received this generation's open report.
         */
        fun republishDocument(project: Project, filePath: String) {
            instances[project]?.reportOpenWithRetry(filePath, attempt = 1)
        }

        fun republishOpenDocumentsAfterNativeReload(projects: Iterable<Project>): Int {
            val liveProjects = projects.filterNot { it.isDisposed }.toList()
            val openFiles = linkedMapOf<Project, List<VirtualFile>>()
            val capture = {
                liveProjects.forEach { project ->
                    openFiles[project] = EditorOpenFileSurface.snapshot(project)
                }
            }
            if (javax.swing.SwingUtilities.isEventDispatchThread()) {
                capture()
            } else {
                ApplicationManager.getApplication().invokeAndWait(capture)
            }
            return liveProjects.sumOf { project ->
                instances[project]?.republishOpenDocumentsAfterNativeReload(openFiles[project].orEmpty()) ?: 0
            }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)
        }

        private fun sha256Text(text: String): String =
            MessageDigest.getInstance("SHA-256")
                .digest(text.toByteArray(Charsets.UTF_8))
                .joinToString("") { "%02x".format(it.toInt() and 0xff) }
    }
}

internal enum class LivenessReportOutcome {
    Published,
    NotSessionDocument,
    Retry,
}

internal const val LIVENESS_REPORT_MAX_ATTEMPTS = 8

/** Backoff for a liveness report that could not reach its controller; null means stop. */
internal fun livenessReportRetryDelayMsUtil(outcome: LivenessReportOutcome, attempt: Int): Long? =
    when {
        outcome != LivenessReportOutcome.Retry -> null
        attempt + 1 >= LIVENESS_REPORT_MAX_ATTEMPTS -> null
        else -> minOf(250L shl attempt.coerceAtMost(4), 4_000L)
    }
