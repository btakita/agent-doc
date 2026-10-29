package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.vfs.VirtualFile
import io.github.lazily.ThreadSafeComputed
import io.github.lazily.ThreadSafeContext
import io.github.lazily.ThreadSafeSource
import java.lang.reflect.Field
import java.lang.reflect.Method
import java.util.concurrent.atomic.AtomicBoolean

internal data class FileCacheConflictProbe(
    val pending: Boolean,
    val method: String?,
)

/**
 * Shared IntelliJ memory↔disk conflict inspection for every editor mutation lane.
 *
 * Current platforms queue the conflict on `MemoryDiskConflictResolver.isInConflictQueue`
 * and log `conflict queued for <file>` without the phrase `File Cache Conflict`.
 * Older platforms expose the same queue as `hasConflict`. Either method is pending.
 */
internal object IntelliJFileCacheConflictGuard {
    internal val conflictProbeMethodNames: List<String> = listOf("isInConflictQueue", "hasConflict")

    private val reflectionWarned = AtomicBoolean(false)

    fun hasPending(
        targetFile: VirtualFile,
        warn: (String, Throwable) -> Unit,
    ): Boolean = probe(targetFile, warn).pending

    fun probe(
        targetFile: VirtualFile,
        warn: (String, Throwable) -> Unit,
    ): FileCacheConflictProbe {
        val fdm = FileDocumentManager.getInstance()
        return try {
            val resolverField = findFieldInHierarchy(fdm.javaClass, "myConflictResolver")
                ?: return FileCacheConflictProbe(false, null)
            resolverField.isAccessible = true
            val resolver = resolverField.get(fdm) ?: return FileCacheConflictProbe(false, null)
            val method = findConflictProbe(resolver.javaClass)
                ?: return FileCacheConflictProbe(false, null)
            method.isAccessible = true
            val pending = method.invoke(resolver, targetFile) as? Boolean ?: false
            FileCacheConflictProbe(pending, method.name)
        } catch (error: Exception) {
            if (reflectionWarned.compareAndSet(false, true)) {
                warn(
                    "unable to inspect IntelliJ File Cache Conflict state; proceeding without conflict guard",
                    error,
                )
            }
            FileCacheConflictProbe(false, null)
        }
    }

    internal fun findConflictProbe(type: Class<*>): Method? {
        for (name in conflictProbeMethodNames) {
            findMethodInHierarchy(type, name, VirtualFile::class.java)?.let { return it }
        }
        return null
    }

    private fun findFieldInHierarchy(type: Class<*>, name: String): Field? {
        var current: Class<*>? = type
        while (current != null) {
            try {
                return current.getDeclaredField(name)
            } catch (_: NoSuchFieldException) {
                current = current.superclass
            }
        }
        return null
    }

    private fun findMethodInHierarchy(
        type: Class<*>,
        name: String,
        vararg parameterTypes: Class<*>,
    ): Method? {
        var current: Class<*>? = type
        while (current != null) {
            try {
                return current.getDeclaredMethod(name, *parameterTypes)
            } catch (_: NoSuchMethodException) {
                current = current.superclass
            }
        }
        return null
    }
}

internal data class FileCacheConflictDecision(
    val deferMutation: Boolean,
    val newlyPendingEdge: Boolean,
)

/**
 * Lazily-derived conflict gate. Stable pending observations stay blocked without
 * manufacturing polling effects; clearing the conflict re-arms the next edge.
 */
internal class FileCacheConflictPlane(private val context: ThreadSafeContext) {
    private data class Observation(
        val pending: Boolean,
        val diskWitness: Long,
    )

    private val observations: ThreadSafeSource<Map<String, Observation>> =
        context.source(emptyMap())
    private val blocked: ThreadSafeComputed<Set<String>> =
        context.computed {
            get(observations)
                .filterValues { observation -> observation.pending }
                .keys
        }

    @Synchronized
    fun observe(
        filePath: String,
        pending: Boolean,
        diskWitness: Long,
    ): FileCacheConflictDecision {
        val prior = context.get(observations)[filePath]
        val observation = Observation(pending, diskWitness)
        val updated = context.get(observations).toMutableMap().apply {
            put(filePath, observation)
        }
        context.set(observations, updated)
        val defer = filePath in context.get(blocked)
        return FileCacheConflictDecision(
            deferMutation = defer,
            newlyPendingEdge =
                defer && (prior?.pending != true || prior.diskWitness != diskWitness),
        )
    }

    @Synchronized
    fun remove(filePath: String) {
        val updated = context.get(observations).toMutableMap()
        if (updated.remove(filePath) != null) context.set(observations, updated)
    }

    fun clear() {
        context.set(observations, emptyMap())
    }
}

/**
 * `idea.log` correlation line for a conflict the resolver has queued.
 * The platform line does not contain `File Cache Conflict`, a dialog button,
 * or a production stack. This line does not invent the button.
 */
internal fun fileCacheConflictQueuedIdeaLogLine(
    filePath: String,
    probe: String,
    documentStamp: Long?,
    oldFileStamp: Long?,
): String {
    val document = documentStamp?.let { " documentStamp=$it" }.orEmpty()
    val oldFile = oldFileStamp?.let { " oldFileStamp=$it" }.orEmpty()
    return "[agent-doc] File Cache Conflict queued for $filePath; resolver=$probe$document$oldFile"
}
