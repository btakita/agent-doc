package com.github.btakita.agentdoc

import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.vfs.VirtualFile
import io.github.lazily.ThreadSafeComputed
import io.github.lazily.ThreadSafeContext
import io.github.lazily.ThreadSafeSource
import java.lang.reflect.Field
import java.lang.reflect.Method
import java.util.concurrent.atomic.AtomicBoolean

/** Shared IntelliJ memory↔disk conflict inspection for every editor mutation lane. */
internal object IntelliJFileCacheConflictGuard {
    private val reflectionWarned = AtomicBoolean(false)

    fun hasPending(
        targetFile: VirtualFile,
        warn: (String, Throwable) -> Unit,
    ): Boolean {
        val fdm = FileDocumentManager.getInstance()
        return try {
            val resolverField = findFieldInHierarchy(fdm.javaClass, "myConflictResolver")
                ?: return false
            resolverField.isAccessible = true
            val resolver = resolverField.get(fdm) ?: return false
            val hasConflict =
                findMethodInHierarchy(resolver.javaClass, "hasConflict", VirtualFile::class.java)
                    ?: return false
            hasConflict.isAccessible = true
            hasConflict.invoke(resolver, targetFile) as? Boolean ?: false
        } catch (error: Exception) {
            if (reflectionWarned.compareAndSet(false, true)) {
                warn(
                    "unable to inspect IntelliJ File Cache Conflict state; proceeding without conflict guard",
                    error,
                )
            }
            false
        }
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
