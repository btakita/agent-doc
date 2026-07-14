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
import io.github.lazily.IngressOutcome
import io.github.lazily.MergePolicy
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

private const val CRDT_LISTENER_WARN_MS = 10L
private const val CRDT_WORKER_WARN_MS = 100L
private const val CRDT_EDT_WARN_MS = 50L
private const val CRDT_AWAIT_ATTACH_TIMEOUT_MS = 750L
private const val CRDT_REGISTER_FAILURE_BASE_BACKOFF_MS = 60_000L
private const val CRDT_REGISTER_FAILURE_MAX_BACKOFF_MS = 300_000L
private const val CRDT_DRAIN_NOOP_RESCHEDULE_BASE_BACKOFF_MS = 100L
// `#crdt-drain-idle-quiet`: the no-op drain-all loop must keep polling so purely-remote
// CRDT updates (a peer edits with no local event here) still get pulled — but an idle
// replica set does not need a 5s cadence. Cap the idle reschedule at 30s so a workspace
// full of parked session-doc replicas stops waking every 5s (observed steady-state
// churn across ~9 attached replicas). Active editing / authority-publish / open-document
// events still trigger an immediate drain, so only passive remote-only observation on an
// otherwise-idle doc sees up to 30s of extra latency.
private const val CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS = 30_000L

private data class PendingRemoteAck(
    val forwarder: CrdtReplicaForwarder,
    val update: ReplicaRemoteUpdate,
)

private data class PendingRemoteEditorApply(
    val filePath: String,
    val expectedText: String,
    val targetText: String,
    val acknowledgements: List<PendingRemoteAck>,
)

private data class RemoteEditorApplyOutcome(
    val applied: Boolean,
    val editorText: String?,
)

/**
 * Oldest baseline + newest converged text + acknowledgement union. This merge
 * is associative, so RelayCell can conflate any producer/drain schedule without
 * losing the durable acknowledgements covered by the final visible state.
 */
private val REMOTE_EDITOR_APPLY_MERGE = MergePolicy(
    name = "RemoteEditorApply",
    merge = { old: PendingRemoteEditorApply, latest: PendingRemoteEditorApply ->
        latest.copy(
            expectedText = old.expectedText,
            acknowledgements = (old.acknowledgements + latest.acknowledgements)
                .distinctBy { it.update.patchId },
        )
    },
    commutative = false,
    idempotent = true,
)

/**
 * Production editor-as-CRDT-replica wiring (`#crdtauth5`, realtime phase 3).
 *
 * The manager is intentionally thin: local edits are forwarded to [CrdtReplicaForwarder],
 * remote updates are pulled from the CPC document model, and document mutation uses the same
 * minimal-edit helper as IPC patches. It never saves the document after a realtime
 * CRDT update.
 */
class CrdtReplicaManager(private val project: Project) : Disposable, DocumentListener {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(CrdtReplicaManager::class.java)
    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-crdt-replica-events").apply { isDaemon = true }
    }
    private val forwarders = ConcurrentHashMap<String, CrdtReplicaForwarder>()
    private val shadows = ConcurrentHashMap<String, String>()
    private val applyingRemote = ConcurrentHashMap.newKeySet<String>()
    private val pendingLocalEdits = ConcurrentHashMap<String, AtomicInteger>()
    private val drainQueued = AtomicBoolean(false)
    private val drainAllRequested = AtomicBoolean(false)
    private val drainRequestedPaths = ConcurrentHashMap.newKeySet<String>()
    private val registerFailureCounts = ConcurrentHashMap<String, Int>()
    private val registerRetryAfterMs = ConcurrentHashMap<String, Long>()
    private val consecutiveNoOpReschedules = AtomicInteger(0)
    private val remoteDrainBackoffScheduled = AtomicBoolean(false)
    private val remoteEditorApplies =
        KeyedCoalescingRelay<String, PendingRemoteEditorApply>(REMOTE_EDITOR_APPLY_MERGE)
    private val remoteEditorApplyScheduled = AtomicBoolean(false)
    private val remoteEditorApplyPaths = ConcurrentHashMap.newKeySet<String>()
    private val disposed = AtomicBoolean(false)

    fun start() {
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(this, this)
    }

    override fun dispose() {
        disposed.set(true)
        remoteEditorApplies.clear()
        remoteEditorApplyPaths.clear()
        drainRequestedPaths.clear()
        registerFailureCounts.clear()
        registerRetryAfterMs.clear()
        forwarders.values.forEach { it.deregister() }
        forwarders.clear()
        shadows.clear()
        executor.shutdownNow()
    }

    override fun documentChanged(event: DocumentEvent) {
        val started = System.nanoTime()
        var loggedFilePath: String? = null
        try {
            val file = FileDocumentManager.getInstance().getFile(event.document) ?: return
            if (!file.name.endsWith(".md")) return
            val filePath = file.path
            loggedFilePath = filePath
            if (CrdtReplicaManager.isApplyingNonOperatorMutation(filePath)) return
            val newFragment = event.newFragment.toString()
            val oldFragment = event.oldFragment.toString()
            if (newFragment.isEmpty() && oldFragment.isEmpty()) return
            if (!shadows.containsKey(filePath)) {
                seedAndAttachFromDocument(filePath, event.document)
                return
            }
            markLocalPending(filePath)
            executor.execute {
                val workerStarted = System.nanoTime()
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
                    logSlow("local-delta-worker", filePath, workerStarted, details = "old_chars=${oldFragment.length} new_chars=${newFragment.length}")
                    requestRemoteDrain(filePath, "local-delta")
                }
            }
        } finally {
            loggedFilePath?.let { logSlow("documentChanged-listener", it, started, warnMs = CRDT_LISTENER_WARN_MS) }
        }
    }

    private fun seedAndAttachFromDocument(filePath: String, document: Document) {
        markLocalPending(filePath)
        executor.execute {
            val started = System.nanoTime()
            var chars = -1
            try {
                val text = ApplicationManager.getApplication().runReadAction<String> { document.text }
                chars = text.length
                shadows[filePath] = text
                forwarderFor(filePath, text, alignExisting = true)
                requestRemoteDrain(filePath, "seed")
            } catch (e: Exception) {
                log.debug("[crdt-replica] seed skipped for $filePath: ${e.message}")
            } finally {
                clearLocalPending(filePath)
                logSlow("seed-and-attach", filePath, started, details = "chars=$chars")
            }
        }
    }

    fun ensureOpenDocumentReplica(
        filePath: String,
        document: Document,
        editorText: String? = null,
        await: Boolean = false,
        forceRefresh: Boolean = false,
    ): Boolean {
        val attach = {
            val started = System.nanoTime()
            var chars = -1
            try {
                val text = editorText ?: ApplicationManager.getApplication().runReadAction<String> { document.text }
                chars = text.length
                shadows[filePath] = text
                val forwarder = forwarderFor(
                    filePath,
                    text,
                    alignExisting = forceRefresh,
                    bypassRegisterBackoff = forceRefresh,
                )
                (forwarder != null).also { attached ->
                    if (attached) requestRemoteDrain(filePath, "open-document")
                }
            } catch (e: Exception) {
                log.debug("[crdt-replica] open-document attach skipped for $filePath: ${e.message}")
                false
            } finally {
                logSlow("open-document-attach", filePath, started, details = "chars=$chars force_refresh=$forceRefresh")
            }
        }
        if (await) {
            return try {
                executor.submit<Boolean> { attach() }
                    .get(CRDT_AWAIT_ATTACH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
            } catch (e: TimeoutException) {
                log.warn("[crdt-replica] open-document attach timed out for $filePath after ${CRDT_AWAIT_ATTACH_TIMEOUT_MS}ms")
                false
            } catch (e: Exception) {
                log.debug("[crdt-replica] open-document attach failed for $filePath: ${e.message}")
                false
            }
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
        val started = System.nanoTime()
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
        logSlow(
            "forward-local-delta",
            filePath,
            started,
            details = "offset_utf16=$eventOffset offset_cp=$offset delete_cp=$deleteLen insert_chars=${newFragment.length}",
        )
    }

    fun requestRemoteDrain(filePath: String? = null, reason: String = "event") {
        if (filePath == null) {
            drainAllRequested.set(true)
        } else {
            drainRequestedPaths.add(filePath)
        }
        if (!drainQueued.compareAndSet(false, true)) return
        executor.execute {
            var appliedTotal = 0
            try {
                appliedTotal = drainRemoteUpdates(reason)
            } catch (e: Exception) {
                log.debug("[crdt-replica] remote drain skipped: ${e.message}")
            } finally {
                drainQueued.set(false)
                if (drainAllRequested.get() || drainRequestedPaths.isNotEmpty()) {
                    // #crdt-drain-backoff: when a drain cycle applied zero useful
                    // updates (notably when the CPC socket is unavailable and every
                    // pullDelivery returns empty deltas), delay the reschedule with
                    // exponential backoff instead of re-executing immediately. A
                    // tight no-op spin generated ~70MB/min of logs and froze the IDE.
                    if (appliedTotal == 0) {
                        val delayMs = nextNoOpRescheduleBackoffMs()
                        log.debug("[crdt-replica] no-op drain cycle; backing off reschedule by ${delayMs}ms (consecutive=${consecutiveNoOpReschedules.get()})")
                        scheduleRemoteDrainAfterBackoff(delayMs, reason)
                    } else {
                        consecutiveNoOpReschedules.set(0)
                        requestRemoteDrain(reason = "rescheduled")
                    }
                } else {
                    consecutiveNoOpReschedules.set(0)
                }
            }
        }
    }

    /**
     * Controller-proven reattach recovery. This is intentionally the only path
     * that sends a bounded text adopt: push the authoritative editor text, then
     * re-register so the native replica/frontier bootstraps from the rebuilt
     * canonical before another incremental op can be emitted.
     */
    fun requestTextAdopt(filePath: String) {
        executor.execute {
            val forwarder = forwarders[filePath] ?: return@execute
            val editorText = shadows[filePath] ?: return@execute
            if (!forwarder.pushTextAdopt(editorText)) return@execute
            if (forwarders.remove(filePath, forwarder)) {
                forwarder.deregister()
                val reattached = forwarderFor(
                    filePath,
                    editorText,
                    alignExisting = false,
                    bypassRegisterBackoff = true,
                )
                log.info(
                    "[reattach-adopt] bounded text adopted for ${File(filePath).name}; " +
                        "reattached=${reattached != null} chars=${editorText.length}",
                )
                requestRemoteDrain(filePath, "reattach-text-adopt")
            }
        }
    }

    private fun nextNoOpRescheduleBackoffMs(): Long {
        val n = consecutiveNoOpReschedules.incrementAndGet()
        val shifted = 1L shl minOf(n - 1, 12)
        return minOf(
            CRDT_DRAIN_NOOP_RESCHEDULE_BASE_BACKOFF_MS * shifted,
            CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS,
        )
    }

    private fun scheduleRemoteDrainAfterBackoff(delayMs: Long, reason: String) {
        if (!remoteDrainBackoffScheduled.compareAndSet(false, true)) return
        executor.schedule(
            {
                remoteDrainBackoffScheduled.set(false)
                if (!disposed.get()) requestRemoteDrain(reason = "$reason-backoff")
            },
            delayMs,
            TimeUnit.MILLISECONDS,
        )
    }

    private fun drainRemoteUpdates(reason: String): Int {
        val started = System.nanoTime()
        val drainAll = drainAllRequested.getAndSet(false)
        val paths = if (drainAll) {
            forwarders.keys().toList()
        } else {
            drainRequestedPaths.toList().also { drained ->
                drained.forEach { drainRequestedPaths.remove(it) }
            }
        }
        if (paths.isEmpty()) return 0
        log.debug("[crdt-replica] draining ${paths.size} replica(s) via $reason")
        var appliedTotal = 0
        for (filePath in paths) {
            val forwarder = forwarders[filePath] ?: continue
            appliedTotal += drainRemoteUpdatesFor(filePath, forwarder)
        }
        logSlow("remote-drain", paths.firstOrNull() ?: "(none)", started, details = "paths=${paths.size} reason=$reason drain_all=$drainAll applied_total=$appliedTotal")
        return appliedTotal
    }

    private fun drainRemoteUpdatesFor(filePath: String, forwarder: CrdtReplicaForwarder): Int {
        val started = System.nanoTime()
        var updateCount = 0
        var selfEchoCount = 0
        var peerUpdateCount = 0
        var ackCount = 0
        var queuedForEditor = false
        var deliveryKind = "deltas"
        var usefulWork = 0
        if (hasPendingLocal(filePath) || remoteEditorApplyPaths.contains(filePath)) return 0
        try {
            val expectedText = shadows[filePath] ?: return 0
            // D2: a replace delivery (out-of-band deletion re-bootstrap) installs
            // the corrected canonical only when the editor buffer still matches
            // the local replica baseline; normal deltas are merged into the native
            // replica first, then applied to the editor in one EDT command.
            val delivery = forwarder.pullRemoteDelivery()
            if (delivery is ReplicaPullDelivery.Replace) {
                deliveryKind = "replace"
            queuedForEditor = applyReplaceDelivery(filePath, forwarder, expectedText, delivery.text)
            usefulWork = if (queuedForEditor) 1 else 0
                return usefulWork
            }
            val updates = (delivery as ReplicaPullDelivery.Deltas).updates
            updateCount = updates.size
            usefulWork = updateCount
            if (updates.isEmpty()) return 0

            if (!editorReplicaBaselineMatches(filePath, forwarder, expectedText)) return usefulWork
            val appliedRemoteUpdates = mutableListOf<ReplicaRemoteUpdate>()
            var converged: String? = null
            for (update in updates) {
                if (hasPendingLocal(filePath)) break
                if (!shouldApplyRemoteCrdtUpdateUtil(update, forwarder.clientId)) {
                    selfEchoCount++
                    // Self-echo still needs visible-content proof: the operator's
                    // local delta may have reached canonical while the editor
                    // buffer moved again before this pull.
                    val visibleText = editorBufferText(filePath) ?: expectedText
                    if (forwarder.ackRemoteUpdate(update, visibleText)) ackCount++
                    continue
                }
                peerUpdateCount++
                converged = forwarder.applyRemoteUpdate(update.update) ?: break
                appliedRemoteUpdates.add(update)
            }

            val targetText = converged
            if (targetText != null && appliedRemoteUpdates.isNotEmpty() && !hasPendingLocal(filePath)) {
            if (queueRemoteTextApply(filePath, expectedText, targetText, forwarder, appliedRemoteUpdates)) {
                queuedForEditor = true
            } else {
                    editorBufferText(filePath)?.let { current ->
                        shadows[filePath] = current
                        forwarder.ensureEditorText(current)
                        requestRemoteDrain(filePath, "editor-buffer-after-apply-failure")
                    }
                }
            }
            usefulWork = peerUpdateCount + ackCount
        } finally {
            logSlow(
                "remote-drain-file",
                filePath,
                started,
                details = "delivery=$deliveryKind updates=$updateCount peer=$peerUpdateCount self=$selfEchoCount acked=$ackCount queued=$queuedForEditor",
            )
        }
        return usefulWork
    }

    /**
     * D2 — apply a REPLACE delivery: install the corrected canonical text into the
     * buffer wholesale (an out-of-band deletion the additive CRDT delta cannot
     * express), then re-bootstrap the local replica node so later deltas are
     * relative to the corrected state. Never clobbers editor-buffer text that
     * has advanced past the local replica baseline; in that case the buffer is
     * published back through the relay and the replacement is dropped.
     */
    private fun applyReplaceDelivery(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
        expectedText: String,
        canonical: String,
    ): Boolean {
        if (hasPendingLocal(filePath)) return false
        val started = System.nanoTime()
        var installed = false
        var deferredEditorText: String? = null
        val replicaText = forwarder.replicaText()
        try {
            ApplicationManager.getApplication().invokeAndWait {
                val edtStarted = System.nanoTime()
                try {
                    val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeAndWait
                    val document = FileDocumentManager.getInstance().getDocument(targetFile) ?: return@invokeAndWait
                    val before = document.text
                    if (before == canonical) {
                        shadows[filePath] = canonical
                        installed = true
                        return@invokeAndWait
                    }
                    if (hasPendingLocal(filePath)) return@invokeAndWait
                    if (!remoteCrdtReplaceStillCurrentUtil(expectedText, before, replicaText)) {
                        val editorHash = contentHash(before)
                        val expectedHash = contentHash(expectedText)
                        val replicaHash = replicaText?.let(::contentHash) ?: "missing"
                        log.warn(
                            "[crdt-replica] replace delivery deferred because editor buffer is authoritative for $filePath: " +
                                "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash canonical_hash=${contentHash(canonical)}"
                        )
                        deferredEditorText = before
                        return@invokeAndWait
                    }
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
                } finally {
                    logSlow("replace-apply-edt", filePath, edtStarted, warnMs = CRDT_EDT_WARN_MS, details = "target_chars=${canonical.length}")
                }
            }
        } finally {
            logSlow("replace-apply-total", filePath, started, details = "target_chars=${canonical.length} installed=$installed deferred=${deferredEditorText != null}")
        }
        deferredEditorText?.let { editorText ->
            shadows[filePath] = editorText
            forwarder.ensureEditorText(editorText)
            requestRemoteDrain(filePath, "editor-buffer-authoritative-replace")
            return false
        }
        if (installed) {
            // Re-open from the canonical bootstrap. Editing the divergent local
            // CRDT until its *text* matches would mint replacement ops and merge
            // them back into a canonical that already contains the response,
            // potentially duplicating content. A true rebootstrap discards the
            // divergent lineage and also retires its stale pending delivery.
            if (forwarders.remove(filePath, forwarder)) {
                forwarder.deregister()
                val reattached = forwarderFor(
                    filePath,
                    canonical,
                    alignExisting = false,
                    bypassRegisterBackoff = true,
                )
                if (reattached == null) {
                    log.warn("[crdt-replica] canonical re-bootstrap could not reattach ${File(filePath).name}; the normal attach path will retry")
                }
            }
        }
        return installed
    }

    private fun queueRemoteTextApply(
        filePath: String,
        expectedText: String,
        converged: String,
        forwarder: CrdtReplicaForwarder,
        updates: List<ReplicaRemoteUpdate>,
    ): Boolean {
        val normalized = normalizeRemoteText(filePath, converged) ?: return false
        if (normalized != converged) {
            log.warn("[crdt-replica] remote update requires template-structure repair for $filePath; rejecting to keep replica state coherent")
            return false
        }
        remoteEditorApplyPaths.add(filePath)
        val outcome = remoteEditorApplies.ingress(
            filePath,
            PendingRemoteEditorApply(
                filePath = filePath,
                expectedText = expectedText,
                targetText = converged,
                acknowledgements = updates.map { PendingRemoteAck(forwarder, it) },
            ),
        )
        log.debug(
            "[crdt-replica] remote editor apply ${outcome.name.lowercase()} for ${File(filePath).name}; " +
                "pending_keys=${remoteEditorApplies.pendingKeyCount()} updates=${updates.size}",
        )
        scheduleRemoteEditorApply()
        return outcome != IngressOutcome.Blocked && outcome != IngressOutcome.Dropped
    }

    private fun scheduleRemoteEditorApply() {
        if (disposed.get() || project.isDisposed) return
        if (!remoteEditorApplyScheduled.compareAndSet(false, true)) return
        try {
            ApplicationManager.getApplication().invokeLater {
                try {
                    if (disposed.get() || project.isDisposed) {
                        remoteEditorApplies.clear()
                    } else {
                        remoteEditorApplies.drainOne()?.second?.let(::applyRemoteTextOnEdt)
                    }
                } finally {
                    remoteEditorApplyScheduled.set(false)
                    if (remoteEditorApplies.hasPending()) scheduleRemoteEditorApply()
                }
            }
        } catch (e: RuntimeException) {
            remoteEditorApplyScheduled.set(false)
            throw e
        }
    }

    private fun applyRemoteTextOnEdt(pending: PendingRemoteEditorApply) {
        val started = System.nanoTime()
        val outcome = try {
            val targetFile = LocalFileSystem.getInstance().findFileByPath(pending.filePath)
                ?: return completeRemoteEditorApply(pending, RemoteEditorApplyOutcome(false, null), started)
            val document = FileDocumentManager.getInstance().getDocument(targetFile)
                ?: return completeRemoteEditorApply(pending, RemoteEditorApplyOutcome(false, null), started)
            val before = document.text
            if (before == pending.targetText) {
                shadows[pending.filePath] = pending.targetText
                RemoteEditorApplyOutcome(true, before)
            } else if (hasPendingLocal(pending.filePath)) {
                RemoteEditorApplyOutcome(false, before)
            } else if (!remoteCrdtApplyStillCurrentUtil(pending.expectedText, before, pending.targetText)) {
                log.warn("[crdt-replica] stale coalesced remote update rejected for ${pending.filePath}; editor text advanced before apply")
                RemoteEditorApplyOutcome(false, before)
            } else {
                applyingRemote.add(pending.filePath)
                try {
                    runUndoableRemoteUpdateCommand(document) {
                        applyMinimalDocumentEditUtil(document, before, pending.targetText)
                        shadows[pending.filePath] = pending.targetText
                    }
                    RemoteEditorApplyOutcome(true, pending.targetText)
                } finally {
                    applyingRemote.remove(pending.filePath)
                }
            }
        } catch (e: RuntimeException) {
            log.warn("[crdt-replica] coalesced remote editor apply failed for ${pending.filePath}", e)
            RemoteEditorApplyOutcome(false, null)
        }
        completeRemoteEditorApply(pending, outcome, started)
    }

    private fun completeRemoteEditorApply(
        pending: PendingRemoteEditorApply,
        outcome: RemoteEditorApplyOutcome,
        started: Long,
    ) {
        logSlow(
            "remote-apply-edt",
            pending.filePath,
            started,
            warnMs = CRDT_EDT_WARN_MS,
            details = "target_chars=${pending.targetText.length} applied=${outcome.applied} coalesced_updates=${pending.acknowledgements.size}",
        )
        if (disposed.get()) return
        try {
            executor.execute {
                try {
                    var acked = 0
                    if (outcome.applied) {
                        for (ack in pending.acknowledgements) {
                            if (ack.forwarder.ackRemoteUpdate(ack.update, outcome.editorText)) acked++
                        }
                    } else if (outcome.editorText != null) {
                        shadows[pending.filePath] = outcome.editorText
                        pending.acknowledgements.lastOrNull()?.forwarder?.ensureEditorText(outcome.editorText)
                    }
                    log.debug(
                        "[crdt-replica] remote editor apply completed for ${File(pending.filePath).name}; " +
                            "applied=${outcome.applied} acked=$acked coalesced_updates=${pending.acknowledgements.size}",
                    )
                } finally {
                    remoteEditorApplyPaths.remove(pending.filePath)
                    requestRemoteDrain(pending.filePath, "remote-editor-apply-complete")
                }
            }
        } catch (_: RejectedExecutionException) {
            remoteEditorApplyPaths.remove(pending.filePath)
        }
    }

    private fun editorReplicaBaselineMatches(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
        expectedText: String,
    ): Boolean {
        val editorText = editorBufferText(filePath) ?: return false
        val replicaText = forwarder.replicaText()
        if (editorText == expectedText && replicaText == expectedText) return true
        val editorHash = contentHash(editorText)
        val expectedHash = contentHash(expectedText)
        val replicaHash = replicaText?.let(::contentHash) ?: "missing"
        if (replicaText == expectedText) {
            log.warn(
                "[crdt-replica] incoming update deferred while editor buffer is published first for $filePath: " +
                    "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash"
            )
            shadows[filePath] = editorText
            forwarder.ensureEditorText(editorText)
            requestRemoteDrain(filePath, "editor-buffer-ahead")
            return false
        }
        if (replicaText == editorText) {
            log.warn(
                "[crdt-replica] incoming update deferred after shadow realignment for $filePath: " +
                    "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash"
            )
            shadows[filePath] = editorText
            requestRemoteDrain(filePath, "shadow-realigned")
            return false
        }
        log.warn(
            "[crdt-replica] incoming update deferred because local replica baseline differs from the authoritative editor buffer for $filePath: " +
                "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash"
        )
        shadows[filePath] = editorText
        forwarder.ensureEditorText(editorText)
        requestRemoteDrain(filePath, "editor-buffer-authoritative")
        return false
    }

    private fun editorBufferText(filePath: String): String? =
        ApplicationManager.getApplication().runReadAction<String?> {
            val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@runReadAction null
            FileDocumentManager.getInstance().getDocument(targetFile)?.text
        }

    private fun contentHash(text: String): String =
        java.security.MessageDigest.getInstance("SHA-256")
            .digest(text.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }

    private fun normalizeRemoteText(filePath: String, converged: String): String? {
        val started = System.nanoTime()
        return try {
            NativePatching.normalizeTemplateStructure(converged).also { normalized ->
                if (normalized == null) {
                    log.warn("[crdt-replica] remote update rejected by template-structure guard for $filePath")
                }
            }
        } finally {
            logSlow("remote-normalize-worker", filePath, started, details = "target_chars=${converged.length}")
        }
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

    private fun forwarderFor(
        filePath: String,
        initialEditorText: String? = null,
        alignExisting: Boolean = false,
        bypassRegisterBackoff: Boolean = false,
    ): CrdtReplicaForwarder? {
        forwarders[filePath]?.let {
            if (alignExisting && initialEditorText != null) {
                it.ensureEditorText(initialEditorText)
            }
            return it
        }
        if (!bypassRegisterBackoff && !shouldAttemptRegister(filePath)) return null
        val root = resolveProjectRoot(filePath) ?: return null
        val identity = "${EditorIdentity.id}:$filePath"
        val forwarder = CrdtReplicaForwarder(
            filePath = filePath,
            identity = identity,
            node = NativeReplicaNode(),
            transport = CpcSocketReplicaTransport(root),
        )
        if (!forwarder.register()) {
            recordRegisterFailure(filePath)
            return null
        }
        clearRegisterFailure(filePath)
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

    private fun shouldAttemptRegister(filePath: String): Boolean {
        val retryAfter = registerRetryAfterMs[filePath] ?: return true
        val now = System.currentTimeMillis()
        if (now >= retryAfter) return true
        log.debug("[crdt-replica] register skipped for $filePath; retry_after_ms=${retryAfter - now}")
        return false
    }

    private fun recordRegisterFailure(filePath: String) {
        val now = System.currentTimeMillis()
        val failureCount = registerFailureCounts.merge(filePath, 1) { old, _ -> (old + 1).coerceAtMost(16) } ?: 1
        val step = (failureCount - 1).coerceAtLeast(0).coerceAtMost(3)
        val backoffMs = (CRDT_REGISTER_FAILURE_BASE_BACKOFF_MS * (1L shl step))
            .coerceAtMost(CRDT_REGISTER_FAILURE_MAX_BACKOFF_MS)
        registerRetryAfterMs[filePath] = now + backoffMs
        log.warn("[crdt-replica] register failed for ${File(filePath).name}; failure_count=$failureCount retry_backoff_ms=$backoffMs")
    }

    private fun clearRegisterFailure(filePath: String) {
        registerFailureCounts.remove(filePath)
        registerRetryAfterMs.remove(filePath)
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

    private fun logSlow(
        operation: String,
        filePath: String,
        startedNanos: Long,
        warnMs: Long = CRDT_WORKER_WARN_MS,
        details: String = "",
    ) {
        val elapsedMs = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - startedNanos)
        val suffix = if (details.isBlank()) "" else " $details"
        val name = if (filePath == "(none)") filePath else File(filePath).name
        val message = "[crdt-perf] $operation file=$name elapsed_ms=$elapsedMs thread=${Thread.currentThread().name}$suffix"
        if (elapsedMs >= warnMs) {
            log.warn(message)
        } else {
            log.debug(message)
        }
    }

    companion object {
        private val instances = ConcurrentHashMap<Project, CrdtReplicaManager>()
        private val applyingAgentMutations = ConcurrentHashMap.newKeySet<String>()

        fun getInstance(project: Project): CrdtReplicaManager =
            instances.getOrPut(project) {
                CrdtReplicaManager(project).also { it.start() }
            }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }

        fun requestRemoteDrain(project: Project, filePath: String? = null, reason: String = "event") {
            instances[project]?.requestRemoteDrain(filePath, reason)
        }

        fun requestTextAdopt(project: Project, filePath: String) {
            instances[project]?.requestTextAdopt(filePath)
        }

        fun <T> withAgentAppliedEditorMutation(filePath: String, block: () -> T): T {
            applyingAgentMutations.add(filePath)
            return try {
                block()
            } finally {
                applyingAgentMutations.remove(filePath)
            }
        }

        fun ensureReplicaForOpenDocument(
            filePath: String,
            document: Document,
            editorText: String? = null,
            await: Boolean = false,
            forceRefresh: Boolean = false,
        ): Boolean {
            val manager = instances.values.firstOrNull { it.ownsFilePath(filePath) }
                ?: instances.values.firstOrNull()
                ?: return false
            return manager.ensureOpenDocumentReplica(filePath, document, editorText, await, forceRefresh)
        }

        fun isApplyingRemote(filePath: String): Boolean =
            instances.values.any { it.applyingRemote.contains(filePath) }

        fun isApplyingNonOperatorMutation(filePath: String): Boolean =
            applyingAgentMutations.contains(filePath) || isApplyingRemote(filePath)
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

internal fun remoteCrdtReplaceStillCurrentUtil(
    expectedText: String,
    currentText: String,
    replicaText: String?,
): Boolean =
    currentText == expectedText && replicaText == expectedText
