package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.vfs.VirtualFile
import io.github.lazily.ThreadSafeComputed
import io.github.lazily.ThreadSafeContext
import io.github.lazily.ThreadSafeSource
import java.lang.reflect.Field
import java.lang.reflect.Method
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

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
 * True when replacing [documentText] with [diskText] would drop an unmarked
 * operator list prompt, including the shape where disk has only the seen
 * (`👀`) form of that same prompt. A null disk snapshot is treated as a drop
 * when the editor holds such a prompt. Non-session files, generated `**field**:`
 * rows, and `▫️` structure rows are left alone, as is an operator delete that
 * exists only on disk.
 */
internal fun reloadWouldDropUnmarkedOperatorPrompt(documentText: String, diskText: String?): Boolean {
    if (!isAgentDocDocumentTextUtil(documentText)) return false
    val memory = unmarkedOperatorPromptBodies(documentText)
    if (memory.isEmpty()) return false
    if (diskText == null) return true
    val disk = unmarkedOperatorPromptBodies(diskText)
    return memory.any { prompt -> prompt !in disk }
}

private val operatorListItem = Regex("""^[ \t]*-[ \t]+(.*\S)[ \t]*$""")
private val generatedListField = Regex("""^\*\*[^*]+\*\*\s*:""")

/** Queue lifecycle glyphs painted onto a row. An unmarked prompt has none of these. */
private val operatorPromptLifecycleGlyphs = listOf(
    "👀",
    "📥",
    "🟡",
    "🚧",
    "⛔",
    "☑️",
    "📫",
    "📤",
    "⚠️",
    "🔄",
    "❔",
    "⏳",
    "📌",
    "📍",
    "🗑️",
    "🔒",
    "🙋",
)

private fun unmarkedOperatorPromptBodies(text: String): Set<String> {
    val prompts = linkedSetOf<String>()
    val normalized = text.replace("\r\n", "\n").replace('\r', '\n')
    for (line in normalized.lineSequence()) {
        val body = operatorListItem.matchEntire(line)?.groupValues?.get(1)?.trim().orEmpty()
        if (body.isEmpty() || body.startsWith("▫️")) continue
        if (generatedListField.containsMatchIn(body)) continue
        if (operatorPromptLifecycleGlyphs.any(body::startsWith)) continue
        prompts += body
    }
    return prompts
}

/**
 * Idle save refreshes the virtual file inside `doSaveDocument` before a
 * [com.intellij.openapi.fileEditor.FileDocumentSynchronizationVetoer] can run,
 * and that refresh queues the conflict dialog. Keep editor memory for the
 * rest of this EDT save when an unsaved document is an agent-doc session.
 * The override is disposed on the next EDT turn, so it does not cover a
 * later watcher refresh and is not a process-lifetime hold.
 *
 * Platforms without `overrideConflictResolution` no-op; the reload veto
 * still refuses a drop.
 */
internal object AgentDocSaveBatchConflictHold {
    private val LOG = Logger.getInstance(AgentDocSaveBatchConflictHold::class.java)
    private val depth = AtomicInteger(0)
    private val override = AtomicReference<Disposable?>(null)
    private val reflectionWarned = AtomicBoolean(false)

    fun holdIfUnsavedAgentDoc(texts: List<CharSequence>) {
        if (texts.none(::isAgentDocDocumentTextUtil)) return
        if (depth.getAndIncrement() == 0) {
            val created = installKeepMemory()
            if (created == null) {
                depth.decrementAndGet()
                return
            }
            override.set(created)
        }
        GenerationWork.invokeLater {
            if (depth.decrementAndGet() == 0) {
                override.getAndSet(null)?.let(Disposer::dispose)
            }
        }
    }

    private fun installKeepMemory(): Disposable? {
        val parent = Disposer.newDisposable("agent-doc-save-batch-keep-memory")
        return try {
            val manager = FileDocumentManager.getInstance()
            val method = manager.javaClass.methods.firstOrNull { candidate ->
                candidate.name == "overrideConflictResolution" && candidate.parameterCount == 2
            } ?: return disposeUnused(parent)
            val keep = method.parameterTypes[0].enumConstants?.firstOrNull { constant ->
                (constant as Enum<*>).name == "KEEP_MEMORY_CHANGES"
            } ?: return disposeUnused(parent)
            method.invoke(manager, keep, parent)
            parent
        } catch (error: Exception) {
            Disposer.dispose(parent)
            if (reflectionWarned.compareAndSet(false, true)) {
                LOG.warn("unable to keep editor memory across an agent-doc save batch", error)
            }
            null
        }
    }

    private fun disposeUnused(parent: Disposable): Disposable? {
        Disposer.dispose(parent)
        return null
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
