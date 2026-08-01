package com.github.btakita.agentdoc

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import com.intellij.openapi.vfs.newvfs.events.VFileMoveEvent
import com.intellij.openapi.vfs.newvfs.events.VFilePropertyChangeEvent
import com.intellij.util.concurrency.AppExecutorUtil
import io.github.lazily.ThreadSafeContext
import io.github.lazily.ThreadSafeSourceMap
import java.nio.file.Path
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

internal enum class DocumentPathTransitionPhase {
    Observed,
    LivenessProjected,
    ControllerAcknowledged,
    ReplicaRebound,
    Converged,
}

internal data class DocumentPathTransitionProjection(
    val transitionId: String,
    val oldPath: String,
    val newPath: String,
    val generation: Long,
    val sequence: Long,
    val phase: DocumentPathTransitionPhase = DocumentPathTransitionPhase.Observed,
    val attempt: Int = 0,
    val lastError: String? = null,
    val requiresReplicaRebind: Boolean = true,
)

internal fun documentPathTransitionRetryDelayMs(attempt: Int): Long {
    val exponent = attempt.coerceIn(0, 7)
    return (250L shl exponent).coerceAtMost(30_000L)
}

/**
 * Reactively converges a VFS path transition without invoking layout sync.
 *
 * The retained SourceMap is truth; controller requests are effect attempts and
 * their convergence receipts advance the projection. The liveness add for the
 * new identity precedes the old close, the controller rekeys durable/live
 * identity next, and only then does the editor atomically register/swap its
 * path-bound replica.
 */
class FileRenameListener(private val project: Project) : BulkFileListener {
    private val context = ThreadSafeContext()
    private val transitions = ThreadSafeSourceMap<String, DocumentPathTransitionProjection>()
    private val scheduled = ConcurrentHashMap<String, ScheduledFuture<*>>()
    private val generation = System.nanoTime().coerceAtLeast(1L)
    private val sequence = AtomicLong(0)

    override fun after(events: List<VFileEvent>) {
        for (event in events) {
            val paths = transitionPaths(event) ?: continue
            if (!shouldHandleFile(Path.of(paths.second).fileName?.toString())) continue
            observe(paths.first, paths.second)
        }
    }

    private fun observe(oldPath: String, newPath: String) {
        val normalizedOld = Path.of(oldPath).toAbsolutePath().normalize().toString()
        val normalizedNew = Path.of(newPath).toAbsolutePath().normalize().toString()
        if (normalizedOld == normalizedNew) return
        val transitionId = pathTransitionId(normalizedOld, normalizedNew)
        val projection =
            DocumentPathTransitionProjection(
                transitionId = transitionId,
                oldPath = normalizedOld,
                newPath = normalizedNew,
                generation = generation,
                sequence = sequence.incrementAndGet(),
            )
        transitions.set(context, transitionId, projection)
        LOG.info("[rename] observed retained path transition $normalizedOld → $normalizedNew")
        schedule(transitionId, 0)
    }

    private fun schedule(transitionId: String, delayMs: Long) {
        if (project.isDisposed) return
        val task =
            AppExecutorUtil.getAppScheduledExecutorService().schedule(
                {
                    scheduled.remove(transitionId)
                    reconcile(transitionId)
                },
                delayMs,
                TimeUnit.MILLISECONDS,
            )
        scheduled.put(transitionId, task)?.cancel(false)
    }

    private fun reconcile(transitionId: String) {
        if (project.isDisposed) return
        var projection = transitions.get(context, transitionId) ?: return
        try {
            if (projection.phase == DocumentPathTransitionPhase.Observed) {
                when (
                    ReliableSyncLivenessListener.reportDocumentPathTransition(
                        project,
                        projection.oldPath,
                        projection.newPath,
                    )
                ) {
                    ReliableSyncLivenessListener.PathTransitionOutcome.Projected -> {
                        projection =
                            projection.copy(
                                phase = DocumentPathTransitionPhase.LivenessProjected,
                                lastError = null,
                            )
                        transitions.set(context, transitionId, projection)
                    }
                    ReliableSyncLivenessListener.PathTransitionOutcome.NotSessionDocument -> {
                        transitions.set(
                            context,
                            transitionId,
                            projection.copy(phase = DocumentPathTransitionPhase.Converged),
                        )
                        return
                    }
                    ReliableSyncLivenessListener.PathTransitionOutcome.NoLiveEditor -> {
                        projection =
                            projection.copy(
                                phase = DocumentPathTransitionPhase.LivenessProjected,
                                lastError = null,
                                requiresReplicaRebind = false,
                            )
                        transitions.set(context, transitionId, projection)
                    }
                    ReliableSyncLivenessListener.PathTransitionOutcome.Retry -> {
                        retry(projection, "reliable liveness transition not yet acknowledged")
                        return
                    }
                }
            }

            if (projection.phase == DocumentPathTransitionPhase.LivenessProjected) {
                val projectRoot =
                    NativePatching.resolveProjectPath(projection.newPath)?.first
                        ?: project.basePath
                        ?: run {
                            retry(projection, "project root unavailable")
                            return
                        }
                val receipt =
                    CpRouteClient.observeDocumentPathTransition(
                        projectRoot = projectRoot,
                        transitionId = transitionId,
                        oldPath = projection.oldPath,
                        newPath = projection.newPath,
                    )
                if (!receipt.converged) {
                    retry(
                        projection,
                        receipt.error ?: "controller transition phase=${receipt.phase}",
                    )
                    return
                }
                projection =
                    projection.copy(
                        phase = DocumentPathTransitionPhase.ControllerAcknowledged,
                        lastError = null,
                    )
                transitions.set(context, transitionId, projection)
            }

            if (projection.phase == DocumentPathTransitionPhase.ControllerAcknowledged) {
                if (
                    projection.requiresReplicaRebind &&
                    !CrdtReplicaManager.rebindOpenDocumentPath(
                        project,
                        projection.oldPath,
                        projection.newPath,
                    )
                ) {
                    retry(projection, "new-path editor replica not yet registered")
                    return
                }
                projection =
                    projection.copy(
                        phase = DocumentPathTransitionPhase.ReplicaRebound,
                        lastError = null,
                    )
                transitions.set(context, transitionId, projection)
            }

            transitions.set(
                context,
                transitionId,
                projection.copy(
                    phase = DocumentPathTransitionPhase.Converged,
                    lastError = null,
                ),
            )
            LOG.info(
                "[rename] path transition converged without layout mutation " +
                    "${projection.oldPath} → ${projection.newPath}",
            )
        } catch (error: Exception) {
            retry(projection, error.message ?: error.javaClass.simpleName)
        }
    }

    private fun retry(projection: DocumentPathTransitionProjection, error: String) {
        val pending =
            projection.copy(
                attempt = projection.attempt + 1,
                lastError = error,
            )
        transitions.set(context, projection.transitionId, pending)
        val delayMs = documentPathTransitionRetryDelayMs(pending.attempt)
        LOG.warn(
            "[rename] retained path transition retry attempt=${pending.attempt} " +
                "delay_ms=$delayMs old=${pending.oldPath} new=${pending.newPath} reason=$error",
        )
        schedule(pending.transitionId, delayMs)
    }

    companion object {
        private val LOG = Logger.getInstance(FileRenameListener::class.java)

        fun shouldHandleFile(fileName: String?): Boolean =
            fileName != null && fileName.endsWith(".md")

        internal fun oldPathForRename(parentPath: String, oldName: String): String =
            Path.of(parentPath, oldName).toString()

        internal fun oldPathForMove(oldParentPath: String, fileName: String): String =
            Path.of(oldParentPath, fileName).toString()

        internal fun pathTransitionId(oldPath: String, newPath: String): String {
            val bytes = "$oldPath\u0000$newPath".toByteArray(Charsets.UTF_8)
            return "path-" +
                MessageDigest.getInstance("SHA-256")
                    .digest(bytes)
                    .take(16)
                    .joinToString("") { "%02x".format(it.toInt() and 0xff) }
        }

        private fun transitionPaths(event: VFileEvent): Pair<String, String>? =
            when (event) {
                is VFileMoveEvent ->
                    oldPathForMove(event.oldParent.path, event.file.name) to event.file.path
                is VFilePropertyChangeEvent -> {
                    if (event.propertyName != VirtualFile.PROP_NAME) {
                        null
                    } else {
                        val parent = event.file.parent ?: return null
                        oldPathForRename(parent.path, event.oldValue.toString()) to event.file.path
                    }
                }
                else -> null
            }
    }
}
