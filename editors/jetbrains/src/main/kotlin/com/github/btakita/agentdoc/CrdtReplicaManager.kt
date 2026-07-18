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
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
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
import java.util.concurrent.atomic.AtomicLong

private const val CRDT_LISTENER_WARN_MS = 10L
private const val CRDT_WORKER_WARN_MS = 100L
private const val CRDT_EDT_WARN_MS = 50L
private const val CRDT_AWAIT_ATTACH_TIMEOUT_MS = 750L
private const val CRDT_AWAIT_CLOSE_PUBLISH_TIMEOUT_MS = 2_000L
private const val CRDT_REGISTER_FAILURE_BASE_BACKOFF_MS = 1_000L
private const val CRDT_REGISTER_FAILURE_MAX_BACKOFF_MS = 30_000L
private const val CRDT_DRAIN_NOOP_RESCHEDULE_BASE_BACKOFF_MS = 100L
// `#crdt-drain-idle-quiet`: the no-op drain-all loop must keep polling so purely-remote
// CRDT updates (a peer edits with no local event here) still get pulled — but an idle
// replica set does not need a 5s cadence. Cap the idle reschedule at 30s so a workspace
// full of parked session-doc replicas stops waking every 5s (observed steady-state
// churn across ~9 attached replicas). Active editing / authority-publish / open-document
// events still trigger an immediate drain, so only passive remote-only observation on an
// otherwise-idle doc sees up to 30s of extra latency.
private const val CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS = 30_000L
// Delivery routability is a component-level fact, not merely an IDE-process
// fact. Refresh from the same serialized executor that pulls/applies/ACKs CRDT
// deliveries: if that worker stalls, this heartbeat stalls too and Rust stops
// targeting it while continuing to protect its possibly-unsaved buffer.

private data class PendingRemoteAck(
    val forwarder: CrdtReplicaForwarder,
    val update: ReplicaRemoteUpdate,
)

internal data class RemoteAckReplayPlan(
    val candidate: ReplicaRemoteUpdate,
    val acknowledgedThroughGeneration: Long?,
)

/**
 * Pick one oldest delivery as the cumulative ACK carrier. The controller
 * matches [visibleContentHash] against the newest represented pending target
 * and drains the entire prefix atomically, so replaying every historical
 * generation only creates head-of-line blocking while the editor is typing.
 */
internal fun remoteAckReplayPlanUtil(
    updates: Collection<ReplicaRemoteUpdate>,
    visibleContentHash: String,
): RemoteAckReplayPlan? {
    val candidate = updates.minWithOrNull(compareBy<ReplicaRemoteUpdate> { it.generation }.thenBy { it.patchId })
        ?: return null
    val acknowledgedThrough = updates.asSequence()
        .filter { it.expectedContentHash == visibleContentHash }
        .maxOfOrNull { it.generation }
    return RemoteAckReplayPlan(candidate, acknowledgedThrough)
}

/**
 * An unacknowledged visible frontier owns the delivery slot. Pulling again while
 * that frontier is retained only returns the same controller delivery and would
 * decode the same (potentially multi-megabyte) CRDT update into the replica on
 * every drain cycle. Let the existing no-op drain backoff retry the ACK first.
 */
internal fun shouldPullRemoteDeliveryAfterAckReplayUtil(pendingAckCount: Int): Boolean =
    pendingAckCount == 0

/** A retained ACK frontier owns the retry cadence. File-watcher and editor
 * events may add work while backoff is active, but must not bypass that gate
 * and hammer the controller with the same rejected ACK. */
internal fun shouldStartRemoteDrainUtil(backoffScheduled: Boolean): Boolean = !backoffScheduled

/**
 * `#crdtpushdrain`: a controller-published CRDT remote event is positive evidence
 * that the CPC already holds a frontier for this document, so it must bypass the
 * speculative no-op drain backoff instead of being suppressed by it.
 *
 * The no-op backoff (see [scheduleRemoteDrainAfterBackoff]) exists to stop a
 * *self-driven* drain spin when there is nothing to pull; on an idle document it
 * climbs to [CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS]. That is exactly the state a
 * document sits in when the operator triggers Compact Exchange, so gating the
 * push event behind it stalled every compact/finalize until the binary escalated
 * to `ack_recovery_force_refresh` after `CRDT_ACK_FORCE_REFRESH_AFTER_MS` — a fixed
 * ~2s toll on the hot path. Controller pushes are externally rate-limited (one per
 * `CRDT_ACK_REPLAY_SIGNAL_INTERVAL_MS`, only while a write awaits ACK), so draining
 * them eagerly cannot reintroduce the spin.
 *
 * `request_full_state` is excluded because it owns the separate text-adopt path.
 */
internal fun shouldUrgentDrainForRemoteEventUtil(reasonToken: String?): Boolean =
    reasonToken != "request_full_state"

@Suppress("UNUSED_PARAMETER")
internal fun shouldAcknowledgeVisibleRemoteDeliveryUtil(
    editorText: String?,
    targetText: String,
    diskPersisted: Boolean,
): Boolean = editorText == targetText

internal enum class TemplateStructureProjectionState {
    Exact,
    RepairRequired,
    Invalid,
}

internal fun templateStructureProjectionStateUtil(
    text: String,
    normalized: String?,
): TemplateStructureProjectionState = when {
    normalized == null -> TemplateStructureProjectionState.Invalid
    normalized == text -> TemplateStructureProjectionState.Exact
    else -> TemplateStructureProjectionState.RepairRequired
}

internal fun remoteReplaceStructureAcceptedUtil(
    remoteState: TemplateStructureProjectionState,
): Boolean = remoteState == TemplateStructureProjectionState.Exact

internal fun replicaRegistrationStructureAcceptedUtil(
    editorState: TemplateStructureProjectionState,
): Boolean = editorState != TemplateStructureProjectionState.Invalid

internal enum class RemoteTemplateProjectionDecision {
    QueueRemote,
    AdoptExactEditorBaseline,
    RetryFailClosed,
}

internal fun remoteTemplateProjectionDecisionUtil(
    remoteState: TemplateStructureProjectionState,
    editorState: TemplateStructureProjectionState?,
    editorMatchesExpected: Boolean,
    recoveryInFlight: Boolean,
): RemoteTemplateProjectionDecision = when {
    remoteState == TemplateStructureProjectionState.Exact ->
        RemoteTemplateProjectionDecision.QueueRemote
    recoveryInFlight -> RemoteTemplateProjectionDecision.RetryFailClosed
    editorMatchesExpected && editorState == TemplateStructureProjectionState.Exact ->
        RemoteTemplateProjectionDecision.AdoptExactEditorBaseline
    else -> RemoteTemplateProjectionDecision.RetryFailClosed
}

internal enum class ReplicaBaselineDecision {
    ApplyRemote,
    RealignShadow,
    AdoptExactEditor,
    RetryFailClosed,
}

/**
 * The visible editor is the operator-authoritative plane. A save or a stale
 * native replica must never turn that text into a second logical mutation or
 * project an older replica snapshot over it.
 */
internal fun replicaBaselineDecisionUtil(
    editorState: TemplateStructureProjectionState?,
    editorMatchesExpected: Boolean,
    replicaMatchesExpected: Boolean,
    replicaMatchesEditor: Boolean,
    recoveryInFlight: Boolean,
): ReplicaBaselineDecision = when {
    recoveryInFlight || editorState != TemplateStructureProjectionState.Exact ->
        ReplicaBaselineDecision.RetryFailClosed
    editorMatchesExpected && replicaMatchesExpected -> ReplicaBaselineDecision.ApplyRemote
    replicaMatchesEditor -> ReplicaBaselineDecision.RealignShadow
    else -> ReplicaBaselineDecision.AdoptExactEditor
}

internal fun shouldForwardLocalDeltaUtil(replicaText: String?, shadowText: String): Boolean =
    replicaText == shadowText

internal fun pullDeliveryRequestsReplicaRefreshUtil(delivery: ReplicaPullDelivery): Boolean =
    delivery is ReplicaPullDelivery.Unavailable

private data class PendingRemoteEditorApply(
    val filePath: String,
    val expectedText: String,
    val targetText: String,
    val acknowledgements: List<PendingRemoteAck>,
)

private data class RemoteEditorApplyOutcome(
    val diskPersisted: Boolean,
    val editorText: String?,
)

private enum class RemoteTextApplyDisposition {
    Queued,
    Recovered,
    RetryFailClosed,
}

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
 * minimal-edit helper as IPC patches. A remote mutation is saved before acknowledgement only
 * when raw disk still equals the guarded editor baseline or the converged target; novel external
 * disk text rejects the apply instead of being overwritten.
 */
class CrdtReplicaManager(private val project: Project) : Disposable, DocumentListener {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(CrdtReplicaManager::class.java)
    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-crdt-replica-delivery").apply { isDaemon = true }
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
    // An editor apply and its controller ACK cross two different queues (EDT ->
    // replica worker -> controller socket). Keep the ACK as Lazily-style retained
    // state until the controller accepts the exact current editor-content proof.
    // A socket/controller recycle must not turn a successful visible apply into a
    // permanently orphaned delivery frontier.
    private val pendingRemoteAckReplays =
        ConcurrentHashMap<String, ConcurrentHashMap<String, PendingRemoteAck>>()
    private val templateGuardRecoveryPaths = ConcurrentHashMap.newKeySet<String>()
    private val templateGuardRecoveryRetryPaths = ConcurrentHashMap.newKeySet<String>()
    private val templateGuardRecoveryFailureCounts = ConcurrentHashMap<String, Int>()
    private val refreshConnectionEpoch = AtomicLong(0)
    private val disposed = AtomicBoolean(false)

    fun start() {
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(this, this)
    }

    override fun dispose() {
        disposed.set(true)
        remoteEditorApplies.clear()
        remoteEditorApplyPaths.clear()
        pendingRemoteAckReplays.clear()
        templateGuardRecoveryPaths.clear()
        templateGuardRecoveryRetryPaths.clear()
        templateGuardRecoveryFailureCounts.clear()
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
            if (managerForFilePath(filePath) !== this) return
            if (!CrdtReplicaManager.isOperatorDocumentEvent(filePath, event)) {
                // A clean File Cache Conflict reload may be the operator
                // accepting a Lazily-retained external disk candidate. Resolve
                // on the worker; exact CAS/rebootstrap rules keep ordinary
                // remote CRDT applies as no-ops here.
                ensureOpenDocumentReplica(filePath, event.document, forceRefresh = true)
                requestRemoteDrain(filePath, "non-operator-editor-event")
                return
            }
            val projectionEpoch = nonOperatorMutationEpoch(filePath)
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
                        projectionEpoch,
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
                forwarderFor(filePath, text)
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
        val attach = attach@{
            val started = System.nanoTime()
            var chars = -1
            try {
                val text = editorText ?: ApplicationManager.getApplication().runReadAction<String> { document.text }
                chars = text.length
                // The open IntelliJ Document is the live authority. A forced refresh
                // must never install a retained whole-document target before the
                // replacement replica exists: that target may predate unsaved operator
                // prompts/deletions and saving it here makes the loss durable. Register
                // from this exact editor cut, then let binary-owned semantic intents
                // replay granularly over the new baseline.
                val registrationText = text
                val registrationState = templateStructureState(filePath, registrationText, "replica-registration")
                if (!replicaRegistrationStructureAcceptedUtil(registrationState)) {
                    log.warn(
                        "[crdt-replica] refused malformed open-document replica registration for ${File(filePath).name}; " +
                            "recovery=retry_deferred_intent_rebase operator_action=none",
                    )
                    return@attach false
                }
                chars = registrationText.length
                val forwarder = forwarderFor(
                    filePath,
                    registrationText,
                    bypassRegisterBackoff = forceRefresh,
                    replaceCached = forceRefresh,
                    expectedEditorTextAtSwap = if (forceRefresh) registrationText else null,
                )
                (forwarder != null).also { attached ->
                    if (attached) {
                        shadows[filePath] = registrationText
                        // Registration published this exact visible editor cut.
                        // Let the binary settle a matching reconnect decision,
                        // but never fetch or install a whole-document target here.
                        NativePatching.deferredWriteReconnectPropagated(filePath, registrationText)
                        requestRemoteDrain(filePath, "open-document")
                    }
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

    /**
     * Publish the exact closing editor cut through the same serialized Lazily
     * replica worker as every preceding local delta, then retire that replica.
     * The reliable-sync close fact is emitted only after this returns true, so
     * the controller can hand authority to disk without losing a last unsaved
     * deletion that was still waiting behind the debounce worker.
     */
    private fun publishClosingDocumentCut(filePath: String, document: Document): Boolean {
        return try {
            executor.submit<Boolean> {
                val text = ApplicationManager.getApplication().runReadAction<String> { document.text }
                val forwarder = forwarderFor(filePath, text) ?: return@submit false
                forwarder.ensureEditorText(text)
                shadows[filePath] = text
                clearLocalPending(filePath)
                if (forwarders.remove(filePath, forwarder)) {
                    forwarder.deregister()
                }
                true
            }.get(CRDT_AWAIT_CLOSE_PUBLISH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        } catch (e: TimeoutException) {
            log.warn(
                "[crdt-replica] closing editor cut publish timed out for $filePath after ${CRDT_AWAIT_CLOSE_PUBLISH_TIMEOUT_MS}ms",
            )
            false
        } catch (e: Exception) {
            log.warn("[crdt-replica] closing editor cut publish failed for $filePath: ${e.message}")
            false
        }
    }

    private fun forwardLocalDeltaFromShadow(
        filePath: String,
        document: Document,
        eventOffset: Int,
        oldFragment: String,
        newFragment: String,
        projectionEpoch: Long,
    ) {
        val started = System.nanoTime()
        if (projectionEpoch != nonOperatorMutationEpoch(filePath)) {
            log.debug(
                "[crdt-replica] dropped stale operator event for $filePath after a newer CPC projection",
            )
            requestRemoteDrain(filePath, "stale-operator-event-fenced")
            return
        }
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
        if (forwarder != null) {
            val replicaText = forwarder.replicaText()
            if (shouldForwardLocalDeltaUtil(replicaText, beforeText)) {
                forwarder.forwardLocalDelta(offset, deleteLen, newFragment)
            } else {
                log.warn(
                    "[crdt-replica] local delta found a stale native baseline for ${File(filePath).name}; " +
                        "shadow_hash=${contentHash(beforeText)} " +
                        "replica_hash=${replicaText?.let(::contentHash) ?: "missing"} " +
                        "recovery=exact_editor_adopt_then_atomic_reregister",
                )
                adoptExactEditorBaseline(
                    filePath = filePath,
                    editorText = nextText,
                    staleForwarder = forwarder,
                    allowPendingLocal = true,
                    reason = "local-delta-baseline-diverged",
                )
            }
        }
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
        if (!shouldStartRemoteDrainUtil(remoteDrainBackoffScheduled.get())) return
        if (!drainQueued.compareAndSet(false, true)) return
        if (!shouldStartRemoteDrainUtil(remoteDrainBackoffScheduled.get())) {
            drainQueued.set(false)
            return
        }
        executor.execute {
            var appliedTotal = 0
            try {
                appliedTotal = drainRemoteUpdates(reason)
            } catch (e: Exception) {
                log.debug("[crdt-replica] remote drain skipped: ${e.message}")
            } finally {
                val moreWorkRequested = drainAllRequested.get() || drainRequestedPaths.isNotEmpty()
                if (moreWorkRequested && appliedTotal == 0) {
                    // #crdt-drain-backoff: when a drain cycle applied zero useful
                    // updates (notably when the CPC socket is unavailable and every
                    // pullDelivery returns empty deltas), delay the reschedule with
                    // exponential backoff instead of re-executing immediately. A
                    // tight no-op spin generated ~70MB/min of logs and froze the IDE.
                    val delayMs = nextNoOpRescheduleBackoffMs()
                    log.debug("[crdt-replica] no-op drain cycle; backing off reschedule by ${delayMs}ms (consecutive=${consecutiveNoOpReschedules.get()})")
                    // Publish the backoff gate before releasing drainQueued so an
                    // external CRDT event cannot win the gap and start immediately.
                    scheduleRemoteDrainAfterBackoff(delayMs, reason)
                    drainQueued.set(false)
                } else if (moreWorkRequested) {
                    consecutiveNoOpReschedules.set(0)
                    drainQueued.set(false)
                    requestRemoteDrain(reason = "rescheduled")
                } else {
                    consecutiveNoOpReschedules.set(0)
                    drainQueued.set(false)
                }
            }
        }
    }

    /**
     * Foreground delivery recovery for a controller write that is already
     * retained in the existing replica's ACK frontier. This deliberately
     * bypasses only the background no-op drain timer: it neither clears that
     * timer nor replaces the replica. Re-registering from the visible editor
     * here would publish the pre-delivery buffer back into canonical and undo
     * the controller write before the editor had a chance to apply it.
     */
    fun requestUrgentRemoteDrain(filePath: String, reason: String) {
        executor.execute {
            val forwarder = forwarders[filePath] ?: return@execute
            var applied = 0
            try {
                applied = drainRemoteUpdatesFor(filePath, forwarder)
            } catch (e: Exception) {
                log.debug("[crdt-replica] urgent remote drain skipped for $filePath: ${e.message}")
            } finally {
                log.debug(
                    "[crdt-replica] urgent remote drain completed for ${File(filePath).name}; " +
                        "reason=$reason applied=$applied",
                )
                if (applied == 0 && !disposed.get()) {
                    requestRemoteDrain(filePath, "$reason-follow-up")
                } else if (applied > 0) {
                    // #crdtpushdrain: useful work proves the document is live again,
                    // so the escalated no-op backoff is stale. Without this reset the
                    // gate stays parked at its previous (up to 30s) delay and the
                    // *next* controller push is suppressed all over again.
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
            if (!TypingTracker.hasUnsyncedOperatorEdits(filePath)) {
                log.info(
                    "[reattach-adopt] refused full editor text adopt for ${File(filePath).name}; " +
                        "no unsynced operator edit proves editor-origin content",
                )
                requestRemoteDrain(filePath, "reattach-cpc-projection-only")
                return@execute
            }
            val forwarder = forwarders[filePath] ?: return@execute
            val editorText = shadows[filePath] ?: return@execute
            if (!forwarder.pushTextAdopt(editorText)) return@execute
            if (forwarders[filePath] === forwarder) {
                val reattached = forwarderFor(
                    filePath,
                    editorText,
                    bypassRegisterBackoff = true,
                    expectedEditorTextAtSwap = editorText,
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
            // Retry an ACK that lost its controller round-trip before pulling more
            // work. The proof is always recomputed from the current editor buffer;
            // stale remembered text is never allowed to acknowledge a newer cut.
            ackCount += replayPendingRemoteAcks(filePath, forwarder)
            usefulWork = ackCount
            val pendingAckCount = pendingRemoteAckCount(filePath, forwarder)
            if (!shouldPullRemoteDeliveryAfterAckReplayUtil(pendingAckCount)) {
                log.debug(
                    "[crdt-replica] remote pull deferred for ${File(filePath).name}; " +
                        "pending_ack_frontier=$pendingAckCount acked=$ackCount",
                )
                return usefulWork
            }
            // D2: a replace delivery (out-of-band deletion re-bootstrap) installs
            // the corrected canonical only when the editor buffer still matches
            // the local replica baseline; normal deltas are merged into the native
            // replica first, then applied to the editor in one EDT command.
            val delivery = forwarder.pullRemoteDelivery()
            if (pullDeliveryRequestsReplicaRefreshUtil(delivery)) {
                val reason = (delivery as ReplicaPullDelivery.Unavailable).reason
                refreshReplicaAfterTransportLoss(filePath, forwarder, expectedText, reason)
                return usefulWork
            }
            if (delivery is ReplicaPullDelivery.Replace) {
                deliveryKind = "replace"
            queuedForEditor = applyReplaceDelivery(filePath, forwarder, expectedText, delivery.text)
                usefulWork += if (queuedForEditor) 1 else 0
                return usefulWork
            }
            val updates = (delivery as ReplicaPullDelivery.Deltas).updates
            updateCount = updates.size
            usefulWork = updateCount
            if (updates.isEmpty()) return usefulWork

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
                    if (forwarder.ackRemoteUpdate(update, visibleText)) {
                        ackCount++
                    } else {
                        rememberPendingRemoteAck(filePath, PendingRemoteAck(forwarder, update))
                    }
                    continue
                }
                peerUpdateCount++
                converged = forwarder.applyRemoteUpdate(update.update) ?: break
                appliedRemoteUpdates.add(update)
            }

            val targetText = converged
            if (targetText != null && appliedRemoteUpdates.isNotEmpty() && !hasPendingLocal(filePath)) {
            when (queueRemoteTextApply(filePath, expectedText, targetText, forwarder, appliedRemoteUpdates)) {
                RemoteTextApplyDisposition.Queued -> queuedForEditor = true
                RemoteTextApplyDisposition.Recovered -> usefulWork++
                RemoteTextApplyDisposition.RetryFailClosed -> {
                    editorBufferText(filePath)?.let { current -> shadows[filePath] = current }
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

    private fun remoteAckKey(update: ReplicaRemoteUpdate): String =
        "${update.patchId}:${update.generation}"

    private fun rememberPendingRemoteAck(filePath: String, ack: PendingRemoteAck) {
        pendingRemoteAckReplays
            .computeIfAbsent(filePath) { ConcurrentHashMap() }[remoteAckKey(ack.update)] = ack
    }

    private fun rememberPendingRemoteAcks(filePath: String, updates: List<PendingRemoteAck>) {
        for (ack in updates) rememberPendingRemoteAck(filePath, ack)
    }

    private fun clearPendingRemoteAcks(filePath: String): Int =
        pendingRemoteAckReplays.remove(filePath)?.size ?: 0

    private fun pendingRemoteAckCount(filePath: String, forwarder: CrdtReplicaForwarder): Int =
        pendingRemoteAckReplays[filePath]
            ?.values
            ?.count { it.forwarder === forwarder }
            ?: 0

    private fun replayPendingRemoteAcks(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
        knownVisibleText: String? = null,
    ): Int {
        val pending = pendingRemoteAckReplays[filePath] ?: return 0
        // A completed EDT apply may race a forced member replacement. Drop
        // acknowledgements from the retired identity instead of replaying them
        // through the replacement member.
        for ((key, ack) in pending.entries) {
            if (ack.forwarder !== forwarder) pending.remove(key, ack)
        }
        if (pending.isEmpty()) {
            pendingRemoteAckReplays.remove(filePath, pending)
            return 0
        }
        val visibleText = knownVisibleText ?: editorBufferText(filePath) ?: return 0
        val plan = remoteAckReplayPlanUtil(
            pending.values.map { it.update },
            contentHash(visibleText),
        ) ?: return 0
        if (!forwarder.ackRemoteUpdate(plan.candidate, visibleText)) return 0

        val acknowledgedThrough = plan.acknowledgedThroughGeneration ?: plan.candidate.generation
        var acknowledged = 0
        for ((key, ack) in pending.entries) {
            if (ack.update.generation <= acknowledgedThrough && pending.remove(key, ack)) acknowledged++
        }
        if (pending.isEmpty()) pendingRemoteAckReplays.remove(filePath, pending)
        return acknowledged
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
        val remoteState = templateStructureState(filePath, canonical, "replace-remote")
        if (!remoteReplaceStructureAcceptedUtil(remoteState)) {
            recoverRejectedRemoteCanonical(
                filePath = filePath,
                expectedText = expectedText,
                remoteText = canonical,
                staleForwarder = forwarder,
                remoteState = remoteState,
            )
            return false
        }
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
                    if (!refreshCleanDocumentBeforeRemoteApply(filePath, targetFile, document)) {
                        deferredEditorText = document.text
                        return@invokeAndWait
                    }
                    val before = document.text
                    if (before == canonical) {
                        shadows[filePath] = canonical
                        installed = persistRemoteCrdtTextIfSafe(
                            filePath,
                            document,
                            expectedText,
                            canonical,
                        )
                        return@invokeAndWait
                    }
                    if (hasPendingLocal(filePath)) return@invokeAndWait
                    if (!remoteCrdtReplaceStillCurrentUtil(expectedText, before, replicaText)) {
                        val editorHash = contentHash(before)
                        val expectedHash = contentHash(expectedText)
                        val replicaHash = replicaText?.let(::contentHash) ?: "missing"
                        log.warn(
                            "[crdt-replica] replace delivery observed non-operator editor divergence for $filePath: " +
                                "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash canonical_hash=${contentHash(canonical)}"
                        )
                        deferredEditorText = before
                        return@invokeAndWait
                    }
                    if (!remoteCrdtDiskCanPersistUtil(expectedText, canonical, readRawDiskText(filePath))) {
                        log.warn(
                            "[crdt-replica] replace delivery rejected because disk contains novel external text for $filePath; " +
                                "expected_hash=${contentHash(expectedText)} canonical_hash=${contentHash(canonical)}"
                        )
                        return@invokeAndWait
                    }
                    advanceNonOperatorMutationEpoch(filePath)
                    applyingRemote.add(filePath)
                    try {
                        runUndoableRemoteUpdateCommand(document) {
                            applyMinimalDocumentEditUtil(document, before, canonical)
                        }
                        shadows[filePath] = canonical
                        installed = persistRemoteCrdtTextIfSafe(
                            filePath,
                            document,
                            expectedText,
                            canonical,
                        )
                        if (installed) {
                            log.info("[crdt-replica] applied and saved REPLACE re-bootstrap for $filePath (${canonical.length} chars)")
                        }
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
            log.warn(
                "[crdt-replica] CPC replace deferred to the exact live editor authority for $filePath; " +
                    "editor_hash=${contentHash(editorText)} canonical_hash=${contentHash(canonical)}",
            )
            adoptExactEditorBaseline(
                filePath = filePath,
                editorText = editorText,
                staleForwarder = forwarder,
                allowPendingLocal = false,
                reason = "replace-delivery-editor-diverged",
            )
            return false
        }
        if (installed) {
            // Re-open from the canonical bootstrap. Editing the divergent local
            // CRDT until its *text* matches would mint replacement ops and merge
            // them back into a canonical that already contains the response,
            // potentially duplicating content. A true rebootstrap discards the
            // divergent lineage and also retires its stale pending delivery.
            if (forwarders[filePath] === forwarder) {
                val reattached = forwarderFor(
                    filePath,
                    canonical,
                    bypassRegisterBackoff = true,
                    expectedEditorTextAtSwap = canonical,
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
    ): RemoteTextApplyDisposition {
        val remoteState = templateStructureState(filePath, converged, "remote")
        if (remoteState != TemplateStructureProjectionState.Exact) {
            return recoverRejectedRemoteCanonical(
                filePath = filePath,
                expectedText = expectedText,
                remoteText = converged,
                staleForwarder = forwarder,
                remoteState = remoteState,
            )
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
        return if (outcome != IngressOutcome.Blocked && outcome != IngressOutcome.Dropped) {
            RemoteTextApplyDisposition.Queued
        } else {
            scheduleTemplateGuardRecoveryRetry(filePath, "remote-editor-apply-${outcome.name.lowercase()}")
            RemoteTextApplyDisposition.RetryFailClosed
        }
    }

    private fun recoverRejectedRemoteCanonical(
        filePath: String,
        expectedText: String,
        remoteText: String,
        staleForwarder: CrdtReplicaForwarder,
        remoteState: TemplateStructureProjectionState,
    ): RemoteTextApplyDisposition {
        val editorText = editorBufferText(filePath)
        val editorState = editorText?.let { templateStructureState(filePath, it, "editor-recovery") }
        val decision = remoteTemplateProjectionDecisionUtil(
            remoteState = remoteState,
            editorState = editorState,
            editorMatchesExpected = editorText == expectedText,
            recoveryInFlight = templateGuardRecoveryPaths.contains(filePath),
        )
        if (decision != RemoteTemplateProjectionDecision.AdoptExactEditorBaseline || editorText == null) {
            log.warn(
                "[crdt-replica] template-guard recovery deferred for ${File(filePath).name}; " +
                    "remote_state=$remoteState editor_state=$editorState " +
                    "editor_matches_expected=${editorText == expectedText} " +
                    "recovery_in_flight=${templateGuardRecoveryPaths.contains(filePath)} " +
                    "remote_hash=${contentHash(remoteText)}",
            )
            scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-proof-missing")
            return RemoteTextApplyDisposition.RetryFailClosed
        }
        if (!templateGuardRecoveryPaths.add(filePath)) {
            scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-recovery-active")
            return RemoteTextApplyDisposition.RetryFailClosed
        }
        try {
            // Revalidate all mutable evidence at the adopt boundary. A document event
            // marks local work pending synchronously, before its delta reaches this worker.
            if (
                forwarders[filePath] !== staleForwarder ||
                hasPendingLocal(filePath) ||
                editorBufferText(filePath) != editorText
            ) {
                scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-adopt-fence-raced")
                return RemoteTextApplyDisposition.RetryFailClosed
            }
            if (!staleForwarder.pushTextAdopt(editorText)) {
                scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-adopt-push-failed")
                return RemoteTextApplyDisposition.RetryFailClosed
            }
            val replacement = forwarderFor(
                filePath = filePath,
                initialEditorText = editorText,
                bypassRegisterBackoff = true,
                replaceCached = true,
                expectedEditorTextAtSwap = editorText,
            )
            if (replacement == null || replacement === staleForwarder) {
                scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-reregister-failed")
                return RemoteTextApplyDisposition.RetryFailClosed
            }
            shadows[filePath] = editorText
            clearTemplateGuardRecoveryBackoff(filePath)
            log.warn(
                "[crdt-replica] recovered rejected remote canonical for ${File(filePath).name}; " +
                    "remote_state=$remoteState editor_chars=${editorText.length} " +
                    "remote_hash=${contentHash(remoteText)} editor_hash=${contentHash(editorText)} " +
                    "recovery=exact_editor_adopt_then_atomic_reregister",
            )
            requestRemoteDrain(filePath, "template-guard-recovered")
            return RemoteTextApplyDisposition.Recovered
        } finally {
            templateGuardRecoveryPaths.remove(filePath)
        }
    }

    private fun adoptExactEditorBaseline(
        filePath: String,
        editorText: String,
        staleForwarder: CrdtReplicaForwarder,
        allowPendingLocal: Boolean,
        reason: String,
    ): Boolean {
        if (templateStructureState(filePath, editorText, "editor-adopt") != TemplateStructureProjectionState.Exact) {
            scheduleTemplateGuardRecoveryRetry(filePath, "$reason-editor-structure-not-exact")
            return false
        }
        if (
            forwarders[filePath] !== staleForwarder ||
            (!allowPendingLocal && hasPendingLocal(filePath)) ||
            editorBufferText(filePath) != editorText
        ) {
            scheduleTemplateGuardRecoveryRetry(filePath, "$reason-adopt-fence-raced")
            return false
        }
        if (!staleForwarder.pushTextAdopt(editorText)) {
            scheduleTemplateGuardRecoveryRetry(filePath, "$reason-adopt-push-failed")
            return false
        }
        if (
            forwarders[filePath] !== staleForwarder ||
            (!allowPendingLocal && hasPendingLocal(filePath)) ||
            editorBufferText(filePath) != editorText
        ) {
            scheduleTemplateGuardRecoveryRetry(filePath, "$reason-editor-advanced")
            return false
        }
        val replacement = forwarderFor(
            filePath = filePath,
            initialEditorText = editorText,
            bypassRegisterBackoff = true,
            replaceCached = true,
            expectedEditorTextAtSwap = editorText,
            allowPendingLocalAtSwap = allowPendingLocal,
        )
        if (replacement == null || replacement === staleForwarder) {
            scheduleTemplateGuardRecoveryRetry(filePath, "$reason-reregister-failed")
            return false
        }
        shadows[filePath] = editorText
        clearTemplateGuardRecoveryBackoff(filePath)
        log.warn(
            "[crdt-replica] adopted exact live editor baseline for ${File(filePath).name}; " +
                "reason=$reason editor_hash=${contentHash(editorText)} " +
                "recovery=exact_editor_adopt_then_atomic_reregister",
        )
        requestRemoteDrain(filePath, "editor-baseline-adopted")
        return true
    }

    private fun scheduleTemplateGuardRecoveryRetry(filePath: String, reason: String) {
        if (!templateGuardRecoveryRetryPaths.add(filePath)) return
        val failureCount = templateGuardRecoveryFailureCounts.merge(filePath, 1) { current, one ->
            current + one
        } ?: 1
        val shifted = 1L shl minOf(failureCount - 1, 12)
        val delayMs = minOf(
            CRDT_DRAIN_NOOP_RESCHEDULE_BASE_BACKOFF_MS * shifted,
            CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS,
        )
        log.debug(
            "[crdt-replica] template-guard recovery retry scheduled for ${File(filePath).name}; " +
                "reason=$reason delay_ms=$delayMs failures=$failureCount",
        )
        executor.schedule(
            {
                templateGuardRecoveryRetryPaths.remove(filePath)
                if (!disposed.get()) requestRemoteDrain(filePath, "template-guard-retry")
            },
            delayMs,
            TimeUnit.MILLISECONDS,
        )
    }

    private fun clearTemplateGuardRecoveryBackoff(filePath: String) {
        templateGuardRecoveryFailureCounts.remove(filePath)
        templateGuardRecoveryRetryPaths.remove(filePath)
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
            if (!refreshCleanDocumentBeforeRemoteApply(pending.filePath, targetFile, document)) {
                return completeRemoteEditorApply(
                    pending,
                    RemoteEditorApplyOutcome(false, document.text),
                    started,
                )
            }
            val before = document.text
            if (before == pending.targetText) {
                shadows[pending.filePath] = pending.targetText
                RemoteEditorApplyOutcome(
                    persistRemoteCrdtTextIfSafe(
                        pending.filePath,
                        document,
                        pending.expectedText,
                        pending.targetText,
                    ),
                    before,
                )
            } else if (hasPendingLocal(pending.filePath)) {
                RemoteEditorApplyOutcome(false, before)
            } else if (!remoteCrdtApplyStillCurrentUtil(pending.expectedText, before, pending.targetText)) {
                log.warn("[crdt-replica] stale coalesced remote update rejected for ${pending.filePath}; editor text advanced before apply")
                RemoteEditorApplyOutcome(false, before)
            } else if (!remoteCrdtDiskCanPersistUtil(
                    pending.expectedText,
                    pending.targetText,
                    readRawDiskText(pending.filePath),
                )
            ) {
                log.warn(
                    "[crdt-replica] coalesced remote update rejected because disk contains novel external text for ${pending.filePath}; " +
                        "expected_hash=${contentHash(pending.expectedText)} target_hash=${contentHash(pending.targetText)}"
                )
                RemoteEditorApplyOutcome(false, before)
            } else {
                advanceNonOperatorMutationEpoch(pending.filePath)
                applyingRemote.add(pending.filePath)
                try {
                    runUndoableRemoteUpdateCommand(document) {
                        applyMinimalDocumentEditUtil(document, before, pending.targetText)
                        shadows[pending.filePath] = pending.targetText
                    }
                    RemoteEditorApplyOutcome(
                        persistRemoteCrdtTextIfSafe(
                            pending.filePath,
                            document,
                            pending.expectedText,
                            pending.targetText,
                        ),
                        pending.targetText,
                    )
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

    private fun readRawDiskText(filePath: String): String? =
        try {
            File(filePath).readText()
        } catch (e: Exception) {
            log.warn("[crdt-replica] raw disk read failed for $filePath: ${e.message}")
            null
        }

    /**
     * Synchronize the target file's VFS stamp before a remote CRDT edit is
     * installed and saved. IntelliJ resolves save conflicts by modification
     * stamp, not by comparing the bytes that [readRawDiskText] validated. An
     * external agent-doc write can therefore leave a clean Document with a
     * stale VirtualFile stamp; editing first and saving second would arm the
     * File Cache Conflict dialog even when disk still equals [expectedText].
     *
     * Refreshing an unsaved Document is the inverse hazard: it immediately
     * asks IntelliJ to choose between operator memory and disk. Fail closed in
     * that case and let the exact editor baseline flow through the CRDT retry.
     */
    private fun refreshCleanDocumentBeforeRemoteApply(
        filePath: String,
        targetFile: VirtualFile,
        document: Document,
    ): Boolean {
        val fileDocumentManager = FileDocumentManager.getInstance()
        if (!shouldRefreshVfsBeforeApplyUtil(fileDocumentManager.isDocumentUnsaved(document))) {
            log.debug("[crdt-replica] remote apply deferred before VFS refresh because the editor is unsaved for $filePath")
            return false
        }
        targetFile.refresh(false, false)
        if (fileDocumentManager.isDocumentUnsaved(document)) {
            log.warn("[crdt-replica] remote apply deferred because the editor became unsaved during the clean VFS refresh for $filePath")
            return false
        }
        return true
    }

    private fun persistRemoteCrdtTextIfSafe(
        filePath: String,
        document: Document,
        expectedText: String,
        targetText: String,
    ): Boolean {
        val diskText = readRawDiskText(filePath)
        if (!remoteCrdtDiskCanPersistUtil(expectedText, targetText, diskText)) {
            return false
        }
        return try {
            val fileDocumentManager = FileDocumentManager.getInstance()
            fileDocumentManager.saveDocument(document)
            val saved = !fileDocumentManager.isDocumentUnsaved(document) && document.text == targetText
            if (!saved) {
                log.warn("[crdt-replica] remote editor apply did not reach a clean saved state for $filePath")
            }
            saved
        } catch (e: RuntimeException) {
            log.warn("[crdt-replica] remote editor apply save failed for $filePath", e)
            false
        }
    }

    private fun completeRemoteEditorApply(
        pending: PendingRemoteEditorApply,
        outcome: RemoteEditorApplyOutcome,
        started: Long,
    ) {
        val projectionVisible = shouldAcknowledgeVisibleRemoteDeliveryUtil(
            outcome.editorText,
            pending.targetText,
            outcome.diskPersisted,
        )
        logSlow(
            "remote-apply-edt",
            pending.filePath,
            started,
            warnMs = CRDT_EDT_WARN_MS,
            details = "target_chars=${pending.targetText.length} visible=$projectionVisible disk_persisted=${outcome.diskPersisted} coalesced_updates=${pending.acknowledgements.size}",
        )
        if (disposed.get()) return
        if (projectionVisible) {
            // Retain before crossing back to the worker. If executor submission or
            // the socket ACK fails, the next drain replays it idempotently.
            rememberPendingRemoteAcks(pending.filePath, pending.acknowledgements)
        }
        try {
            executor.execute {
                try {
                    var acked = 0
                    if (projectionVisible) {
                        val activeForwarder = forwarders[pending.filePath]
                        if (activeForwarder != null) {
                            acked = replayPendingRemoteAcks(
                            pending.filePath,
                            activeForwarder,
                                outcome.editorText,
                            )
                        }
                    }
                    log.debug(
                        "[crdt-replica] remote editor apply completed for ${File(pending.filePath).name}; " +
                            "visible=$projectionVisible disk_persisted=${outcome.diskPersisted} acked=$acked coalesced_updates=${pending.acknowledgements.size}",
                    )
                } finally {
                    remoteEditorApplyPaths.remove(pending.filePath)
                    if (projectionVisible) {
                        consecutiveNoOpReschedules.set(0)
                        requestRemoteDrain(pending.filePath, "remote-editor-apply-complete")
                    } else {
                        val delayMs = nextNoOpRescheduleBackoffMs()
                        log.debug(
                            "[crdt-replica] remote editor projection not yet visible for ${File(pending.filePath).name}; " +
                                "backing off retry by ${delayMs}ms",
                        )
                        scheduleRemoteDrainAfterBackoff(delayMs, "remote-editor-apply-raced")
                    }
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
        val editorState = templateStructureState(filePath, editorText, "editor-baseline")
        val decision = replicaBaselineDecisionUtil(
            editorState = editorState,
            editorMatchesExpected = editorText == expectedText,
            replicaMatchesExpected = replicaText == expectedText,
            replicaMatchesEditor = replicaText == editorText,
            recoveryInFlight = templateGuardRecoveryPaths.contains(filePath),
        )
        if (decision == ReplicaBaselineDecision.ApplyRemote) return true
        val editorHash = contentHash(editorText)
        val expectedHash = contentHash(expectedText)
        val replicaHash = replicaText?.let(::contentHash) ?: "missing"
        if (decision == ReplicaBaselineDecision.RealignShadow) {
            log.warn(
                "[crdt-replica] incoming update deferred after shadow realignment for $filePath: " +
                    "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash"
            )
            shadows[filePath] = editorText
            requestRemoteDrain(filePath, "shadow-realigned")
            return false
        }
        if (decision == ReplicaBaselineDecision.AdoptExactEditor) {
            log.warn(
                "[crdt-replica] incoming update deferred while the exact editor baseline replaces a stale native replica for $filePath: " +
                    "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash",
            )
            adoptExactEditorBaseline(
                filePath = filePath,
                editorText = editorText,
                staleForwarder = forwarder,
                allowPendingLocal = false,
                reason = "remote-delivery-baseline-diverged",
            )
            return false
        }
        log.warn(
            "[crdt-replica] incoming update deferred because editor adoption lacks a stable exact proof for $filePath: " +
                "editor_state=$editorState editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$replicaHash",
        )
        scheduleTemplateGuardRecoveryRetry(filePath, "editor-baseline-proof-missing")
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

    private fun templateStructureState(
        filePath: String,
        text: String,
        source: String,
    ): TemplateStructureProjectionState {
        val started = System.nanoTime()
        return try {
            val normalized = NativePatching.normalizeTemplateStructure(text)
            templateStructureProjectionStateUtil(text, normalized).also { state ->
                if (state != TemplateStructureProjectionState.Exact) {
                    log.warn(
                        "[crdt-replica] $source text rejected by template-structure guard for $filePath; " +
                            "state=$state",
                    )
                }
            }
        } finally {
            logSlow("template-normalize-worker", filePath, started, details = "source=$source target_chars=${text.length}")
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
        bypassRegisterBackoff: Boolean = false,
        replaceCached: Boolean = bypassRegisterBackoff,
        expectedEditorTextAtSwap: String? = null,
        allowPendingLocalAtSwap: Boolean = false,
    ): CrdtReplicaForwarder? {
        val cached = forwarders[filePath]
        if (bypassRegisterBackoff) {
            // A refresh is register -> swap -> retire. Never create an authority
            // gap by deregistering the working member before its replacement has
            // accepted the canonical bootstrap.
            clearRegisterFailure(filePath)
        } else if (!replaceCached) {
            cached?.let { return it }
        }
        if (!bypassRegisterBackoff && !shouldAttemptRegister(filePath)) return cached
        val root = resolveProjectRoot(filePath) ?: return null
        val baseIdentity = "${EditorIdentity.id}:$filePath"
        val identity = if (replaceCached && cached != null) {
            "$baseIdentity:refresh-${refreshConnectionEpoch.incrementAndGet()}"
        } else {
            baseIdentity
        }
        val forwarder = CrdtReplicaForwarder(
            filePath = filePath,
            identity = identity,
            node = NativeReplicaNode(),
            transport = CpcSocketReplicaTransport(root),
        )
        if (!forwarder.register()) {
            recordRegisterFailure(filePath)
            if (cached != null) {
                log.warn("[crdt-replica] replacement register failed for ${File(filePath).name}; retained cached forwarder")
            }
            return null
        }
        clearRegisterFailure(filePath)
        if (initialEditorText != null) {
            forwarder.ensureEditorText(initialEditorText)
        }
        if (
            expectedEditorTextAtSwap != null &&
            (editorBufferText(filePath) != expectedEditorTextAtSwap ||
                (!allowPendingLocalAtSwap && hasPendingLocal(filePath)))
        ) {
            // Registration and native bootstrap can block. Fence both first
            // registration and replacement registration at the actual swap so
            // neither can publish a snapshot older than the visible editor.
            forwarder.deregister()
            return null
        }
        if (replaceCached && cached != null) {
            if (forwarders.replace(filePath, cached, forwarder)) {
                // The replacement is now authoritative. Retire the prior
                // member's retained ACK frontier only after the successful
                // swap so a failed registration keeps the working lineage.
                val retiredPendingAcks = clearPendingRemoteAcks(filePath)
                cached.deregister()
                log.info(
                    "[crdt-replica] atomically replaced cached forwarder for ${File(filePath).name}; " +
                        "retired_pending_acks=$retiredPendingAcks",
                )
                return forwarder
            }
            // The manager worker is serialized, but preserve a concurrently
            // installed winner without sending a false document-close event.
            forwarder.deregister()
            return forwarders[filePath]
        }
        val existing = forwarders.putIfAbsent(filePath, forwarder)
        if (existing != null) {
            forwarder.deregister()
            return existing
        }
        log.info("[crdt-replica] attached ${File(filePath).name} as $identity")
        return forwarder
    }

    private fun refreshReplicaAfterTransportLoss(
        filePath: String,
        staleForwarder: CrdtReplicaForwarder,
        editorText: String,
        reason: String,
    ) {
        if (forwarders[filePath] !== staleForwarder) return
        val replacement = forwarderFor(
            filePath = filePath,
            initialEditorText = editorText,
            bypassRegisterBackoff = false,
            replaceCached = true,
            expectedEditorTextAtSwap = editorText,
        )
        if (replacement != null && replacement !== staleForwarder) {
            log.info(
                "[crdt-replica] controller transport recovered for ${File(filePath).name}; " +
                    "reason=$reason",
            )
            requestRemoteDrain(filePath, "controller-transport-reregistered")
        } else {
            log.debug(
                "[crdt-replica] controller transport unavailable for ${File(filePath).name}; " +
                    "reason=$reason",
            )
        }
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
        private val nonOperatorMutationEpochs = ConcurrentHashMap<String, AtomicLong>()

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

        fun requestUrgentRemoteDrain(project: Project, filePath: String, reason: String) {
            instances[project]?.requestUrgentRemoteDrain(filePath, reason)
        }

        fun requestTextAdopt(project: Project, filePath: String) {
            instances[project]?.requestTextAdopt(filePath)
        }

        fun forceRefreshOpenDocumentReplicas(project: Project, reason: String) {
            val manager = instances[project] ?: return
            val openDocuments = ApplicationManager.getApplication().runReadAction<List<Triple<String, String, Document>>> {
                val fileDocumentManager = FileDocumentManager.getInstance()
                FileEditorManager.getInstance(project).openFiles
                    .asSequence()
                    .filter { it.name.endsWith(".md") }
                    .mapNotNull { file ->
                        fileDocumentManager.getDocument(file)?.let { document ->
                            Triple(file.path, file.name, document)
                        }
                    }
                    .toList()
            }
            openDocuments.forEach { (filePath, fileName, document) ->
                manager.log.info("[crdt-replica] forcing open-document re-register for $fileName reason=$reason")
                manager.ensureOpenDocumentReplica(
                    filePath,
                    document,
                    await = false,
                    forceRefresh = true,
                )
            }
        }

        fun <T> withAgentAppliedEditorMutation(filePath: String, block: () -> T): T {
            advanceNonOperatorMutationEpoch(filePath)
            applyingAgentMutations.add(filePath)
            return try {
                block()
            } finally {
                applyingAgentMutations.remove(filePath)
            }
        }

        fun forceRefreshOpenDocumentReplica(project: Project, filePath: String, reason: String) {
            val manager = instances[project] ?: return
            val openDocument = ApplicationManager.getApplication().runReadAction<Triple<String, String, Document>?> {
                val file = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@runReadAction null
                val document = FileDocumentManager.getInstance().getDocument(file) ?: return@runReadAction null
                Triple(file.path, file.name, document)
            } ?: return
            val (resolvedFilePath, fileName, document) = openDocument
            manager.log.info(
                "[crdt-replica] forcing delivery-ack re-register for $fileName reason=$reason",
            )
            manager.ensureOpenDocumentReplica(
                resolvedFilePath,
                document,
                await = false,
                forceRefresh = true,
            )
        }

        fun ensureReplicaForOpenDocument(
            filePath: String,
            document: Document,
            editorText: String? = null,
            await: Boolean = false,
            forceRefresh: Boolean = false,
        ): Boolean {
            val manager = managerForFilePath(filePath)
                ?: return false
            return manager.ensureOpenDocumentReplica(filePath, document, editorText, await, forceRefresh)
        }

        fun publishClosingDocumentCut(filePath: String, document: Document): Boolean {
            val manager = managerForFilePath(filePath) ?: return false
            return manager.publishClosingDocumentCut(filePath, document)
        }

        fun isApplyingRemote(filePath: String): Boolean =
            instances.values.any { it.applyingRemote.contains(filePath) }

        fun isApplyingNonOperatorMutation(filePath: String): Boolean =
            applyingAgentMutations.contains(filePath) || isApplyingRemote(filePath)

        fun isOperatorDocumentEvent(filePath: String, event: DocumentEvent): Boolean =
            isOperatorDocumentEventUtil(
                nonOperatorMutation = isApplyingNonOperatorMutation(filePath),
                wholeTextReplaced = event.isWholeTextReplaced,
            )

        private fun managerForFilePath(filePath: String): CrdtReplicaManager? =
            instances.values
                .filter { it.ownsFilePath(filePath) }
                .maxWithOrNull(
                    compareBy<CrdtReplicaManager> { it.project.basePath?.length ?: 0 }
                        .thenBy { it.project.basePath.orEmpty() },
                )

        private fun nonOperatorMutationEpoch(filePath: String): Long =
            nonOperatorMutationEpochs[filePath]?.get() ?: 0L

        private fun advanceNonOperatorMutationEpoch(filePath: String): Long {
            val epoch = nonOperatorMutationEpochs
                .computeIfAbsent(filePath) { AtomicLong(0L) }
                .incrementAndGet()
        AgentDocLib.get()?.agent_doc_clear_editor_op_epoch(filePath)
            return epoch
        }
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

internal fun isOperatorDocumentEventUtil(
    nonOperatorMutation: Boolean,
    wholeTextReplaced: Boolean,
): Boolean = !nonOperatorMutation && !wholeTextReplaced

internal fun remoteCrdtApplyStillCurrentUtil(
    expectedText: String,
    currentText: String,
    targetText: String,
): Boolean =
    currentText == expectedText || currentText == targetText

internal fun remoteCrdtDiskCanPersistUtil(
    expectedText: String,
    targetText: String,
    diskText: String?,
): Boolean = diskText == expectedText || diskText == targetText

internal fun remoteCrdtReplaceStillCurrentUtil(
    expectedText: String,
    currentText: String,
    replicaText: String?,
): Boolean =
    currentText == expectedText && replicaText == expectedText
