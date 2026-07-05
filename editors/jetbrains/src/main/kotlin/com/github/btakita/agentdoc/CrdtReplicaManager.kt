package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.CommandProcessor
import com.intellij.openapi.command.UndoConfirmationPolicy
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/**
 * Production editor-as-CRDT-replica wiring (`#crdtauth5`, realtime phase 3).
 *
 * The manager is intentionally thin: local edits are forwarded to [CrdtReplicaForwarder],
 * remote updates are pulled from the supervisor, and document mutation uses the same
 * minimal-edit helper as IPC patches. It never saves the document after a realtime
 * CRDT update.
 */
class CrdtReplicaManager(private val project: Project) : Disposable, DocumentListener {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(CrdtReplicaManager::class.java)
    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-crdt-replica-poller").apply { isDaemon = true }
    }
    private val forwarders = ConcurrentHashMap<String, CrdtReplicaForwarder>()
    private val shadows = ConcurrentHashMap<String, String>()
    private val applyingRemote = ConcurrentHashMap.newKeySet<String>()
    private val pendingLocalEdits = ConcurrentHashMap<String, AtomicInteger>()
    @Volatile private var pollTask: ScheduledFuture<*>? = null

    fun start() {
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(this, this)
        pollTask = executor.scheduleWithFixedDelay({
            try {
                pollRemoteUpdates()
            } catch (e: Exception) {
                log.debug("[crdt-replica] remote poll skipped: ${e.message}")
            }
        }, 250, 250, TimeUnit.MILLISECONDS)
    }

    override fun dispose() {
        pollTask?.cancel(false)
        pollTask = null
        forwarders.values.forEach { it.deregister() }
        forwarders.clear()
        shadows.clear()
        executor.shutdownNow()
    }

    override fun documentChanged(event: DocumentEvent) {
        val file = FileDocumentManager.getInstance().getFile(event.document) ?: return
        if (!file.name.endsWith(".md")) return
        val filePath = file.path
        if (applyingRemote.contains(filePath)) return
        val newFragment = event.newFragment.toString()
        val oldFragment = event.oldFragment.toString()
        if (newFragment.isEmpty() && oldFragment.isEmpty()) return
        if (!shadows.containsKey(filePath)) {
            seedAndAttachFromDocument(filePath, event.document)
            return
        }
        markLocalPending(filePath)
        executor.execute {
            try {
                forwardLocalDeltaFromShadow(
                    filePath,
                    event.document,
                    event.offset,
                    oldFragment,
                    newFragment,
                )
            } finally {
                clearLocalPending(filePath)
            }
        }
    }

    private fun seedAndAttachFromDocument(filePath: String, document: Document) {
        markLocalPending(filePath)
        executor.execute {
            try {
                val text = ApplicationManager.getApplication().runReadAction<String> { document.text }
                shadows[filePath] = text
                forwarderFor(filePath, text)
            } catch (e: Exception) {
                log.debug("[crdt-replica] seed skipped for $filePath: ${e.message}")
            } finally {
                clearLocalPending(filePath)
            }
        }
    }

    fun ensureOpenDocumentReplica(
        filePath: String,
        document: Document,
        editorText: String? = null,
        await: Boolean = false,
    ): Boolean {
        val attach = {
            try {
                val text = editorText ?: ApplicationManager.getApplication().runReadAction<String> { document.text }
                shadows[filePath] = text
                forwarderFor(filePath, text) != null
            } catch (e: Exception) {
                log.debug("[crdt-replica] open-document attach skipped for $filePath: ${e.message}")
                false
            }
        }
        if (await) {
            log.debug("[crdt-replica] open-document attach requested with await=true; scheduling asynchronously for $filePath")
        }
        executor.execute { attach() }
        return true
    }

    private fun forwardLocalDeltaFromShadow(
        filePath: String,
        document: Document,
        eventOffset: Int,
        oldFragment: String,
        newFragment: String,
    ) {
        val beforeText = shadows[filePath] ?: run {
            seedAndAttachFromDocument(filePath, document)
            return
        }
        val nextText = applyEventToShadow(beforeText, eventOffset, oldFragment, newFragment) ?: run {
            shadows.remove(filePath)
            seedAndAttachFromDocument(filePath, document)
            return
        }
        shadows[filePath] = nextText
        val offset = codePointOffset(beforeText, eventOffset)
        val deleteLen = oldFragment.codePointCount(0, oldFragment.length)
        val forwarder = forwarderFor(filePath, beforeText)
        forwarder?.forwardLocalDelta(offset, deleteLen, newFragment)
    }

    private fun pollRemoteUpdates() {
        for ((filePath, forwarder) in forwarders) {
            if (hasPendingLocal(filePath)) continue
            // D2: a replace delivery (out-of-band deletion re-bootstrap) installs the
            // corrected canonical wholesale; a normal delta batch applies per-update.
            val delivery = forwarder.pullRemoteDelivery()
            if (delivery is ReplicaPullDelivery.Replace) {
                applyReplaceDelivery(filePath, forwarder, delivery.text)
                continue
            }
            val updates = (delivery as ReplicaPullDelivery.Deltas).updates
            if (updates.isEmpty()) continue
            for (update in updates) {
                if (hasPendingLocal(filePath)) break
                if (!shouldApplyRemoteCrdtUpdateUtil(update, forwarder.clientId)) {
                    forwarder.ackRemoteUpdate(update)
                    continue
                }
                val expectedText = shadows[filePath] ?: continue
                val converged = forwarder.applyRemoteUpdate(update.update) ?: continue
                if (hasPendingLocal(filePath)) continue
                if (applyRemoteText(filePath, expectedText, converged)) {
                    forwarder.ackRemoteUpdate(update)
                }
            }
        }
    }

    /**
     * D2 — apply a REPLACE delivery: install the corrected canonical text into the
     * buffer wholesale (an out-of-band deletion the additive CRDT delta cannot
     * express), then re-bootstrap the local replica node so later deltas are
     * relative to the corrected state. Never clobbers unsaved operator edits
     * (fail-open to the buffer), and no-ops when the buffer already matches.
     */
    private fun applyReplaceDelivery(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
        canonical: String,
    ) {
        if (hasPendingLocal(filePath)) return
        var installed = false
        ApplicationManager.getApplication().invokeAndWait {
            val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeAndWait
            val document = FileDocumentManager.getInstance().getDocument(targetFile) ?: return@invokeAndWait
            val before = document.text
            if (before == canonical) {
                shadows[filePath] = canonical
                installed = true
                return@invokeAndWait
            }
            if (hasPendingLocal(filePath)) return@invokeAndWait
            applyingRemote.add(filePath)
            try {
                runUndoableRemoteUpdateCommand(document) {
                    applyMinimalDocumentEditUtil(document, before, canonical)
                }
                shadows[filePath] = canonical
                installed = true
                log.info("[crdt-replica] applied REPLACE re-bootstrap for $filePath (${canonical.length} chars)")
            } finally {
                applyingRemote.remove(filePath)
            }
        }
        if (installed) {
            // Re-bootstrap the native replica against the corrected canonical so the
            // next additive delta stream is relative to the post-deletion state.
            forwarder.deregister()
            forwarder.register()
        }
    }

    private fun applyRemoteText(filePath: String, expectedText: String, converged: String): Boolean {
        var applied = false
        ApplicationManager.getApplication().invokeAndWait {
            val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeAndWait
            val document = FileDocumentManager.getInstance().getDocument(targetFile) ?: return@invokeAndWait
            val before = document.text
            if (before == converged) {
                shadows[filePath] = converged
                applied = true
                return@invokeAndWait
            }
            if (hasPendingLocal(filePath)) return@invokeAndWait
            if (!remoteCrdtApplyStillCurrentUtil(expectedText, before, converged)) {
                log.warn("[crdt-replica] stale remote update rejected for $filePath; editor text advanced before apply")
                return@invokeAndWait
            }
            val normalized = NativePatching.normalizeTemplateStructure(converged) ?: run {
                log.warn("[crdt-replica] remote update rejected by template-structure guard for $filePath")
                return@invokeAndWait
            }
            if (normalized != converged) {
                log.warn("[crdt-replica] remote update requires template-structure repair for $filePath; rejecting to keep replica state coherent")
                return@invokeAndWait
            }
            applyingRemote.add(filePath)
            try {
                runUndoableRemoteUpdateCommand(document) {
                    applyMinimalDocumentEditUtil(document, before, converged)
                    shadows[filePath] = converged
                    applied = true
                }
            } finally {
                applyingRemote.remove(filePath)
            }
        }
        return applied
    }

    private fun runUndoableRemoteUpdateCommand(document: Document, body: () -> Unit) {
        CommandProcessor.getInstance().executeCommand(
            project,
            {
                ApplicationManager.getApplication().runWriteAction {
                    body()
                }
            },
            "Agent Doc CRDT Remote Update",
            null,
            UndoConfirmationPolicy.DEFAULT,
            document,
        )
    }

    private fun forwarderFor(filePath: String, initialEditorText: String? = null): CrdtReplicaForwarder? {
        forwarders[filePath]?.let {
            if (initialEditorText != null) {
                it.ensureEditorText(initialEditorText)
            }
            return it
        }
        val root = resolveProjectRoot(filePath) ?: return null
        val identity = "${EditorIdentity.id}:$filePath"
        val forwarder = CrdtReplicaForwarder(
            filePath = filePath,
            identity = identity,
            node = NativeReplicaNode(),
            transport = SupervisorSocketReplicaTransport(root),
        )
        if (!forwarder.register()) {
            return null
        }
        val existing = forwarders.putIfAbsent(filePath, forwarder)
        if (existing != null) {
            forwarder.deregister()
            return existing
        }
        if (initialEditorText != null) {
            forwarder.ensureEditorText(initialEditorText)
        }
        log.info("[crdt-replica] attached ${File(filePath).name} as $identity")
        return forwarder
    }

    private fun markLocalPending(filePath: String) {
        pendingLocalEdits.computeIfAbsent(filePath) { AtomicInteger(0) }.incrementAndGet()
    }

    private fun clearLocalPending(filePath: String) {
        val counter = pendingLocalEdits[filePath] ?: return
        if (counter.decrementAndGet() <= 0) {
            pendingLocalEdits.remove(filePath, counter)
        }
    }

    private fun hasPendingLocal(filePath: String): Boolean =
        (pendingLocalEdits[filePath]?.get() ?: 0) > 0

    private fun resolveProjectRoot(filePath: String): String? {
        var dir: File? = File(filePath).absoluteFile.parentFile
        while (dir != null) {
            if (File(dir, ".agent-doc").isDirectory) return dir.absolutePath
            dir = dir.parentFile
        }
        return project.basePath?.takeIf { File(it, ".agent-doc").isDirectory }
    }

    private fun codePointOffset(text: String, utf16Offset: Int): Int {
        val bounded = utf16Offset.coerceIn(0, text.length)
        return text.codePointCount(0, bounded)
    }

    private fun applyEventToShadow(
        oldText: String,
        offset: Int,
        oldFragment: String,
        newFragment: String,
    ): String? {
        val bounded = offset.coerceIn(0, oldText.length)
        val oldEnd = bounded + oldFragment.length
        if (oldEnd > oldText.length) return null
        if (oldFragment.isNotEmpty() && oldText.substring(bounded, oldEnd) != oldFragment) {
            return null
        }
        return oldText.substring(0, bounded) +
            newFragment +
            oldText.substring(oldEnd)
    }

    companion object {
        private val instances = ConcurrentHashMap<Project, CrdtReplicaManager>()

        fun getInstance(project: Project): CrdtReplicaManager =
            instances.getOrPut(project) {
                CrdtReplicaManager(project).also { it.start() }
            }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }

        fun ensureReplicaForOpenDocument(
            filePath: String,
            document: Document,
            editorText: String? = null,
            await: Boolean = false,
        ): Boolean {
            val manager = instances.values.firstOrNull { it.ownsFilePath(filePath) }
                ?: instances.values.firstOrNull()
                ?: return false
            return manager.ensureOpenDocumentReplica(filePath, document, editorText, await)
        }

        fun isApplyingRemote(filePath: String): Boolean =
            instances.values.any { it.applyingRemote.contains(filePath) }
    }

    private fun ownsFilePath(filePath: String): Boolean {
        val base = project.basePath ?: return false
        return try {
            File(filePath).absoluteFile.toPath().startsWith(File(base).absoluteFile.toPath())
        } catch (_: Exception) {
            false
        }
    }
}

internal fun shouldApplyRemoteCrdtUpdateUtil(update: ReplicaRemoteUpdate, clientId: Long): Boolean =
    update.origin != clientId

internal fun remoteCrdtApplyStillCurrentUtil(
    expectedText: String,
    currentText: String,
    targetText: String,
): Boolean =
    currentText == expectedText || currentText == targetText
