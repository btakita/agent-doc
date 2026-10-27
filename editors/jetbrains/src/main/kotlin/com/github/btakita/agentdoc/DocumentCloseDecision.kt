package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/**
 * What a `fileClosed` event means for a document's CRDT replica.
 *
 * GH #241: `FileEditorManagerListener.fileClosed` fires when ONE editor for a file is
 * removed, not only when the file stops being open. On a Remote Dev backend the guest
 * opened a second editor for an already-open document, one of the two was removed, and
 * the platform delivered `fileClosed` while the guest's
 * [ClientFileEditorManager.getAllFiles] still listed the file. The close handler
 * published the closing cut, deregistered the document's only replica and reported the
 * per-editor close, so the open buffer stopped receiving controller deliveries for
 * 4 h 48 min.
 *
 * A removed editor is a close only when no editor for the file remains open in ANY
 * client: the backend-local manager, the manager that fired the event, and every
 * Remote Dev client manager.
 */
internal enum class DocumentCloseVerdict {
    /** No client still has the file open: publish the closing cut and deregister. */
    RELEASE,

    /** Another editor (local or a Remote Dev client) still has the file open. */
    KEEP_STILL_OPEN,
}

/** One observation of the open-editor surface taken at a `fileClosed` event. */
internal data class DocumentCloseObservation(
    /** `FileEditorManager.isFileOpen` on the backend-local manager. */
    val backendOpen: Boolean,
    /** `isFileOpen` on the manager that delivered the `fileClosed` event. */
    val sourceOpen: Boolean,
    /** Each Remote Dev client manager's `getAllFiles()` paths. */
    val clientOpenPaths: List<Collection<String>>,
)

internal object DocumentCloseDecision {
    /**
     * Bound for the single deferred re-observation after a [DocumentCloseVerdict.KEEP_STILL_OPEN]
     * verdict. The platform exposes no "last editor for this file closed" event, and
     * the managers update their open-file sets on their own schedule, so a close that
     * first observes the file as still open is re-observed once after this bound. A
     * later genuine close of the remaining editor fires its own `fileClosed` too; the
     * re-observation only covers a client manager that had not yet dropped the file.
     */
    const val STILL_OPEN_RECHECK_MS = 1_500L

    /**
     * Pure close decision. `EditorTracker.activeEditors` is deliberately NOT an input:
     * GH #175 found it retains hidden Editor/Preview tabs, and at a `fileClosed` event
     * it can still hold the editor being removed, which would keep a genuinely closed
     * document's replica forever.
     */
    fun verdict(filePath: String, observation: DocumentCloseObservation): DocumentCloseVerdict =
        if (observation.backendOpen ||
            observation.sourceOpen ||
            observation.clientOpenPaths.any { paths -> filePath in paths }
        ) {
            DocumentCloseVerdict.KEEP_STILL_OPEN
        } else {
            DocumentCloseVerdict.RELEASE
        }

    /** Observe every client's open-file set for [file]. Called on the EDT. */
    fun observe(
        project: Project,
        source: FileEditorManager?,
        file: VirtualFile,
    ): DocumentCloseObservation {
        if (project.isDisposed) {
            return DocumentCloseObservation(false, false, emptyList())
        }
        val backendOpen = try {
            FileEditorManager.getInstance(project).isFileOpen(file)
        } catch (_: Throwable) {
            false
        }
        val sourceOpen = try {
            source?.isFileOpen(file) == true
        } catch (_: Throwable) {
            false
        }
        val clientOpenPaths = try {
            MarketplacePlatformBridge.remoteFileEditorManagers(project)
                .map { manager ->
                    try {
                        MarketplacePlatformBridge.allFiles(manager).map { it.path }
                    } catch (_: Throwable) {
                        emptyList()
                    }
                }
        } catch (_: Throwable) {
            emptyList()
        }
        return DocumentCloseObservation(backendOpen, sourceOpen, clientOpenPaths)
    }

    private val log =
        com.intellij.openapi.diagnostic.Logger.getInstance(DocumentCloseDecision::class.java)

    /** One pending bounded re-observation per (consumer, file). */
    private val pendingRechecks by lazy {
        ConcurrentHashMap<String, ScheduledFuture<*>>().also { pending ->
            // A dynamic unload must not leave a scheduled re-observation holding this
            // plugin generation's classes or releasing through a retired generation.
            PluginGeneration.registerResource("document-close-rechecks") {
                pending.values.forEach { it.cancel(false) }
                pending.clear()
            }
        }
    }

    /**
     * Route one `fileClosed` event for [consumer] (a stable name per listener). Runs
     * [onRelease] when no client still has the file open -- immediately, or after the
     * single bounded re-observation -- and [onKeep] when the event removed an editor of
     * a file that is still open. Called on the EDT that delivered `fileClosed`.
     */
    fun route(
        consumer: String,
        project: Project,
        source: FileEditorManager?,
        file: VirtualFile,
        onKeep: () -> Unit = {},
        onRelease: () -> Unit,
    ): DocumentCloseVerdict {
        val key = "$consumer|${file.path}"
        val verdict = verdict(file.path, observe(project, source, file))
        when (verdict) {
            DocumentCloseVerdict.RELEASE -> {
                pendingRechecks.remove(key)?.cancel(false)
                onRelease()
            }
            DocumentCloseVerdict.KEEP_STILL_OPEN -> {
                log.info(
                    "[crdt-replica] editor removed for still-open ${file.path}; " +
                        "consumer=$consumer decision=keep_still_open (GH #241)",
                )
                onKeep()
                val scheduled = AtomicReference<ScheduledFuture<*>?>()
                val future = GenerationWork.schedule(
                    {
                        GenerationWork.invokeLater {
                            // Only the latest re-observation for this key may release.
                            val current = scheduled.get() ?: return@invokeLater
                            if (!pendingRechecks.remove(key, current)) return@invokeLater
                            if (project.isDisposed) return@invokeLater
                            if (verdict(file.path, observe(project, null, file)) ==
                                DocumentCloseVerdict.RELEASE
                            ) {
                                log.info(
                                    "[crdt-replica] deferred close re-observation found ${file.path} " +
                                        "closed in every client; consumer=$consumer decision=release",
                                )
                                onRelease()
                            }
                        }
                    },
                    STILL_OPEN_RECHECK_MS,
                    TimeUnit.MILLISECONDS,
                )
                scheduled.set(future)
                pendingRechecks.put(key, future)?.cancel(false)
            }
        }
        return verdict
    }
}
