package com.github.btakita.agentdoc

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.application.ex.ApplicationEx
import com.intellij.openapi.command.CommandProcessor
import com.intellij.openapi.command.UndoConfirmationPolicy
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileDocumentManagerListener
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileContentChangeEvent
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import io.github.lazily.IngressOutcome
import io.github.lazily.MergePolicy
import io.github.lazily.ThreadSafeContext
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import javax.swing.SwingUtilities

private const val CRDT_LISTENER_WARN_MS = 10L
private const val CRDT_WORKER_WARN_MS = 100L

/**
 * GH #65: off-EDT CRDT work (replica workers, native calls, socket transport) routinely
 * takes 100-400 ms and blocks nothing the user sees. Warning at the per-call 50/100 ms
 * thresholds put every keystroke batch into `idea.log` at WARN (13% of the log, forcing
 * rotation). Off the EDT only a genuine stall warns; the full per-delta timing stays at
 * DEBUG (`Help > Diagnostic Tools > Debug Log Settings` → `#com.github.btakita.agentdoc`).
 */
internal const val CRDT_OFF_EDT_WARN_FLOOR_MS = 1_000L

/**
 * Whether a `[crdt-perf]` timing line is a warning. On the EDT the caller's threshold
 * stands, because any UI-thread stall is user-visible; off the EDT it is raised to
 * [CRDT_OFF_EDT_WARN_FLOOR_MS].
 */
internal fun crdtPerfWarns(elapsedMs: Long, warnMs: Long, onEdt: Boolean): Boolean =
    elapsedMs >= if (onEdt) warnMs else maxOf(warnMs, CRDT_OFF_EDT_WARN_FLOOR_MS)
private const val NATIVE_RELOAD_WORKER_TIMEOUT_MS = 5_000L

internal fun nativeReloadRemainingWaitMillis(deadlineNanos: Long, nowNanos: Long): Long? {
    val remainingNanos = deadlineNanos - nowNanos
    if (remainingNanos <= 0L) return null
    return TimeUnit.NANOSECONDS.toMillis(remainingNanos).coerceAtLeast(1L)
}

internal data class NativeReloadReplicaRestartReport(
    val expected: Int,
    val attached: Int,
    val failedPaths: List<String>,
    val liveProjects: Int,
) {
    val converged: Boolean
        get() = expected > 0 && expected == attached && failedPaths.isEmpty()
}

internal data class NativeReloadReplicaHandoff(
    val projectDocuments: Map<Project, Set<String>>,
    val reloadSafe: Boolean,
    /**
     * `#steerreplicachurn`: whether the quiesce disposed the replica managers.
     * A capture that missed its deadline returns before disposing anything; the
     * live replicas must then be left alone, not re-registered.
     */
    val replicasTornDown: Boolean = true,
)

/**
 * `#steerreplicachurn`: whether a native-reload attempt must rebuild the CRDT
 * replicas. Only a quiesce that actually disposed the managers requires it; a
 * quiesce that threw is treated as having disposed them, the conservative case.
 * Rebuilding replicas that were never torn down force-refreshed every open
 * document's registration on each failed-closed reload attempt.
 */
internal fun nativeReloadReplicaRestartRequiredUtil(
    replicaQuiesceAttempted: Boolean,
    handoff: NativeReloadReplicaHandoff?,
): Boolean = replicaQuiesceAttempted && (handoff?.replicasTornDown ?: true)

internal fun nativeReloadReplicaRestartReport(
    expectedPaths: Collection<String>,
    attachedPaths: Collection<String>,
    liveProjects: Int = 0,
): NativeReloadReplicaRestartReport {
    val expected = expectedPaths.toSortedSet()
    val attached = attachedPaths.toSet()
    val failed = expected.filterNot(attached::contains)
    return NativeReloadReplicaRestartReport(
        expected = expected.size,
        attached = expected.count(attached::contains),
        failedPaths = failed,
        liveProjects = liveProjects,
    )
}

/**
 * Merge per-project replica restart reports into one whole-IDE report.
 *
 * A dynamic plugin load reattaches every open project at once, so the receipt the
 * package installer reads has to describe the IDE, not one project.
 */
internal fun mergeReplicaRestartReports(
    reports: List<NativeReloadReplicaRestartReport>,
): NativeReloadReplicaRestartReport =
    NativeReloadReplicaRestartReport(
        expected = reports.sumOf { it.expected },
        attached = reports.sumOf { it.attached },
        failedPaths = reports.flatMap { it.failedPaths },
        liveProjects = reports.sumOf { it.liveProjects },
    )

/**
 * `#jbupgradereattach`: one-line, classloader-safe receipt for the package installer.
 *
 * This is a receipt, never a verdict. The upgrade verdict is already decided by the
 * time it is produced -- the replacement bytes are installed and a fresh descriptor of
 * the expected version is live. Pending paths stay last in the text so a path
 * containing `:` cannot be mistaken for another field.
 */
internal fun dynamicLoadReattachReceipt(report: NativeReloadReplicaRestartReport): String {
    val documents = "documents=${report.attached}/${report.expected}"
    if (report.expected == 0) {
        return "$documents:state=no-open-documents:live_projects=${report.liveProjects}"
    }
    if (report.failedPaths.isEmpty()) return documents
    val pending = report.failedPaths.joinToString(",") { path ->
        path.replace('\n', ' ').replace('\r', ' ')
    }
    return "$documents:pending=$pending"
}
private const val CRDT_EDT_WARN_MS = 50L
private const val CRDT_AWAIT_ATTACH_TIMEOUT_MS = 750L
private const val EDITOR_CAPTURE_CUT_ATTEMPTS = 20
private const val EDITOR_CAPTURE_CUT_RETRY_MS = 5L
private const val UNFORWARDED_OPERATOR_TEXT_CONFIRM_MS = 1_000L
private const val DYNAMIC_PLUGIN_ATTACH_RECEIPT_TIMEOUT_MS = 15_000L
private const val CRDT_AWAIT_CLOSE_PUBLISH_TIMEOUT_MS = 2_000L
private const val CRDT_AWAIT_PERSIST_CURRENT_TIMEOUT_MS = 5_000L
private const val CRDT_REGISTER_FAILURE_BASE_BACKOFF_MS = 1_000L
private const val CRDT_REGISTER_FAILURE_MAX_BACKOFF_MS = 30_000L
// A human typing burst should become one ordered durable CRDT publication,
// rather than paying push + broadcast + projection round trips per character.
internal const val LOCAL_EDITOR_FLUSH_QUIET_MS = 250L
private const val LOCAL_EDITOR_RETRY_BASE_MS = 250L
private const val LOCAL_EDITOR_RETRY_MAX_MS = 30_000L
private const val CRDT_DRAIN_NOOP_RESCHEDULE_BASE_BACKOFF_MS = 100L

/**
 * `#ctrlkillreregister` Tier 3: minimum gap between whole-editor missing-replica
 * pulls. A controller kill makes every open document report transport loss at once,
 * and one pull already answers for all of them.
 */
private const val PEER_REPLICA_PULL_MIN_INTERVAL_MS = 5_000L
// `#crdt-drain-idle-quiet`: back off work that arrived while a no-op drain was
// already running. The resume callback must consume that retained work without
// manufacturing another drain-all request; doing so turns one overlap into a
// permanent workspace-wide socket polling loop.
private const val CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS = 30_000L
private const val CRDT_IDLE_WORKER_WARN_MS = 1_000L
private const val CRDT_DRAIN_BACKOFF_REASON = "backoff-resume"
private const val PROJECTION_RECOVERY_REREGISTER_MIN_INTERVAL_MS = 5_000L
// Delivery routability is a component-level fact, not merely an IDE-process
// fact. Refresh from the same serialized executor that pulls/applies/ACKs CRDT
// deliveries: if that worker stalls, this heartbeat stalls too and Rust stops
// targeting it while continuing to protect its possibly-unsaved buffer.

private data class RemoteEditorEffectToken(
    val generation: Long,
    val endpoint: CrdtReplicaForwarder,
)

internal fun remoteEditorEffectTokenCurrentUtil(
    tokenGeneration: Long,
    liveGeneration: Long?,
    endpointMatches: Boolean,
    endpointBacked: Boolean,
): Boolean =
    liveGeneration == tokenGeneration &&
        endpointMatches &&
        endpointBacked

/** A retained delivery frontier owns the retry cadence. File-watcher and editor
 * events may add work while backoff is active, but must not bypass that gate
 * and hammer the controller with the same unresolved projection. */
internal fun shouldStartRemoteDrainUtil(backoffScheduled: Boolean): Boolean = !backoffScheduled

/**
 * `#crdtpushdrain`: a controller-published CRDT remote event is positive evidence
 * that the CP already holds a frontier for this document, so it must bypass the
 * speculative no-op drain backoff instead of being suppressed by it.
 *
 * The no-op backoff (see [scheduleRemoteDrainAfterBackoff]) exists to stop a
 * *self-driven* drain spin when there is nothing to pull; on an idle document it
 * climbs to [CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS]. That is exactly the state a
 * document sits in when the operator triggers Compact Exchange, so every
 * controller projection event drains eagerly. The controller's keyed retained
 * Lazily projection provides the coalescing boundary.
 */
internal fun shouldUrgentDrainForRemoteEventUtil(@Suppress("UNUSED_PARAMETER") reasonToken: String?): Boolean = true

/** A registration refused because the owning controller names another generation's endpoint. */
internal fun registerFailureNeedsLivenessRepublishUtil(reason: String?): Boolean =
    reason?.contains("replica_register_stale_editor_endpoint") == true

internal const val STALE_ENDPOINT_REGISTER_RETRY_MS = 1_000L

/**
 * #jbrejectlog: name why a `deliver_crdt_remote` event is refused before any
 * replica work runs, or null when it is admitted. The receipt the controller
 * sees is a bare `rejected`, so the plugin log is the only place this can be read.
 */
internal fun crdtRemoteAdmissionRejectReasonUtil(
    file: String?,
    editorId: String?,
    thisEditorId: String,
): String? = when {
    file == null -> "missing_file"
    editorId == null -> "missing_editor_id"
    editorId != thisEditorId -> "editor_id_mismatch"
    else -> null
}

/**
 * #jbrejectlog: name which piece of editor state a recovery re-register could
 * not capture, or null when everything it needs is present.
 */
internal fun replicaRecoveryCaptureMissReasonUtil(
    projectDisposed: Boolean,
    managerPresent: Boolean,
    filePresent: Boolean,
    documentPresent: Boolean,
): String? = when {
    projectDisposed -> "project_disposed"
    !managerPresent -> "no_replica_manager"
    !filePresent -> "file_not_found"
    !documentPresent -> "no_document"
    else -> null
}

/**
 * #jbmanagerbyfile: which replica manager serves a file-scoped request. The manager
 * already holding the file's replica wins; then [preferred] (the requesting project's
 * own manager); otherwise null, and the caller falls back to project ownership.
 */
internal fun <M> selectReplicaManagerUtil(
    candidates: List<M>,
    preferred: M?,
    holdsReplica: (M) -> Boolean,
): M? {
    if (preferred != null && holdsReplica(preferred)) return preferred
    return candidates.firstOrNull(holdsReplica) ?: preferred
}

/** Only the controller's typed missing-membership recovery may republish editor state. */
internal fun shouldReregisterForRemoteEventUtil(reasonToken: String?): Boolean =
    reasonToken == "editor_replica_reregister"

internal fun projectionRecoveryReregisterDueUtil(
    lastStartedMs: Long?,
    nowMs: Long,
    minIntervalMs: Long = PROJECTION_RECOVERY_REREGISTER_MIN_INTERVAL_MS,
): Boolean =
    lastStartedMs == null ||
        nowMs < lastStartedMs ||
        nowMs - lastStartedMs >= minIntervalMs

@Suppress("UNUSED_PARAMETER")
internal fun shouldProjectVisibleRemoteDeliveryUtil(
    editorText: String?,
    targetText: String,
    diskPersisted: Boolean,
): Boolean = editorText == targetText

internal enum class RemoteCrdtProjectionMode {
    Reject,
    MemoryOnly,
    Persist,
}

internal enum class RemotePersistReconciliation {
    Persisted,
    PersistedEditorNormalization,
    RollbackToBefore,
    PreserveAdvancedEditor,
}

/**
 * A failed save is transactional only while both observable planes still prove
 * the attempted mutation. Roll back when the editor remains at the remote
 * target and disk remains at the exact pre-apply text; otherwise preserve the
 * advanced plane and fail closed.
 */
internal fun remotePersistReconciliationUtil(
    beforeText: String,
    targetText: String,
    editorAfterSave: String,
    diskAfterSave: String?,
): RemotePersistReconciliation = when {
    editorAfterSave == targetText && diskAfterSave == targetText ->
        RemotePersistReconciliation.Persisted
    editorAfterSave == targetText && diskAfterSave == beforeText ->
        RemotePersistReconciliation.RollbackToBefore
    diskAfterSave != null && editorAfterSave == diskAfterSave ->
        RemotePersistReconciliation.PersistedEditorNormalization
    else -> RemotePersistReconciliation.PreserveAdvancedEditor
}

internal data class ReplicaRegistrationRetryProjection(
    val failureCount: Int,
    val retryAfterMs: Long,
    val backoffMs: Long,
)

internal fun nextReplicaRegistrationRetryProjection(
    previous: ReplicaRegistrationRetryProjection?,
    nowMs: Long,
    baseBackoffMs: Long = CRDT_REGISTER_FAILURE_BASE_BACKOFF_MS,
    maxBackoffMs: Long = CRDT_REGISTER_FAILURE_MAX_BACKOFF_MS,
): ReplicaRegistrationRetryProjection {
    val failureCount = ((previous?.failureCount ?: 0) + 1).coerceAtMost(16)
    val step = (failureCount - 1).coerceAtLeast(0).coerceAtMost(15)
    val multiplier = 1L shl step
    val backoffMs =
        if (baseBackoffMs >= maxBackoffMs || baseBackoffMs > Long.MAX_VALUE / multiplier) {
            maxBackoffMs
        } else {
            (baseBackoffMs * multiplier).coerceAtMost(maxBackoffMs)
        }
    return ReplicaRegistrationRetryProjection(
        failureCount = failureCount,
        retryAfterMs = nowMs + backoffMs,
        backoffMs = backoffMs,
    )
}

internal fun replicaRegistrationAttemptDueUtil(
    projection: ReplicaRegistrationRetryProjection?,
    nowMs: Long,
): Boolean = projection == null || nowMs >= projection.retryAfterMs

private data class RemotePersistOutcome(
    val diskPersisted: Boolean,
    val editorTextForProjection: String?,
    val editorNormalizedText: String? = null,
)

/**
 * The CRDT canonical is the durable effect sink. An unsaved editor can safely
 * accept a causally fenced remote delta in memory, but the plugin must not
 * refresh or save behind the operator. Clean editors still require exact disk
 * proof before projection and persistence.
 */
internal fun remoteCrdtProjectionModeUtil(
    documentUnsaved: Boolean,
    diskCanPersist: Boolean,
): RemoteCrdtProjectionMode = when {
    documentUnsaved -> RemoteCrdtProjectionMode.MemoryOnly
    diskCanPersist -> RemoteCrdtProjectionMode.Persist
    else -> RemoteCrdtProjectionMode.Reject
}

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

internal enum class RemoteTemplateProjectionDecision {
    QueueRemote,
    RecoverEditorBaseline,
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
    !recoveryInFlight &&
        editorState == TemplateStructureProjectionState.Exact &&
        editorMatchesExpected ->
        RemoteTemplateProjectionDecision.RecoverEditorBaseline
    else -> RemoteTemplateProjectionDecision.RetryFailClosed
}

internal enum class ReplicaBaselineDecision {
    ApplyRemote,
    ApplyRemoteRepair,
    ProjectRemoteTarget,
    RebootstrapVisibleRemoteTarget,
    ReplayRemoteTarget,
    RealignShadow,
    RetryFailClosed,
}

internal fun matchingRemoteTargetGenerationUtil(
    updates: List<ReplicaRemoteUpdate>,
    contentHash: String?,
): Long? =
    contentHash?.let { targetHash ->
        updates
            .asSequence()
            .filter { it.expectedContentHash == targetHash }
            .maxOfOrNull { it.generation }
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
    editorMatchesRemoteTarget: Boolean,
    replicaMatchesRemoteTarget: Boolean,
    recoveryInFlight: Boolean,
    canonicalProjectionRetained: Boolean = false,
): ReplicaBaselineDecision = when {
    recoveryInFlight ->
        ReplicaBaselineDecision.RetryFailClosed
    editorMatchesRemoteTarget && replicaMatchesEditor ->
        ReplicaBaselineDecision.ProjectRemoteTarget
    editorMatchesRemoteTarget ->
        ReplicaBaselineDecision.RebootstrapVisibleRemoteTarget
    canonicalProjectionRetained && replicaMatchesRemoteTarget ->
        ReplicaBaselineDecision.ReplayRemoteTarget
    canonicalProjectionRetained && replicaMatchesExpected ->
        ReplicaBaselineDecision.ApplyRemote
    canonicalProjectionRetained ->
        ReplicaBaselineDecision.RetryFailClosed
    editorMatchesExpected && replicaMatchesRemoteTarget ->
        ReplicaBaselineDecision.ReplayRemoteTarget
    editorState != TemplateStructureProjectionState.Exact && editorMatchesExpected ->
        ReplicaBaselineDecision.ApplyRemoteRepair
    editorState != TemplateStructureProjectionState.Exact ->
        ReplicaBaselineDecision.RetryFailClosed
    editorMatchesExpected && replicaMatchesExpected -> ReplicaBaselineDecision.ApplyRemote
    replicaMatchesEditor -> ReplicaBaselineDecision.RealignShadow
    else -> ReplicaBaselineDecision.RetryFailClosed
}

internal fun shouldForwardLocalDeltaUtil(replicaText: String?, shadowText: String): Boolean =
    replicaText == shadowText

internal enum class LocalReplicaBaselineDecision {
    ForwardLocal,
    RebootstrapCanonicalThenForward,
}

/**
 * `#steerreplicachurn`: a captured local delta rebased onto [canonicalText] may
 * be forwarded on the endpoint that is already attached exactly when that
 * endpoint's replica text IS that canonical generation and the operator has not
 * typed since the captured cut. Otherwise recovery re-registers from canonical.
 */
internal fun capturedRebaseCanReuseEndpointUtil(
    endpointReplicaText: String?,
    canonicalText: String,
    visibleEditorText: String?,
    capturedVisibleText: String,
): Boolean = endpointReplicaText == canonicalText && visibleEditorText == capturedVisibleText

internal fun localReplicaBaselineDecisionUtil(
    replicaText: String?,
    capturedBaseText: String,
): LocalReplicaBaselineDecision =
    if (shouldForwardLocalDeltaUtil(replicaText, capturedBaseText)) {
        LocalReplicaBaselineDecision.ForwardLocal
    } else {
        LocalReplicaBaselineDecision.RebootstrapCanonicalThenForward
    }

internal fun localEditorRetryDelayMsUtil(failureCount: Int): Long {
    val exponent = (failureCount.coerceAtLeast(1) - 1).coerceAtMost(12)
    return (LOCAL_EDITOR_RETRY_BASE_MS * (1L shl exponent))
        .coerceAtMost(LOCAL_EDITOR_RETRY_MAX_MS)
}

internal data class CapturedLocalEditorEdit(
    val offsetUtf16: Int,
    val oldFragment: String,
    val newFragment: String,
    val projectionEpoch: Long,
)

/**
 * Reconstruct the exact editor cut that preceded the first observed local
 * splice. JetBrains delivers [DocumentEvent] after mutating the document, so a
 * newly-opened session document can receive operator input before its CRDT
 * shadow/registration exists. Treating that post-edit buffer as the bootstrap
 * loses the splice and lets the retained controller text overwrite it.
 */
internal fun reconstructLocalEditorBaseTextUtil(
    after: String,
    edit: CapturedLocalEditorEdit,
): String? {
    val start = edit.offsetUtf16
    val end = start + edit.newFragment.length
    if (start < 0 || start > after.length || end > after.length) return null
    if (after.substring(start, end) != edit.newFragment) return null
    return after.substring(0, start) + edit.oldFragment + after.substring(end)
}

internal data class PreparedLocalEditorEdit(
    val offsetCodePoints: Int,
    val deleteCodePoints: Int,
    val insert: String,
)

internal data class PreparedLocalEditorBatch(
    val edits: List<PreparedLocalEditorEdit>,
    val resultingText: String,
)

private enum class LocalEditorForwardResult {
    Applied,
    Fenced,
    Retry,
}

/**
 * Validate and translate the editor's causal splice stream against the retained
 * CRDT shadow. Ordinary editor events already carry the changed UTF-16 range;
 * never widen them into a whole-buffer diff.
 */
internal fun prepareLocalEditorEditsUtil(
    before: String,
    edits: List<CapturedLocalEditorEdit>,
): PreparedLocalEditorBatch? {
    val current = StringBuilder(before)
    var anchorUtf16 = before.length
    var anchorCodePoints = before.codePointCount(0, before.length)
    val prepared = ArrayList<PreparedLocalEditorEdit>(edits.size)
    for (edit in edits) {
        val start = edit.offsetUtf16
        val end = start + edit.oldFragment.length
        if (start < 0 || start > current.length || end > current.length) return null
        if (current.substring(start, end) != edit.oldFragment) return null
        val deleteCodePoints = edit.oldFragment.codePointCount(0, edit.oldFragment.length)
        val insertCodePoints = edit.newFragment.codePointCount(0, edit.newFragment.length)
        val offsetCodePoints =
            if (start >= anchorUtf16) {
                anchorCodePoints + Character.codePointCount(current, anchorUtf16, start)
            } else {
                anchorCodePoints - Character.codePointCount(current, start, anchorUtf16)
            }
        prepared.add(
            PreparedLocalEditorEdit(
                offsetCodePoints = offsetCodePoints,
                deleteCodePoints = deleteCodePoints,
                insert = edit.newFragment,
            ),
        )
        current.replace(start, end, edit.newFragment)
        anchorUtf16 = start + edit.newFragment.length
        anchorCodePoints = offsetCodePoints + insertCodePoints
    }
    return PreparedLocalEditorBatch(prepared, current.toString())
}

/**
 * The captured splice batches still owed to the replica: only those taken in the
 * current projection epoch. A whole-buffer publication (`PublishOperatorBuffer`,
 * `MergeForward`) advances the epoch because the published buffer already
 * contains every earlier splice (`#subsumedsplicereplay`).
 *
 * Live 2026-09-30 on infra.md: re-register published the whole buffer, typing
 * included, from its settled shadow; the same 18 keystrokes were still queued as
 * a splice batch and were forwarded again onto the re-registered replica. Pure
 * inserts carry no range text to mismatch, so the replay passed validation and
 * the operator's text landed twice.
 */
internal fun currentEpochCapturedEditsUtil(
    edits: List<CapturedLocalEditorEdit>,
    currentEpoch: Long,
): List<CapturedLocalEditorEdit> = edits.filter { it.projectionEpoch == currentEpoch }

/**
 * The captured splices a whole-buffer publication did NOT subsume.
 *
 * `retainedtargetdropsedit` (live 2026-10-01, a session document): a re-register
 * published the operator buffer `S` (shadow + one typed newline). While the
 * 322 KB publication delta was in flight the operator pasted a 99-byte queue
 * line. The fence then advanced the projection epoch, retiring EVERY captured
 * splice — the paste too, although `S` never contained it. The very next splice
 * (deleting the paste's trailing newline, offset 7482) was forwarded from the
 * shadow `S`; `S[7482]` also happened to be a newline (the blank line before
 * `## Review`), so its range check passed and the replica deleted the wrong
 * line. Canonical, replica and every later retained persistence target became
 * `468b48…` (no paste, one blank line missing) while the editor showed
 * `8849a8…`; the editor refused every exact-target save until a controller
 * restart.
 *
 * Answer exactly the suffix of [captured] typed after the [published] cut, by
 * undoing captured splices from the newest backwards from [visible] (an atomic
 * snapshot with [captured]) until the text equals [published]. Every kept splice
 * is re-stamped to [epoch]. When the walk cannot prove a cut, a single splice
 * `published -> visible` stands in for the raced edits; it is exact text, only
 * less granular. Never null: the operator's visible text is always owed.
 */
internal fun capturedEditsOwedAfterPublishedCutUtil(
    published: String,
    visible: String,
    captured: List<CapturedLocalEditorEdit>,
    epoch: Long,
): List<CapturedLocalEditorEdit> {
    var text = visible
    var undone = 0
    while (true) {
        if (text == published) {
            val owed = captured.takeLast(undone).map { it.copy(projectionEpoch = epoch) }
            if (owed.isEmpty() || prepareLocalEditorEditsUtil(published, owed)?.resultingText == visible) {
                return owed
            }
            break
        }
        if (undone == captured.size) break
        text = reconstructLocalEditorBaseTextUtil(text, captured[captured.size - 1 - undone]) ?: break
        undone++
    }
    return listOfNotNull(singleSpliceCapturedEditUtil(published, visible, epoch))
}

/**
 * One UTF-16 splice turning [before] into [after] (common prefix/suffix), never
 * splitting a surrogate pair; null when they are equal.
 */
internal fun singleSpliceCapturedEditUtil(
    before: String,
    after: String,
    epoch: Long,
): CapturedLocalEditorEdit? {
    if (before == after) return null
    var prefix = 0
    val maxPrefix = minOf(before.length, after.length)
    while (prefix < maxPrefix && before[prefix] == after[prefix]) prefix++
    if (prefix > 0 && Character.isHighSurrogate(before[prefix - 1])) prefix--
    var suffix = 0
    while (
        suffix < before.length - prefix &&
        suffix < after.length - prefix &&
        before[before.length - 1 - suffix] == after[after.length - 1 - suffix]
    ) {
        suffix++
    }
    if (suffix > 0 && Character.isLowSurrogate(before[before.length - suffix])) suffix--
    return CapturedLocalEditorEdit(
        offsetUtf16 = prefix,
        oldFragment = before.substring(prefix, before.length - suffix),
        newFragment = after.substring(prefix, after.length - suffix),
        projectionEpoch = epoch,
    )
}

/**
 * `retainedtargetdropsedit` self-heal: the replica is missing operator text when
 * the shadow is exactly the replica, no captured splice is outstanding, and the
 * visible buffer still differs from the shadow. That is a lost splice: nothing
 * else moves the visible buffer without either a captured splice or a remote
 * apply, which callers exclude. Answer the splice that rolls the visible text
 * forward onto the replica (editor text is the base), or null when the capture
 * chain is intact.
 */
internal fun unforwardedOperatorTextSpliceUtil(
    shadow: String?,
    replica: String?,
    visible: String?,
    captured: List<CapturedLocalEditorEdit>,
    epoch: Long,
): CapturedLocalEditorEdit? {
    if (shadow == null || replica == null || visible == null) return null
    if (shadow != replica || captured.isNotEmpty()) return null
    return singleSpliceCapturedEditUtil(shadow, visible, epoch)
}

internal fun pullDeliveryRequestsReplicaRefreshUtil(delivery: ReplicaPullDelivery): Boolean =
    delivery is ReplicaPullDelivery.Unavailable

private data class PendingRemoteEditorApply(
    val filePath: String,
    val expectedText: String,
    val targetText: String,
    val effectToken: RemoteEditorEffectToken,
)

private data class RemoteEditorApplyOutcome(
    val diskPersisted: Boolean,
    val editorText: String?,
    val editorNormalizedText: String? = null,
    val fileCacheConflictDeferred: Boolean = false,
)

private enum class RemoteTextApplyDisposition {
    Queued,
    Recovered,
    RetryFailClosed,
}

/**
 * Oldest baseline + newest converged text. Full visible-state projection makes
 * the final text itself the cumulative delivery proof.
 */
private val REMOTE_EDITOR_APPLY_MERGE = MergePolicy(
    name = "RemoteEditorApply",
    merge = { old: PendingRemoteEditorApply, latest: PendingRemoteEditorApply ->
        latest.copy(
            expectedText = old.expectedText,
        )
    },
    commutative = false,
    idempotent = true,
)

/**
 * Production editor-as-CRDT-replica wiring (`#crdtauth5`, realtime phase 3).
 *
 * The manager is intentionally thin: local edits are forwarded to [CrdtReplicaForwarder],
 * remote updates are pulled from the CP document model, and document mutation uses the same
 * minimal-edit helper as IPC patches. A remote mutation is projected as disk-persisted only
 * when raw disk still equals the guarded editor baseline or the converged target; novel external
 * disk text rejects the apply instead of being overwritten.
 */
class CrdtReplicaManager(private val project: Project) : Disposable, DocumentListener {
    private val log = com.intellij.openapi.diagnostic.Logger.getInstance(CrdtReplicaManager::class.java)
    // Project-wide scheduling owns fan-out/backoff only. Every operation that
    // reads or mutates one replica runs on that document's serialized lane.
    private val executor = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "agent-doc-crdt-replica-control").apply { isDaemon = true }
    }
    private val documentWorkers = DocumentReplicaWorkers()
    private val forwarders = ConcurrentHashMap<String, CrdtReplicaForwarder>()
    // Every document ownership chart joins the manager's project-lifetime graph.
    // This keeps cross-thread projections composable instead of creating a
    // private reactive island per open editor.
    private val ownershipContext = ThreadSafeContext()
    private val templateValidation =
        TemplateValidationPlane(
            ownershipContext,
            NativePatching::normalizeTemplateStructure,
            ::contentHash,
        )
    private val fileCacheConflicts = FileCacheConflictPlane(ownershipContext)
    private val shadows = ConcurrentHashMap<String, String>()
    // Unlike [shadows], this frontier advances only after the controller
    // acknowledges the complete visible projection. A local CRDT mutation can
    // update the editing baseline while its controller is retiring or its
    // durable lineage is stale; treating that local baseline as settled lets a
    // replacement controller project an older snapshot over operator text.
    private val settledShadows = ConcurrentHashMap<String, String>()
    private val applyingRemote = ConcurrentHashMap.newKeySet<String>()
    // #opcapturedormant: `beforeFileContentReload` and `fileContentReloaded` are two
    // separate callbacks with no `finally` between them, so a reload that is vetoed,
    // cancelled, or fails mid-write never posts its completion. A bare path set then
    // strands the path forever, `isApplyingNonOperatorMutation` stays true for the
    // life of the manager, and EVERY later operator keystroke is misclassified as a
    // projection — silencing both op capture and local-splice forwarding. Store the
    // reload start instead so the window is bounded and self-healing.
    private val fileContentReloadingPaths = ConcurrentHashMap<String, Long>()
    private val pendingLocalEdits = ConcurrentHashMap<String, AtomicInteger>()
    private val localEditorFlushVersions = ConcurrentHashMap<String, AtomicLong>()
    private val localEditorFlushTasks = ConcurrentHashMap<String, ScheduledFuture<*>>()
    private val localEditorRetryFailureCounts = ConcurrentHashMap<String, Int>()
    private val pendingLocalEditorEdits =
        ConcurrentHashMap<String, MutableList<CapturedLocalEditorEdit>>()
    private val remoteEditorEffectGenerations = ConcurrentHashMap<String, AtomicLong>()

    /** Latest published remote-editor effect generation for [filePath], or `-1`. */
    internal fun remoteEditorEffectGeneration(filePath: String): Long =
        remoteEditorEffectGenerations[filePath]?.get() ?: -1L
    private val localEditorFlushPendingPaths = ConcurrentHashMap.newKeySet<String>()
    private val drainQueued = AtomicBoolean(false)
    private val drainAllRequested = AtomicBoolean(false)
    private val drainRequestedPaths = ConcurrentHashMap.newKeySet<String>()
    private val registerFailureCounts = ConcurrentHashMap<String, Int>()
    private val registerRetryAfterMs = ConcurrentHashMap<String, Long>()
    private val registerRetryProjections =
        ConcurrentHashMap<String, ReplicaRegistrationRetryProjection>()
    private val registerRetryTasks = ConcurrentHashMap<String, ScheduledFuture<*>>()
    private val projectionRecoveryReregisterStartedAtMs = ConcurrentHashMap<String, Long>()
    private val consecutiveNoOpReschedules = AtomicInteger(0)
    private val remoteDrainBackoffScheduled = AtomicBoolean(false)
    private val remoteEditorApplies =
        KeyedCoalescingRelay<String, PendingRemoteEditorApply>(REMOTE_EDITOR_APPLY_MERGE)
    private val remoteEditorApplyScheduled = AtomicBoolean(false)
    private val remoteEditorApplyPaths = ConcurrentHashMap.newKeySet<String>()
    private val unforwardedOperatorTextObservations = ConcurrentHashMap<String, String>()
    private val retainedCanonicalProjectionPaths = ConcurrentHashMap.newKeySet<String>()
    private val retainedProjectionHoldPaths = ConcurrentHashMap.newKeySet<String>()
    private val templateGuardRecoveryPaths = ConcurrentHashMap.newKeySet<String>()
    private val templateGuardRecoveryRetryPaths = ConcurrentHashMap.newKeySet<String>()
    private val templateGuardRecoveryFailureCounts = ConcurrentHashMap<String, Int>()
    private val deferredWriteReplayRetryPaths = ConcurrentHashMap.newKeySet<String>()
    private val deferredWriteReplayFailureCounts = ConcurrentHashMap<String, Int>()
    // `#ctrlkillreregister` Tier 3: transport loss is reported per document, but a
    // dead controller strands every document at once. One pull answers for all of
    // them, so the second and third file to notice must not each start their own.
    private val lastPeerReplicaPullAtMs = AtomicLong(0)
    private val disposed = AtomicBoolean(false)

    fun start() {
        EditorFactory.getInstance().eventMulticaster.addDocumentListener(this, this)
        ApplicationManager.getApplication().messageBus.connect(this).subscribe(
            FileDocumentManagerListener.TOPIC,
            object : FileDocumentManagerListener {
                override fun beforeFileContentReload(file: VirtualFile, document: Document) {
                    val filePath = file.path
                    if (!file.name.endsWith(".md") || managerForFilePath(filePath) !== this@CrdtReplicaManager) {
                        return
                    }
                    fileContentReloadingPaths[filePath] = System.currentTimeMillis()
                    advanceNonOperatorMutationEpoch(filePath)
                }

                override fun fileContentReloaded(file: VirtualFile, document: Document) {
                    fileContentReloadingPaths.remove(file.path)
                }
            },
        )
        ApplicationManager.getApplication().messageBus.connect(this).subscribe(
            VirtualFileManager.VFS_CHANGES,
            object : BulkFileListener {
                override fun after(events: List<VFileEvent>) {
                    events
                        .asSequence()
                        .filterIsInstance<VFileContentChangeEvent>()
                        .map { it.file.path }
                        .filter { it.endsWith(".md") && managerForFilePath(it) === this@CrdtReplicaManager }
                        .distinct()
                        .forEach(::projectNativeSaveReceipt)
                }
            },
        )
    }

    override fun dispose() {
        disposed.set(true)
        executor.shutdownNow()
        documentWorkers.shutdownNow()
        localEditorFlushTasks.clear()
        localEditorFlushVersions.clear()
        localEditorRetryFailureCounts.clear()
        pendingLocalEditorEdits.clear()
        localEditorFlushPendingPaths.clear()
        pendingLocalEdits.clear()
        fileContentReloadingPaths.clear()
        remoteEditorApplies.clear()
        remoteEditorApplyPaths.clear()
        unforwardedOperatorTextObservations.clear()
        retainedCanonicalProjectionPaths.clear()
        retainedProjectionHoldPaths.clear()
        remoteEditorEffectGenerations.clear()
        templateGuardRecoveryPaths.clear()
        templateGuardRecoveryRetryPaths.clear()
        templateGuardRecoveryFailureCounts.clear()
        deferredWriteReplayRetryPaths.clear()
        deferredWriteReplayFailureCounts.clear()
        drainRequestedPaths.clear()
        registerFailureCounts.clear()
        registerRetryAfterMs.clear()
        registerRetryProjections.clear()
        registerRetryTasks.values.forEach { it.cancel(false) }
        registerRetryTasks.clear()
        projectionRecoveryReregisterStartedAtMs.clear()
        forwarders.values.forEach { it.deregister() }
        forwarders.clear()
        shadows.clear()
        settledShadows.clear()
        templateValidation.clear()
        fileCacheConflicts.clear()
    }

    private fun awaitWorkerTermination(timeoutMs: Long): Boolean {
        val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMs)
        val controlStopped = try {
            executor.awaitTermination(timeoutMs, TimeUnit.MILLISECONDS)
        } catch (_: InterruptedException) {
            Thread.currentThread().interrupt()
            false
        }
        if (!controlStopped) return false
        val remaining = deadline - System.nanoTime()
        return remaining > 0L &&
            documentWorkers.awaitTermination(TimeUnit.NANOSECONDS.toMillis(remaining).coerceAtLeast(1L))
    }

    /**
     * Claim the next Tier 3 pull window, or report that a recent pull already covers
     * this caller. Reuses [projectionRecoveryReregisterDueUtil]'s interval rule; only the
     * window differs, because one pull is authoritative for every document.
     */
    private fun beginPeerReplicaPull(): Boolean {
        val nowMs = System.currentTimeMillis()
        val lastMs = lastPeerReplicaPullAtMs.get()
        if (!projectionRecoveryReregisterDueUtil(
                lastStartedMs = lastMs.takeIf { it > 0L },
                nowMs = nowMs,
                minIntervalMs = PEER_REPLICA_PULL_MIN_INTERVAL_MS,
            )
        ) {
            return false
        }
        return lastPeerReplicaPullAtMs.compareAndSet(lastMs, nowMs)
    }

    private fun beginProjectionRecoveryReregister(filePath: String): Boolean {
        val nowMs = System.currentTimeMillis()
        var due = false
        projectionRecoveryReregisterStartedAtMs.compute(filePath) { _, lastStartedMs ->
            if (projectionRecoveryReregisterDueUtil(lastStartedMs, nowMs)) {
                due = true
                nowMs
            } else {
                lastStartedMs
            }
        }
        return due
    }

    override fun documentChanged(event: DocumentEvent) {
        if (disposed.get()) return
        val started = System.nanoTime()
        var loggedFilePath: String? = null
        try {
            val file = FileDocumentManager.getInstance().getFile(event.document) ?: return
            if (!file.name.endsWith(".md")) return
            val filePath = file.path
            loggedFilePath = filePath
            if (managerForFilePath(filePath) !== this) return
            if (!CrdtReplicaManager.isOperatorDocumentEvent(filePath, event)) {
                advanceNonOperatorMutationEpoch(filePath)
                // A remote CRDT apply mutates the IntelliJ Document before its
                // visible-content projection reaches this worker. Replacing
                // the replica from this listener would run ahead of that projection,
                // retire its retained frontier, and make the controller deliver
                // the same canonical edit again. The remote apply path updates
                // the shadow and schedules its own post-projection drain.
                if (CrdtReplicaManager.isApplyingRemote(filePath)) return
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
            val edit =
                CapturedLocalEditorEdit(
                    offsetUtf16 = event.offset,
                    oldFragment = oldFragment,
                    newFragment = newFragment,
                    projectionEpoch = projectionEpoch,
                )
            if (!shadows.containsKey(filePath)) {
                val visibleText = tryReadDocumentText(event.document) ?: return
                val beforeText = reconstructLocalEditorBaseTextUtil(visibleText, edit)
                if (beforeText == null) {
                    log.warn(
                        "[crdt-replica] first local splice could not reconstruct its base for $filePath; " +
                            "leaving the operator buffer untouched",
                    )
                    return
                }
                // Install the pre-edit base synchronously before another EDT
                // event arrives. The serialized worker will register from this
                // cut and forward this deletion/insertion as a real local delta.
                shadows.putIfAbsent(filePath, beforeText)
            }
            recordLocalEditorEdit(filePath, edit)
            scheduleLocalEditorFlush(filePath)
        } finally {
            loggedFilePath?.let { logSlow("documentChanged-listener", it, started, warnMs = CRDT_LISTENER_WARN_MS) }
        }
    }

    /**
     * Keep at most one running and one latest queued splice flush per document.
     *
     * A native/controller round trip can be slower than typing. Enqueuing every
     * DocumentEvent then makes a one-second operation repeat once per character.
     * Exact event splices accumulate for one transport batch. The retained
     * version owns one pending-local fence for the whole burst; a superseded
     * task never clears that fence out from under its successor.
     */
    private fun scheduleLocalEditorFlush(
        filePath: String,
        retryVersion: Long? = null,
    ) {
        val versionCounter =
            localEditorFlushVersions.computeIfAbsent(filePath) { AtomicLong(0L) }
        val version = retryVersion ?: versionCounter.incrementAndGet()
        if (retryVersion == null && localEditorFlushPendingPaths.add(filePath)) {
            markLocalPending(filePath)
        }
        val retryFailureCount = localEditorRetryFailureCounts[filePath] ?: 0
        val delayMs =
            if (retryVersion == null && retryFailureCount == 0) {
                LOCAL_EDITOR_FLUSH_QUIET_MS
            } else {
                localEditorRetryDelayMsUtil(retryFailureCount)
            }
        lateinit var scheduled: ScheduledFuture<*>
        try {
            scheduled =
                documentWorkers.forDocument(filePath).schedule(
                    Runnable {
                        if (disposed.get() || versionCounter.get() != version) {
                            return@Runnable
                        }
                        val workerStarted = System.nanoTime()
                        val capturedEdits = drainLocalEditorEdits(filePath)
                        var retrySplices = false
                        try {
                            retrySplices =
                                forwardLocalEditsFromShadow(
                                    filePath,
                                    capturedEdits,
                                ) == LocalEditorForwardResult.Retry
                        } catch (error: Exception) {
                            retrySplices = true
                            log.warn(
                                "[crdt-replica] local editor splice flush failed for $filePath: ${error.message}",
                                error,
                            )
                        } finally {
                    if (retrySplices) {
                        prependLocalEditorEdits(filePath, capturedEdits)
                        localEditorRetryFailureCounts.compute(filePath) { _, failures ->
                            ((failures ?: 0) + 1).coerceAtMost(13)
                        }
                    } else {
                        localEditorRetryFailureCounts.remove(filePath)
                    }
                            if (versionCounter.get() == version) {
                                if (retrySplices && !disposed.get()) {
                            scheduleLocalEditorFlush(
                                filePath,
                                retryVersion = version,
                            )
                                } else {
                                    localEditorFlushTasks.remove(filePath, scheduled)
                                    localEditorFlushVersions.remove(filePath, versionCounter)
                                    if (localEditorFlushPendingPaths.remove(filePath)) {
                                        clearLocalPending(filePath)
                                    }
                                    logSlow(
                                    "local-delta-worker",
                                    filePath,
                                    workerStarted,
                                    details = "splices=${capturedEdits.size}",
                                    )
                                    requestRemoteDrain(filePath, "local-delta")
                                }
                            }
                        }
                    },
                    delayMs,
                    TimeUnit.MILLISECONDS,
                )
            localEditorFlushTasks.put(filePath, scheduled)?.cancel(false)
        } catch (error: RejectedExecutionException) {
            if (versionCounter.get() == version) {
                localEditorFlushVersions.remove(filePath, versionCounter)
                localEditorRetryFailureCounts.remove(filePath)
                if (localEditorFlushPendingPaths.remove(filePath)) {
                    clearLocalPending(filePath)
                }
            }
            if (!disposed.get()) {
                log.warn("[crdt-replica] local editor flush scheduling rejected for $filePath", error)
            }
        }
    }

    private fun recordLocalEditorEdit(filePath: String, edit: CapturedLocalEditorEdit) {
        pendingLocalEditorEdits.compute(filePath) { _, existing ->
            (existing ?: mutableListOf()).also { it.add(edit) }
        }
    }

    private fun drainLocalEditorEdits(filePath: String): List<CapturedLocalEditorEdit> {
        var drained: List<CapturedLocalEditorEdit> = emptyList()
        pendingLocalEditorEdits.compute(filePath) { _, existing ->
            if (existing != null) drained = existing.toList()
            null
        }
        return drained
    }

    private fun prependLocalEditorEdits(
        filePath: String,
        edits: List<CapturedLocalEditorEdit>,
    ) {
        if (edits.isEmpty()) return
        pendingLocalEditorEdits.compute(filePath) { _, existing ->
            mutableListOf<CapturedLocalEditorEdit>().also { retained ->
                retained.addAll(edits)
                if (existing != null) retained.addAll(existing)
            }
        }
    }

    fun ensureOpenDocumentReplica(
        filePath: String,
        document: Document,
        editorText: String? = null,
        await: Boolean = false,
        forceRefresh: Boolean = false,
        requireFreshRegistration: Boolean = false,
        // `#netadv5` R2: told when the bounded await elapsed with the attach still
        // running, so the caller can answer "slow, still trying" instead of "refused".
        onAwaitTimeout: (() -> Unit)? = null,
    ): Boolean {
        // TypingTracker reports the Lazily current-document projection after
        // each coalesced edit burst. Once this document already owns a live
        // replica, that observation must not enqueue another attach, normalize
        // the whole markdown document, or overwrite the incremental shadow with
        // a future editor cut. The latter used to advance the shadow past queued
        // DocumentEvents and manufacture the stale-baseline recovery loop.
        if (!forceRefresh && forwarders[filePath]?.attached == true) {
            return true
        }
        val attach = attach@{
            val started = System.nanoTime()
            var chars = -1
            try {
                val text = editorText ?: tryReadDocumentText(document)
                    ?: run {
                        attachFailureReasons[filePath] = "editor-text-unavailable"
                        return@attach false
                    }
                chars = text.length
                // `#replicarefusalstorm` (the editor half). The controller refuses a
                // non-agent-doc markdown file terminally and cheaply, and its comment
                // names this as the durable stop: "the editor only registering
                // agent-doc documents". Until now the editor registered every `.md`
                // it opened and retried the refusal on an 8s backoff forever —
                // measured 2026-08-12 at ~150 retries per file per 20 minutes across
                // four plain markdown files.
                //
                // No sticky state: the test runs on the CURRENT text every time, so a
                // plain file that later gains `<!-- agent:` markers registers on the
                // next observation without anything to invalidate.
                if (!isAgentDocDocumentTextUtil(text)) {
                    // Recorded so an operator-facing command can say WHY, but
                    // deliberately NOT through `recordRegisterFailure`: that arms a
                    // retry, and there is nothing here to converge on.
                    attachFailureReasons[filePath] = "not_agent_doc_document"
                    return@attach false
                }
                // Registration always opens the controller bootstrap. An existing
                // editor buffer is a downstream consumer until subsequent DocumentEvent
                // deltas prove new operator intent.
                val registrationText = text
                if (!NativePatching.isAvailable()) {
                    log.warn(
                        "[crdt-replica] open-document replica registration deferred for ${File(filePath).name}; " +
                            "native FFI unavailable",
                    )
                    recordRegisterFailure(filePath, "native-ffi-unavailable")
                    return@attach false
                }
                chars = registrationText.length
                val previousForwarder = forwarders[filePath]
                val pendingLocalAtRegistration =
                    requireFreshRegistration && hasPendingLocal(filePath)
                val forwarder = forwarderFor(
                    filePath,
                    registrationText,
                    bypassRegisterBackoff = forceRefresh,
                    replaceCached = forceRefresh,
                    expectedEditorTextAtSwap = if (forceRefresh) registrationText else null,
                    // A typed missing-membership repair is already bounded by the
                    // controller. It must attempt a real registration even when a
                    // prior three-generation hold armed the ordinary retry gate.
                    bypassRetainedProjectionHold = requireFreshRegistration,
                    // If operator splices are queued, install an exact canonical
                    // endpoint without projecting it over the editor. The serialized
                    // local worker retains the shadow-relative splice batch and rebases
                    // it onto this endpoint next; no whole-buffer adoption occurs.
                    allowPendingLocalAtSwap = pendingLocalAtRegistration,
                    bootstrapFromControllerCanonical = pendingLocalAtRegistration,
                    deferCanonicalProjectionForPendingLocal = pendingLocalAtRegistration,
                )
                val attached =
                    forwarder != null &&
                        forwarder.attached &&
                        (!requireFreshRegistration || forwarder !== previousForwarder)
                attached.also {
                    if (attached) {
                        // Queue retained semantic replay behind this registration task.
                        // In await mode the caller can therefore observe registration
                        // before any native reconstruction or EDT mutation begins.
                        scheduleDeferredWriteReplayAfterRegistration(
                            filePath,
                            document,
                            forwarder!!,
                        )
                        requestRemoteDrain(filePath, "open-document")
                    }
                }
            } catch (e: Exception) {
                log.debug("[crdt-replica] open-document attach skipped for $filePath: ${e.message}")
                recordRegisterFailure(filePath, "attach-exception")
                false
            } finally {
                logSlow("open-document-attach", filePath, started, details = "chars=$chars force_refresh=$forceRefresh")
            }
        }
        if (await) {
            return try {
                documentWorkers.forDocument(filePath).submit<Boolean> { attach() }
                    .get(CRDT_AWAIT_ATTACH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
            } catch (e: TimeoutException) {
                log.warn("[crdt-replica] open-document attach timed out for $filePath after ${CRDT_AWAIT_ATTACH_TIMEOUT_MS}ms (attach still running; receipt=deferred)")
                onAwaitTimeout?.invoke()
                if (forwarders[filePath]?.attached == true) return true
                attachFailureReasons[filePath] = "attach-timeout-pending"
                false
            } catch (e: Exception) {
                log.debug("[crdt-replica] open-document attach failed for $filePath: ${e.message}")
                attachFailureReasons[filePath] = "attach-worker-failed"
                false
            }
        }
        documentWorkers.forDocument(filePath).execute { attach() }
        return true
    }

    private fun rebindOpenDocumentPath(
        oldPath: String,
        newPath: String,
        document: Document,
        editorText: String,
    ): Boolean {
        if (oldPath == newPath) {
            return ensureOpenDocumentReplica(
                newPath,
                document,
                editorText = editorText,
                await = true,
                forceRefresh = false,
            )
        }
        return try {
            documentWorkers.forDocument(newPath).submit<Boolean> {
                val existingNew = forwarders[newPath]
                val oldForwarder = forwarders[oldPath]
                val activeNew =
                    if (existingNew?.attached == true) {
                        existingNew
                    } else {
                        val root = resolveProjectRoot(newPath) ?: return@submit false
                        val resume = oldForwarder?.captureResumeState()
                        val replacement =
                            CrdtReplicaForwarder(
                                filePath = newPath,
                                identity = EditorIdentity.nextReplicaConnectionIdentity(newPath),
                                node = NativeReplicaNode(),
                                transport = CpSocketReplicaTransport(root),
                                ownershipContext = ownershipContext,
                                resumeState = resume,
                            )
                        if (!replacement.register()) {
                            recordRegisterFailure(newPath, "path-transition-register")
                            return@submit false
                        }
                    val raced = forwarders.putIfAbsent(newPath, replacement)
                        if (raced != null) {
                            replacement.deregister()
                            raced
                    } else {
                        retainCanonicalProjectionAfterRegistration(newPath, replacement)
                        replacement
                        }
            }

            if (!activeNew.attached) return@submit false
            retainCanonicalProjectionAfterRegistration(newPath, activeNew)
            clearRegisterFailure(newPath)
                scheduleDeferredWriteReplayAfterRegistration(
                    newPath,
                    document,
                    activeNew,
                )

                if (oldForwarder != null && forwarders.remove(oldPath, oldForwarder)) {
                    oldForwarder.deregister()
                }
                shadows.remove(oldPath)
                localEditorFlushTasks.remove(oldPath)?.cancel(false)
                localEditorFlushVersions.remove(oldPath)
                pendingLocalEditorEdits.remove(oldPath)
                if (localEditorFlushPendingPaths.remove(oldPath)) {
                    clearLocalPending(oldPath)
                }
                pendingLocalEdits.remove(oldPath)
                remoteEditorEffectGenerations.remove(oldPath)
                drainRequestedPaths.remove(oldPath)
                registerFailureCounts.remove(oldPath)
                registerRetryAfterMs.remove(oldPath)
                registerRetryProjections.remove(oldPath)
                registerRetryTasks.remove(oldPath)?.cancel(false)
                projectionRecoveryReregisterStartedAtMs.remove(oldPath)
                remoteEditorApplyPaths.remove(oldPath)
                unforwardedOperatorTextObservations.remove(oldPath)
                retainedCanonicalProjectionPaths.remove(oldPath)
                templateGuardRecoveryPaths.remove(oldPath)
                templateGuardRecoveryRetryPaths.remove(oldPath)
                templateGuardRecoveryFailureCounts.remove(oldPath)
                deferredWriteReplayRetryPaths.remove(oldPath)
                deferredWriteReplayFailureCounts.remove(oldPath)
                nonOperatorMutationEpochs.remove(oldPath)?.let { oldEpoch ->
                    nonOperatorMutationEpochs
                        .computeIfAbsent(newPath) { AtomicLong(0L) }
                        .updateAndGet { current -> maxOf(current, oldEpoch.get()) }
                }
                TypingTracker.rekeyDocumentPath(oldPath, newPath, document)
                requestRemoteDrain(newPath, "document-path-transition")
                true
            }.get(CRDT_AWAIT_CLOSE_PUBLISH_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        } catch (error: Exception) {
            log.warn(
                "[crdt-replica] path transition rebind deferred $oldPath → $newPath: ${error.message}",
            )
            false
        }
    }

    private fun scheduleDeferredWriteReplayAfterRegistration(
        filePath: String,
        document: Document,
        forwarder: CrdtReplicaForwarder,
    ) {
        try {
            documentWorkers.forDocument(filePath).execute {
                if (disposed.get() || forwarders[filePath] !== forwarder) {
                    return@execute
                }
            replayDeferredWriteAfterRegistration(
                filePath,
                document,
                forwarder,
            )
        }
        } catch (error: RejectedExecutionException) {
            if (!disposed.get()) {
                log.warn(
                    "[crdt-replica] deferred write replay scheduling rejected for $filePath",
                    error,
                )
            }
        }
    }

    private fun replayDeferredWriteAfterRegistration(
        filePath: String,
        document: Document,
        forwarder: CrdtReplicaForwarder,
    ): Boolean {
        if (forwarders[filePath] !== forwarder) return false
        val publishedEditorCut = tryReadDocumentText(document)
        if (
            publishedEditorCut == null ||
            hasPendingLocal(filePath) ||
            shadows[filePath] != publishedEditorCut
        ) {
            if (publishedEditorCut != null && !hasPendingLocal(filePath)) {
                scheduleUnforwardedOperatorTextRecovery(filePath, "post-register-replay-replica-raced")
            }
            scheduleDeferredWriteReplayRetry(
                filePath,
                document,
                forwarder,
                "post-register-replay-replica-raced",
            )
            return false
        }
        if (!NativePatching.projectDeferredWritePostRegister(filePath, publishedEditorCut)) {
            scheduleDeferredWriteReplayRetry(
                filePath,
                document,
                forwarder,
                "post-register-projection-unavailable",
            )
            return false
        }
        deferredWriteReplayRetryPaths.remove(filePath)
        deferredWriteReplayFailureCounts.remove(filePath)
        // FFI only wakes/projects the retained semantic intent into the
        // controller-owned CRDT authority. The ordinary remote-delivery path
        // below is the sole owner of editor mutation and persistence.
        requestUrgentRemoteDrain(filePath, "post-register-projected-intent")
        log.info(
            "[crdt-replica] projected deferred write after exact editor registration for $filePath; " +
                "baseline_hash=${contentHash(publishedEditorCut)} editor_mutation=remote_delivery_only",
        )
        return true
    }

    private fun scheduleDeferredWriteReplayRetry(
        filePath: String,
        document: Document,
        forwarder: CrdtReplicaForwarder,
        reason: String,
    ) {
        if (!deferredWriteReplayRetryPaths.add(filePath)) return
        val failureCount = deferredWriteReplayFailureCounts.merge(filePath, 1) { current, one ->
            current + one
        } ?: 1
        val shifted = 1L shl minOf(failureCount - 1, 12)
        val delayMs = minOf(
            CRDT_DRAIN_NOOP_RESCHEDULE_BASE_BACKOFF_MS * shifted,
            CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS,
        )
        log.info(
            "[crdt-replica] deferred write replay retry scheduled for ${File(filePath).name}; " +
                "reason=$reason delay_ms=$delayMs failures=$failureCount",
        )
        documentWorkers.forDocument(filePath).schedule(
            {
                deferredWriteReplayRetryPaths.remove(filePath)
                if (!disposed.get() && forwarders[filePath] === forwarder) {
                    replayDeferredWriteAfterRegistration(filePath, document, forwarder)
                }
            },
            delayMs,
            TimeUnit.MILLISECONDS,
        )
    }

    /**
     * Publish the exact closing editor cut through the same serialized Lazily
     * replica worker as every preceding local delta, then retire that replica.
     * The reliable-sync close fact is emitted only after this returns true, so
     * the controller can hand authority to disk without losing a last unsaved
     * deletion that was still waiting behind the debounce worker.
     */
    private fun publishClosingDocumentCut(filePath: String, document: Document): Boolean {
        val closingText = tryReadDocumentText(document) ?: return false
        return try {
            documentWorkers.forDocument(filePath).submit<Boolean> {
                val forwarder = forwarderFor(filePath, closingText) ?: return@submit false
                forwarder.ensureEditorText(closingText)
                shadows[filePath] = closingText
                clearLocalPending(filePath)
                if (forwarders.remove(filePath, forwarder)) {
                    forwarder.deregister()
                }
                templateValidation.remove(filePath)
                fileCacheConflicts.remove(filePath)
                true
            // Closing the editor removes the only replica that can settle this
            // cut. Do not apply the ordinary short RPC budget here: a large
            // document can legitimately spend several seconds broadcasting
            // its final update, and returning early would let this queued task
            // deregister the replica after the caller had retained authority.
            // This runs on the content-report worker, never the EDT, so wait for
            // the serialized resolver to finish before emitting the close fact.
            }.get()
        } catch (e: Exception) {
            log.warn("[crdt-replica] closing editor cut publish failed for $filePath: ${e.message}")
            false
        }
    }

    private fun forwardLocalEditsFromShadow(
        filePath: String,
        capturedEdits: List<CapturedLocalEditorEdit>,
    ): LocalEditorForwardResult {
        val started = System.nanoTime()
        if (capturedEdits.isEmpty()) return LocalEditorForwardResult.Applied
        val currentEpoch = nonOperatorMutationEpoch(filePath)
        val currentEdits = currentEpochCapturedEditsUtil(capturedEdits, currentEpoch)
        if (currentEdits.isEmpty()) {
            log.debug(
                "[crdt-replica] dropped stale operator splice batch for $filePath after a newer non-operator projection",
            )
            requestRemoteDrain(filePath, "stale-operator-event-fenced")
            return LocalEditorForwardResult.Fenced
        }
        val beforeText = shadows[filePath] ?: return LocalEditorForwardResult.Retry
        val batch =
            prepareLocalEditorEditsUtil(beforeText, currentEdits)
                ?: run {
                    log.debug(
                        "[crdt-replica] retained local splice batch for $filePath because its exact shadow range no longer matches",
                    )
                    requestRemoteDrain(filePath, "captured-local-splice-baseline-mismatch")
                    return LocalEditorForwardResult.Retry
                }
        if (currentEpoch != nonOperatorMutationEpoch(filePath)) {
            requestRemoteDrain(filePath, "stale-operator-event-fenced")
            return LocalEditorForwardResult.Fenced
        }
        val editorText = batch.resultingText
        val forwarder =
            forwarderFor(
                filePath = filePath,
                initialEditorText = beforeText,
                deferCanonicalProjectionForPendingLocal = true,
            )
        if (forwarder == null) {
            shadows[filePath] = beforeText
            return LocalEditorForwardResult.Retry
        }
        val replicaText = forwarder.replicaText()
        val forwarded =
            when (localReplicaBaselineDecisionUtil(replicaText, beforeText)) {
                LocalReplicaBaselineDecision.ForwardLocal -> {
                    currentEpoch == nonOperatorMutationEpoch(filePath) &&
                        forwarder.forwardLocalEdits(batch)
                }
                LocalReplicaBaselineDecision.RebootstrapCanonicalThenForward ->
                    rebootstrapCanonicalAndForwardCapturedLocalEdit(
                        filePath = filePath,
                        capturedBaseText = beforeText,
                        visibleEditorText = editorText,
                        staleForwarder = forwarder,
                        batch = batch,
                    )
            }
        if (!forwarded) {
            // Retain the exact captured base. Advancing the shadow before the
            // delta reaches a canonical replica loses the only safe rebase
            // proof and turns the next retry into whole-editor adoption.
            shadows[filePath] = beforeText
            if (replicaText != beforeText) {
                retainedCanonicalProjectionPaths.add(filePath)
                log.warn(
                    "[crdt-replica] local delta found a stale native baseline for ${File(filePath).name}; " +
                        "shadow_hash=${contentHash(beforeText)} " +
                        "replica_hash=${replicaText?.let(::contentHash) ?: "missing"} " +
                        "recovery=canonical-rebootstrap-captured-local-delta",
                )
            }
            requestRemoteDrain(filePath, "captured-local-delta-retry")
            return LocalEditorForwardResult.Retry
        }
        shadows[filePath] = editorText
        if (forwarders[filePath]?.replicaText() != editorText) {
            retainedCanonicalProjectionPaths.add(filePath)
            requestRemoteDrain(filePath, "rebased-local-splices-projection")
        }
        if (!projectSettledVisibleState(filePath, forwarders[filePath]!!, editorText)) {
            requestRemoteDrain(filePath, "local-visible-projection-retry")
        }
        logSlow(
            "forward-local-delta",
            filePath,
            started,
            details =
                "splices=${batch.edits.size} before_chars=${beforeText.length} after_chars=${editorText.length}",
        )
        return LocalEditorForwardResult.Applied
    }

    /**
     * `#ambiguousholdforever`: only consulted when shadow, buffer and canonical
     * all differ. The native proof answers true exactly when every operator
     * change from the shadow is already in canonical; anything else, including a
     * missing native library, leaves the hold in place.
     */
    private fun canonicalContainsOperatorEditsAtRegistration(
        filePath: String,
        canonicalProjectionRetained: Boolean,
        publishedShadow: String?,
        bufferText: String?,
        canonicalText: String?,
    ): Boolean? {
        // Computed for ordinary re-registers too (`#reloadclobbersoperatortext`):
        // it is the lossless exit from a hold on either path.
        if (publishedShadow == null || bufferText == null || canonicalText == null) {
            return null
        }
        if (bufferText == publishedShadow || canonicalText == bufferText || canonicalText == publishedShadow) {
            return null
        }
        // `#ambiguousholdforever2`: prove containment over operator text only. A controller
        // disk projection that the IDE reloaded carries binary-owned `(HEAD)` / boundary
        // placement canonical does not share; left in, that bookkeeping change can never be
        // "contained" and the hold had no exit (formal/tla/RetainedProjectionHold.tla).
        val shadowOperatorText = withoutBinaryOwnedMarkersUtil(publishedShadow)
        val bufferOperatorText = withoutBinaryOwnedMarkersUtil(bufferText)
        val canonicalOperatorText = withoutBinaryOwnedMarkersUtil(canonicalText)
        val contained =
            if (bufferOperatorText == shadowOperatorText || bufferOperatorText == canonicalOperatorText) {
                true
            } else {
                NativePatching.capturedSplicesContained(
                    shadowOperatorText,
                    canonicalOperatorText,
                    singleSpliceBatchUtil(shadowOperatorText, bufferOperatorText),
                ) ?: return false
            }
        if (contained) {
            log.info(
                "[crdt-replica] retained canonical already contains the operator buffer's edits for " +
                    "${File(filePath).name}; adopting canonical is lossless. " +
                    "shadow_hash=${contentHash(publishedShadow)} buffer_hash=${contentHash(bufferText)} " +
                    "canonical_hash=${contentHash(canonicalText)}",
            )
        }
        return contained
    }

    private fun rebootstrapCanonicalAndForwardCapturedLocalEdit(
        filePath: String,
        capturedBaseText: String,
        visibleEditorText: String,
        staleForwarder: CrdtReplicaForwarder,
        batch: PreparedLocalEditorBatch,
    ): Boolean {
        if (forwarders[filePath] !== staleForwarder) return false
        val root = resolveProjectRoot(filePath) ?: return false
        val canonical = CpSocketReplicaTransport(root).currentCanonicalText(filePath) ?: return false
        val rebased = NativePatching.rebaseCapturedSplices(capturedBaseText, canonical, batch) ?: return false
        if (
            capturedRebaseCanReuseEndpointUtil(
                endpointReplicaText = staleForwarder.replicaText(),
                canonicalText = canonical,
                visibleEditorText = editorBufferText(filePath),
                capturedVisibleText = visibleEditorText,
            )
        ) {
            // `#steerreplicachurn`: the endpoint already holds this exact
            // canonical generation (it received the controller's update while
            // the operator's burst waited). Forward the rebased splices on it.
            // Re-registering from a full bootstrap (6.4 MB for sdk.md, 12 s)
            // raced every controller write, failed promotion, and churned the
            // replica the controller was waiting on.
            return forwarders[filePath] === staleForwarder && staleForwarder.forwardLocalEdits(rebased)
        }
        val replacement =
            forwarderFor(
                filePath = filePath,
                bypassRegisterBackoff = true,
                replaceCached = true,
                expectedEditorTextAtSwap = visibleEditorText,
                allowPendingLocalAtSwap = true,
                bootstrapFromControllerCanonical = true,
                expectedCanonicalTextAtSwap = canonical,
                deferCanonicalProjectionForPendingLocal = true,
            )
        if (replacement == null || replacement === staleForwarder) return false
        if (
            editorBufferText(filePath) != visibleEditorText ||
            replacement.replicaText() != canonical
        ) {
            return false
        }
        return replacement.forwardLocalEdits(rebased)
    }

    fun requestRemoteDrain(filePath: String? = null, reason: String = "event") {
        if (filePath == null) {
            drainAllRequested.set(true)
        } else {
            drainRequestedPaths.add(filePath)
        }
        queueRemoteDrain(reason)
    }

    /** Queue retained drain flags without creating new work. */
    private fun queueRemoteDrain(reason: String) {
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
                    // updates (notably when the CP socket is unavailable and every
                    // pullDelivery returns empty deltas), delay the reschedule with
                    // exponential backoff instead of re-executing immediately. A
                    // tight no-op spin generated ~70MB/min of logs and froze the IDE.
                    val delayMs = nextNoOpRescheduleBackoffMs()
                    log.debug("[crdt-replica] no-op drain cycle; backing off reschedule by ${delayMs}ms (consecutive=${consecutiveNoOpReschedules.get()})")
                    // Publish the backoff gate before releasing drainQueued so an
                    // external CRDT event cannot win the gap and start immediately.
                    scheduleRemoteDrainAfterBackoff(delayMs)
                    drainQueued.set(false)
                } else if (moreWorkRequested) {
                    consecutiveNoOpReschedules.set(0)
                    drainQueued.set(false)
                    queueRemoteDrain("rescheduled")
                } else {
                    consecutiveNoOpReschedules.set(0)
                    drainQueued.set(false)
                    // Close the request-vs-release race: a request can arrive after the
                    // moreWorkRequested snapshot but before drainQueued is released.
                    if (drainAllRequested.get() || drainRequestedPaths.isNotEmpty()) {
                        queueRemoteDrain("handoff")
                    }
                }
            }
        }
    }

    /**
     * Foreground delivery recovery for a controller write that is already
     * retained in the existing replica's delivery frontier. This deliberately
     * bypasses only the background no-op drain timer: it neither clears that
     * timer nor replaces the replica. Re-registering from the visible editor
     * here would publish the pre-delivery buffer back into canonical and undo
     * the controller write before the editor had a chance to apply it.
     */
    fun requestUrgentRemoteDrain(filePath: String, reason: String) {
        documentWorkers.forDocument(filePath).execute {
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

    private fun nextNoOpRescheduleBackoffMs(): Long {
        val n = consecutiveNoOpReschedules.incrementAndGet()
        val shifted = 1L shl minOf(n - 1, 12)
        return minOf(
            CRDT_DRAIN_NOOP_RESCHEDULE_BASE_BACKOFF_MS * shifted,
            CRDT_DRAIN_NOOP_RESCHEDULE_MAX_BACKOFF_MS,
        )
    }

    private fun scheduleRemoteDrainAfterBackoff(delayMs: Long, retryFilePath: String? = null) {
        if (retryFilePath != null) drainRequestedPaths.add(retryFilePath)
        if (!remoteDrainBackoffScheduled.compareAndSet(false, true)) return
        executor.schedule(
            {
                remoteDrainBackoffScheduled.set(false)
                if (!disposed.get()) queueRemoteDrain(CRDT_DRAIN_BACKOFF_REASON)
            },
            delayMs,
            TimeUnit.MILLISECONDS,
        )
    }

    private fun drainRemoteUpdates(reason: String): Int {
        val started = System.nanoTime()
        val drainAll = drainAllRequested.getAndSet(false)
        val paths = if (drainAll) {
            // Every currently requested path is already covered by this all-replica
            // snapshot. Leaving those flags behind rearms the no-op backoff forever.
            drainRequestedPaths.clear()
            forwarders.keys().toList()
        } else {
            drainRequestedPaths.toList().also { drained ->
                drained.forEach { drainRequestedPaths.remove(it) }
            }
        }
        if (paths.isEmpty()) return 0
        log.debug("[crdt-replica] draining ${paths.size} replica(s) via $reason")
        val drains = paths.mapNotNull { filePath ->
            val forwarder = forwarders[filePath] ?: return@mapNotNull null
            filePath to documentWorkers.forDocument(filePath).submit<Int> {
                if (forwarders[filePath] !== forwarder) {
                    0
                } else {
                    drainRemoteUpdatesFor(filePath, forwarder)
                }
            }
        }
        var appliedTotal = 0
        for ((filePath, drain) in drains) {
            try {
                appliedTotal += drain.get()
            } catch (e: Exception) {
                log.debug("[crdt-replica] document drain skipped for $filePath: ${e.message}")
            }
        }
        logSlow(
            "remote-drain",
            paths.firstOrNull() ?: "(none)",
            started,
            warnMs = if (appliedTotal == 0) CRDT_IDLE_WORKER_WARN_MS else CRDT_WORKER_WARN_MS,
            details = "paths=${paths.size} reason=$reason drain_all=$drainAll applied_total=$appliedTotal",
        )
        return appliedTotal
    }

    private fun drainRemoteUpdatesFor(filePath: String, forwarder: CrdtReplicaForwarder): Int {
        val started = System.nanoTime()
        var updateCount = 0
        var selfEchoCount = 0
        var peerUpdateCount = 0
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
            if (updates.isEmpty()) {
                if (retainedCanonicalProjectionPaths.contains(filePath)) {
                    val canonical = forwarder.replicaText()
                    if (canonical != null && canonical != expectedText) {
                        queueRemoteTextApply(filePath, expectedText, canonical, forwarder, emptyList())
                    }
                }
                return usefulWork
            }

        if (!editorReplicaBaselineMatches(filePath, forwarder, expectedText, updates)) {
            return usefulWork
        }
            val appliedRemoteUpdates = mutableListOf<ReplicaRemoteUpdate>()
            var converged: String? = null
            for (update in updates) {
                if (hasPendingLocal(filePath)) break
                if (!shouldApplyRemoteCrdtUpdateUtil(update, forwarder.clientId)) {
                    selfEchoCount++
                    val visibleText = editorBufferText(filePath) ?: expectedText
                    if (!projectSettledVisibleState(filePath, forwarder, visibleText)) {
                        requestRemoteDrain(filePath, "self-echo-projection-retry")
                    }
                    continue
                }
                peerUpdateCount++
                converged = forwarder.applyRemoteUpdate(update.update) ?: break
                appliedRemoteUpdates.add(update)
            }

            val targetText = converged
            if (targetText != null && appliedRemoteUpdates.isNotEmpty() && !hasPendingLocal(filePath)) {
                val projectionExpectedText =
                    if (
                        retainedCanonicalProjectionPaths.contains(filePath) &&
                        !TypingTracker.hasUnsyncedOperatorEdits(filePath)
                    ) {
                        editorBufferText(filePath) ?: expectedText
                    } else {
                        expectedText
                    }
                when (
                    queueRemoteTextApply(
                        filePath,
                        projectionExpectedText,
                        targetText,
                        forwarder,
                        appliedRemoteUpdates,
                    )
                ) {
                    RemoteTextApplyDisposition.Queued -> queuedForEditor = true
                    RemoteTextApplyDisposition.Recovered -> usefulWork++
                    RemoteTextApplyDisposition.RetryFailClosed -> {
                        editorBufferText(filePath)?.let { current -> shadows[filePath] = current }
                    }
                }
            }
            usefulWork = peerUpdateCount
        } finally {
            logSlow(
                "remote-drain-file",
                filePath,
                started,
                warnMs = if (usefulWork == 0) CRDT_IDLE_WORKER_WARN_MS else CRDT_WORKER_WARN_MS,
                details = "delivery=$deliveryKind updates=$updateCount peer=$peerUpdateCount self=$selfEchoCount queued=$queuedForEditor",
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
        val remoteState =
            templateStructureState(
                filePath,
                canonical,
                TemplateValidationPlane.Lane.RemoteCandidate,
                "replace-remote",
            )
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
        var unsettledOperatorBuffer: String? = null
        val replicaText = forwarder.replicaText()
        if (!prepareNonOperatorEditorMutationOnWorker(filePath)) {
            log.warn("[crdt-replica] replace delivery retained because native op-capture fencing failed for $filePath")
            scheduleTemplateGuardRecoveryRetry(filePath, "replace-delivery-op-epoch")
            return false
        }
        try {
            ApplicationManager.getApplication().invokeAndWait {
                val edtStarted = System.nanoTime()
                try {
                    val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return@invokeAndWait
                    val document = FileDocumentManager.getInstance().getDocument(targetFile) ?: return@invokeAndWait
                    // The REPLACE analogue of the re-register operator-text hold:
                    // decide on the live buffer BEFORE the clean/unsaved gate. Run
                    // Agent Doc saves the document first, so a REPLACE retained while
                    // the buffer was unsaved used to pass every later gate (shadow ==
                    // buffer == local replica, because the operator's keystrokes are
                    // local CRDT ops) and wipe text the controller never accepted
                    // (lazily.md 2026-10-01 21:42:17: queue edits reverted on Run).
                    val liveBuffer = document.text
                    if (
                        replaceDeliveryWouldClobberUnsettledOperatorTextUtil(
                            settledShadow = settledShadows[filePath] ?: nativeReloadSettledShadows[filePath],
                            bufferText = liveBuffer,
                            canonicalText = canonical,
                        )
                    ) {
                        unsettledOperatorBuffer = liveBuffer
                        return@invokeAndWait
                    }
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
                            before,
                        ).diskPersisted
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
                            before,
                        ).diskPersisted
                        if (installed) {
                            log.info("[crdt-replica] applied and saved REPLACE re-bootstrap for $filePath (${canonical.length} chars)")
                        } else {
                            scheduleTemplateGuardRecoveryRetry(
                                filePath,
                                "replace-delivery-persist-rollback",
                            )
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
        unsettledOperatorBuffer?.let { bufferText ->
            // The canonical in a REPLACE wholesale-replaces the local replica, so
            // the operator ops it lacks would be destroyed, not merged. Re-register
            // from the live buffer instead: registration decides causally from
            // (settled shadow, buffer, canonical) — publish the buffer when
            // canonical is still the settled shadow, adopt canonical only when it
            // provably contains the operator edits, merge forward or hold
            // otherwise. Never retain this canonical for a lazy projection.
            log.warn(
                "[crdt-replica] REPLACE refused: the live buffer holds operator text the controller never " +
                    "accepted for $filePath; re-registering from the buffer instead of projecting canonical over it. " +
                    "buffer_hash=${contentHash(bufferText)} " +
                    "settled_hash=${(settledShadows[filePath] ?: nativeReloadSettledShadows[filePath])?.let(::contentHash) ?: "missing"} " +
                    "canonical_hash=${contentHash(canonical)}",
            )
            retainedCanonicalProjectionPaths.remove(filePath)
            refreshReplicaAfterTransportLoss(
                filePath,
                forwarder,
                bufferText,
                "replace-delivery-unsettled-operator-text",
            )
            return false
        }
        deferredEditorText?.let { editorText ->
            log.warn(
                "[crdt-replica] CP replace retained while the live editor diverges for $filePath; " +
                    "editor_hash=${contentHash(editorText)} canonical_hash=${contentHash(canonical)}",
            )
            retainedCanonicalProjectionPaths.add(filePath)
            requestRemoteDrain(filePath, "replace-delivery-lazy-canonical-projection")
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
        val remoteState =
            templateStructureState(
                filePath,
                converged,
                TemplateValidationPlane.Lane.RemoteCandidate,
                "remote",
            )
        if (remoteState != TemplateStructureProjectionState.Exact) {
            return recoverRejectedRemoteCanonical(
                filePath = filePath,
                expectedText = expectedText,
                remoteText = converged,
                staleForwarder = forwarder,
                remoteState = remoteState,
            )
        }
        // Retain authority before the native op-capture fence. That fence may
        // time out behind a compact/write RPC, but the already-pulled canonical
        // target must still block request-full-state and stale-baseline adoption.
        retainedCanonicalProjectionPaths.add(filePath)
        if (!prepareNonOperatorEditorMutationOnWorker(filePath)) {
            log.warn("[crdt-replica] remote editor apply retained because native op-capture fencing failed for $filePath")
            scheduleTemplateGuardRecoveryRetry(filePath, "remote-editor-apply-op-epoch")
            return RemoteTextApplyDisposition.RetryFailClosed
        }
        remoteEditorApplyPaths.add(filePath)
        val effectCounter =
            remoteEditorEffectGenerations
                .computeIfAbsent(filePath) { AtomicLong(0L) }
        val effectGeneration = effectCounter.get() + 1L
        val outcome = remoteEditorApplies.ingress(
            filePath,
            PendingRemoteEditorApply(
                filePath = filePath,
                expectedText = expectedText,
                targetText = converged,
                effectToken =
                    RemoteEditorEffectToken(
                        generation = effectGeneration,
                        endpoint = forwarder,
                    ),
            ),
        )
        if (outcome != IngressOutcome.Blocked && outcome != IngressOutcome.Dropped) {
            effectCounter.set(effectGeneration)
        }
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
        retainedCanonicalProjectionPaths.add(filePath)
        val editorText = editorBufferText(filePath)
        val editorState =
            editorText?.let {
                templateStructureState(
                    filePath,
                    it,
                    TemplateValidationPlane.Lane.Editor,
                    "template-guard-recovery-editor",
                )
            }
        val decision =
            remoteTemplateProjectionDecisionUtil(
                remoteState = remoteState,
                editorState = editorState,
                editorMatchesExpected = editorText == expectedText,
                recoveryInFlight = templateGuardRecoveryPaths.contains(filePath),
            )
        if (decision != RemoteTemplateProjectionDecision.RecoverEditorBaseline || editorText == null) {
            log.warn(
                "[crdt-replica] rejected malformed remote projection for ${File(filePath).name}; " +
                    "remote_state=$remoteState editor_state=$editorState " +
                    "expected_hash=${contentHash(expectedText)} remote_hash=${contentHash(remoteText)} " +
                    "recovery=bounded-template-guard-retry",
            )
            scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-rejected-remote")
            return RemoteTextApplyDisposition.RetryFailClosed
        }
        if (!templateGuardRecoveryPaths.add(filePath)) {
            scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-recovery-in-flight")
            return RemoteTextApplyDisposition.RetryFailClosed
        }
        try {
            val replacement =
                forwarderFor(
                    filePath = filePath,
                    initialEditorText = editorText,
                    bypassRegisterBackoff = true,
                    replaceCached = true,
                    expectedEditorTextAtSwap = editorText,
                    bootstrapFromControllerCanonical = true,
                )
            if (replacement == null || replacement === staleForwarder) {
                scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-reregister")
                return RemoteTextApplyDisposition.RetryFailClosed
            }
            replacement.ensureEditorText(editorText)
            if (replacement.replicaText() != editorText || editorBufferText(filePath) != editorText) {
                scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-editor-adopt")
                return RemoteTextApplyDisposition.RetryFailClosed
            }
            shadows[filePath] = editorText
            retainedCanonicalProjectionPaths.remove(filePath)
            clearTemplateGuardRecoveryBackoff(filePath)
            if (!projectSettledVisibleState(filePath, replacement, editorText)) {
                scheduleTemplateGuardRecoveryRetry(filePath, "template-guard-visible-proof")
            }
            log.warn(
                "[crdt-replica] rejected malformed remote projection and rebuilt canonical from " +
                    "the unchanged exact editor baseline for ${File(filePath).name}; " +
                    "editor_hash=${contentHash(editorText)} rejected_hash=${contentHash(remoteText)}",
            )
            requestRemoteDrain(filePath, "template-guard-editor-baseline-rebuilt")
            return RemoteTextApplyDisposition.Recovered
        } finally {
            if (!templateGuardRecoveryRetryPaths.contains(filePath)) {
                templateGuardRecoveryPaths.remove(filePath)
            }
        }
    }

    private fun scheduleTemplateGuardRecoveryRetry(filePath: String, reason: String) {
        if (!templateGuardRecoveryRetryPaths.add(filePath)) return
        templateGuardRecoveryPaths.add(filePath)
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
        documentWorkers.forDocument(filePath).schedule(
            {
                templateGuardRecoveryRetryPaths.remove(filePath)
                templateGuardRecoveryPaths.remove(filePath)
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
        val liveEffectGeneration = remoteEditorEffectGenerations[pending.filePath]?.get()
        if (
            !remoteEditorEffectTokenCurrentUtil(
                tokenGeneration = pending.effectToken.generation,
                liveGeneration = liveEffectGeneration,
                endpointMatches = forwarders[pending.filePath] === pending.effectToken.endpoint,
                endpointBacked = pending.effectToken.endpoint.attached,
            )
        ) {
            log.warn(
                "[crdt-replica] remote editor effect refused for ${pending.filePath}; " +
                    "reason=retired_or_superseded_or_model_less " +
                    "token_generation=${pending.effectToken.generation} live_generation=$liveEffectGeneration",
            )
            return completeRemoteEditorApply(
                pending,
                RemoteEditorApplyOutcome(false, null),
                started,
            )
        }
        val outcome = try {
            val targetFile = LocalFileSystem.getInstance().findFileByPath(pending.filePath)
                ?: return completeRemoteEditorApply(pending, RemoteEditorApplyOutcome(false, null), started)
            val document = FileDocumentManager.getInstance().getDocument(targetFile)
                ?: return completeRemoteEditorApply(pending, RemoteEditorApplyOutcome(false, null), started)
            val conflictDecision =
                fileCacheConflicts.observe(
                    pending.filePath,
                    pending =
                        IntelliJFileCacheConflictGuard.hasPending(targetFile) { message, error ->
                            log.warn("[crdt-replica] $message", error)
                        },
                    diskWitness = targetFile.modificationStamp,
                )
            if (conflictDecision.deferMutation) {
                if (conflictDecision.newlyPendingEdge) {
                    log.warn(
                        "[crdt-replica] File Cache Conflict pending for ${pending.filePath}; " +
                            "dropping the stale remote payload without mutating editor or disk",
                    )
                }
                return completeRemoteEditorApply(
                    pending,
                    RemoteEditorApplyOutcome(
                        diskPersisted = false,
                        editorText = document.text,
                        fileCacheConflictDeferred = true,
                    ),
                    started,
                )
            }
            val fileDocumentManager = FileDocumentManager.getInstance()
            val documentWasUnsaved = fileDocumentManager.isDocumentUnsaved(document)
            if (
                !documentWasUnsaved &&
                !refreshCleanDocumentBeforeRemoteApply(pending.filePath, targetFile, document)
            ) {
                return completeRemoteEditorApply(
                    pending,
                    RemoteEditorApplyOutcome(false, document.text),
                    started,
                )
            }
            val before = document.text
            val projectionMode =
                if (documentWasUnsaved) {
                    RemoteCrdtProjectionMode.MemoryOnly
                } else {
                    remoteCrdtProjectionModeUtil(
                        documentUnsaved = false,
                        diskCanPersist =
                            remoteCrdtDiskCanPersistUtil(
                                pending.expectedText,
                                pending.targetText,
                                readRawDiskText(pending.filePath),
                            ),
                    )
                }
            if (before == pending.targetText) {
                shadows[pending.filePath] = pending.targetText
                if (projectionMode == RemoteCrdtProjectionMode.Persist) {
                    val persisted =
                        persistRemoteCrdtTextIfSafe(
                            pending.filePath,
                            document,
                            pending.expectedText,
                            pending.targetText,
                            before,
                        )
                    RemoteEditorApplyOutcome(
                        persisted.diskPersisted,
                        persisted.editorTextForProjection,
                        persisted.editorNormalizedText,
                    )
                } else {
                    RemoteEditorApplyOutcome(false, before)
                }
            } else if (hasPendingLocal(pending.filePath)) {
                RemoteEditorApplyOutcome(false, before)
            } else if (!remoteCrdtApplyStillCurrentUtil(pending.expectedText, before, pending.targetText)) {
                log.warn("[crdt-replica] stale coalesced remote update rejected for ${pending.filePath}; editor text advanced before apply")
                RemoteEditorApplyOutcome(false, before)
            } else if (projectionMode == RemoteCrdtProjectionMode.Reject) {
                log.warn(
                    "[crdt-replica] coalesced remote update rejected because disk contains novel external text for ${pending.filePath}; " +
                        "expected_hash=${contentHash(pending.expectedText)} target_hash=${contentHash(pending.targetText)}"
                )
                RemoteEditorApplyOutcome(false, before)
            } else {
                if (projectionMode == RemoteCrdtProjectionMode.MemoryOnly) {
                    log.debug(
                        "[crdt-replica] projecting remote delta into unsaved editor memory for ${pending.filePath}; " +
                            "durable_sink=crdt_canonical disk_write=deferred",
                    )
                }
                advanceNonOperatorMutationEpoch(pending.filePath)
                applyingRemote.add(pending.filePath)
                try {
                    runUndoableRemoteUpdateCommand(document) {
                        applyMinimalDocumentEditUtil(document, before, pending.targetText)
                        shadows[pending.filePath] = pending.targetText
                    }
                    if (projectionMode == RemoteCrdtProjectionMode.Persist) {
                        val persisted =
                            persistRemoteCrdtTextIfSafe(
                                pending.filePath,
                                document,
                                pending.expectedText,
                                pending.targetText,
                                before,
                            )
                        RemoteEditorApplyOutcome(
                            persisted.diskPersisted,
                            persisted.editorTextForProjection,
                            persisted.editorNormalizedText,
                        )
                    } else {
                        RemoteEditorApplyOutcome(false, pending.targetText)
                    }
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
     * Refreshing an unsaved Document is the inverse hazard: it immediately asks
     * IntelliJ to choose between operator memory and disk. Delta deliveries skip
     * this helper for unsaved buffers, project only into the Document, and leave
     * persistence to the durable CRDT canonical. Replace deliveries still fail
     * closed here because they do not carry merge semantics.
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
        beforeText: String,
    ): RemotePersistOutcome {
        val diskText = readRawDiskText(filePath)
        if (!remoteCrdtDiskCanPersistUtil(expectedText, targetText, diskText)) {
            return reconcileRemotePersistence(
                filePath,
                document,
                beforeText,
                targetText,
                diskText,
            )
        }
        return try {
            val fileDocumentManager = FileDocumentManager.getInstance()
            fileDocumentManager.saveDocument(document)
            val diskAfterSave = readRawDiskText(filePath)
            val outcome = reconcileRemotePersistence(
                filePath,
                document,
                beforeText,
                targetText,
                diskAfterSave,
            )
            if (
                outcome.diskPersisted &&
                fileDocumentManager.isDocumentUnsaved(document)
            ) {
                log.debug(
                    "[crdt-replica] remote editor apply reached exact disk bytes while IntelliJ save state still lagged for $filePath",
                )
            }
            outcome
        } catch (e: RuntimeException) {
            log.warn("[crdt-replica] remote editor apply save failed for $filePath", e)
            reconcileRemotePersistence(
                filePath,
                document,
                beforeText,
                targetText,
                readRawDiskText(filePath),
            )
        }
    }

    /** Save the exact already-visible replica revision through IntelliJ without
     * replacing the Document. Every observation is repeated after the native
     * save so an operator edit or endpoint swap retires the receipt. */
    private fun persistCurrentVisibleRevision(
        filePath: String,
        expectedContentHash: String,
        expectedContentLen: Int,
    ): Boolean {
        fun reject(reason: String, detail: String = ""): Boolean {
            log.warn(
                "[crdt-replica] native persist-current rejected reason=$reason file=$filePath " +
                    "expected_hash=$expectedContentHash expected_len=$expectedContentLen$detail",
            )
            return false
        }

        if (disposed.get()) return reject("manager_disposed")
        val forwarder = forwarders[filePath] ?: return reject("forwarder_missing")
        if (!forwarder.attached) return reject("forwarder_detached")
        val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath)
            ?: return reject("virtual_file_missing")
        val document = FileDocumentManager.getInstance().getDocument(targetFile)
            ?: return reject("document_missing")
        val visibleText = document.text
        val visibleLen = visibleText.toByteArray(Charsets.UTF_8).size
        val visibleHash = contentHash(visibleText)
        val replicaText = forwarder.replicaText()
        val replicaHash = replicaText?.let { contentHash(it) } ?: "missing"
        if (visibleLen != expectedContentLen) {
            requestUrgentRemoteDrain(filePath, "persist-current-visible-length-mismatch")
            scheduleUnforwardedOperatorTextRecovery(filePath, "persist-current-rejected-visible-length-mismatch")
            return reject(
                "visible_length_mismatch",
                " visible_hash=$visibleHash visible_len=$visibleLen replica_hash=$replicaHash",
            )
        }
        if (!visibleHash.equals(expectedContentHash, ignoreCase = true)) {
            requestUrgentRemoteDrain(filePath, "persist-current-visible-hash-mismatch")
            scheduleUnforwardedOperatorTextRecovery(filePath, "persist-current-rejected-visible-hash-mismatch")
            return reject(
                "visible_hash_mismatch",
                " visible_hash=$visibleHash visible_len=$visibleLen replica_hash=$replicaHash",
            )
        }
        if (replicaText != visibleText) {
            requestUrgentRemoteDrain(filePath, "persist-current-visible-mismatch")
            scheduleUnforwardedOperatorTextRecovery(filePath, "persist-current-rejected-visible-mismatch")
            return reject(
                "replica_visible_mismatch",
                " visible_hash=$visibleHash visible_len=$visibleLen replica_hash=$replicaHash",
            )
        }
        return try {
            FileDocumentManager.getInstance().saveDocument(document)
            val diskText = readRawDiskText(filePath)
            val exact =
                forwarders[filePath] === forwarder &&
                forwarder.attached &&
                document.text == visibleText &&
                forwarder.replicaText() == visibleText &&
                diskText == visibleText
            if (!exact) {
                requestUrgentRemoteDrain(filePath, "persist-current-post-save-mismatch")
                return reject(
                    "post_save_projection_mismatch",
                    " visible_hash=${contentHash(document.text)} " +
                        "replica_hash=${forwarder.replicaText()?.let { contentHash(it) } ?: "missing"} " +
                        "disk_hash=${diskText?.let { contentHash(it) } ?: "missing"}",
                )
            }
            documentWorkers.forDocument(filePath).execute {
                if (
                    !disposed.get() &&
                    forwarders[filePath] === forwarder &&
                    forwarder.attached &&
                    editorBufferText(filePath) == visibleText &&
                    forwarder.replicaText() == visibleText &&
                    readRawDiskText(filePath) == visibleText &&
                            !projectSettledVisibleState(filePath, forwarder, visibleText, true)
                ) {
                    requestRemoteDrain(filePath, "persist-current-receipt-retry")
                }
            }
            true
        } catch (failure: RuntimeException) {
            log.warn("[crdt-replica] native persist-current failed for $filePath", failure)
            false
        }
    }

    /**
     * A native save may return before IntelliJ's File Cache Conflict UI is
     * resolved. Treat the later VFS write as the completion edge, but publish a
     * persistence receipt only when disk, the live editor, and the same replica
     * generation still contain identical bytes. The editor remains authority;
     * an external disk value is never loaded or merged here.
     */
    private fun projectNativeSaveReceipt(filePath: String) {
        val forwarder = forwarders[filePath] ?: return
        if (!forwarder.attached) return
        val visibleText = editorBufferText(filePath) ?: return
        try {
            documentWorkers.forDocument(filePath).execute {
                if (
                    disposed.get() ||
                    forwarders[filePath] !== forwarder ||
                    !forwarder.attached ||
                    editorBufferText(filePath) != visibleText ||
                    forwarder.replicaText() != visibleText ||
                    readRawDiskText(filePath) != visibleText
                ) {
                    return@execute
                }
                if (!projectSettledVisibleState(filePath, forwarder, visibleText, true)) {
                    requestRemoteDrain(filePath, "native-save-receipt-retry")
                }
            }
        } catch (_: RejectedExecutionException) {
            // Disposal owns the lane; no stale receipt may escape afterward.
        }
    }

    private fun reconcileRemotePersistence(
        filePath: String,
        document: Document,
        beforeText: String,
        targetText: String,
        diskAfterSave: String?,
    ): RemotePersistOutcome {
        return when (
            remotePersistReconciliationUtil(
                beforeText,
                targetText,
                document.text,
                diskAfterSave,
            )
        ) {
            RemotePersistReconciliation.Persisted -> {
                shadows[filePath] = targetText
                RemotePersistOutcome(true, targetText)
            }
            RemotePersistReconciliation.PersistedEditorNormalization -> {
                val normalizedText = document.text
                shadows[filePath] = normalizedText
                log.info(
                    "[crdt-replica] editor save normalized a remote projection for $filePath; " +
                        "target_hash=${contentHash(targetText)} normalized_hash=${contentHash(normalizedText)} " +
                        "disk_hash=${diskAfterSave?.let(::contentHash) ?: "missing"} " +
                        "recovery=project_normalized_editor_text",
                )
                RemotePersistOutcome(
                    diskPersisted = true,
                    editorTextForProjection = normalizedText,
                    editorNormalizedText = normalizedText,
                )
            }
            RemotePersistReconciliation.RollbackToBefore -> {
                log.warn(
                    "[crdt-replica] remote editor apply did not persist; rolling the exact attempted projection back for $filePath: " +
                        "before_hash=${contentHash(beforeText)} target_hash=${contentHash(targetText)} " +
                        "editor_hash=${contentHash(document.text)} disk_hash=${diskAfterSave?.let(::contentHash) ?: "missing"}",
                )
                if (document.text == targetText) {
                    runUndoableRemoteUpdateCommand(document) {
                        applyMinimalDocumentEditUtil(document, targetText, beforeText)
                    }
                }
                shadows[filePath] = beforeText
                if (document.text == beforeText && diskAfterSave == beforeText) {
                    FileDocumentManager.getInstance().reloadFromDisk(document)
                }
                RemotePersistOutcome(false, beforeText)
            }
            RemotePersistReconciliation.PreserveAdvancedEditor -> {
                val editorText = document.text
                shadows[filePath] = editorText
                log.warn(
                    "[crdt-replica] remote editor persistence diverged from both exact planes; preserving the advanced editor and withholding disk-persisted projection for $filePath: " +
                        "before_hash=${contentHash(beforeText)} target_hash=${contentHash(targetText)} " +
                        "editor_hash=${contentHash(editorText)} disk_hash=${diskAfterSave?.let(::contentHash) ?: "missing"} " +
                        "document_unsaved=${FileDocumentManager.getInstance().isDocumentUnsaved(document)} " +
                        "pending_local=${hasPendingLocal(filePath)}",
                )
                RemotePersistOutcome(false, null)
            }
        }
    }

    private fun completeRemoteEditorApply(
        pending: PendingRemoteEditorApply,
        outcome: RemoteEditorApplyOutcome,
        started: Long,
    ) {
        val projectionVisible =
            !outcome.fileCacheConflictDeferred &&
                shouldProjectVisibleRemoteDeliveryUtil(
                    outcome.editorText,
                    pending.targetText,
                    outcome.diskPersisted,
                )
        logSlow(
            "remote-apply-edt",
            pending.filePath,
            started,
            warnMs = CRDT_EDT_WARN_MS,
            details = "target_chars=${pending.targetText.length} visible=$projectionVisible disk_persisted=${outcome.diskPersisted}",
        )
        if (disposed.get()) return
        try {
            documentWorkers.forDocument(pending.filePath).execute {
                var projectionPublished = false
                try {
                    val normalizedText = outcome.editorNormalizedText
                    if (normalizedText != null) {
                        retainedCanonicalProjectionPaths.add(pending.filePath)
                        log.info(
                            "[crdt-replica] editor normalization retained for controller reprojection for " +
                                "${File(pending.filePath).name}; normalized_hash=${contentHash(normalizedText)}",
                        )
                    }
                    projectionPublished =
                        projectionVisible &&
                            outcome.editorText?.let { visibleText ->
                            projectSettledVisibleState(
                                pending.filePath,
                                pending.effectToken.endpoint,
                                visibleText,
                                outcome.diskPersisted,
                            )
                            } == true
                    log.debug(
                        "[crdt-replica] remote editor apply completed for ${File(pending.filePath).name}; " +
                            "visible=$projectionVisible disk_persisted=${outcome.diskPersisted} projection_published=$projectionPublished",
                    )
                    if (!projectionPublished && projectionVisible) {
                        requestRemoteDrain(
                            pending.filePath,
                            "remote-editor-state-projection-retry",
                        )
                    }
                } finally {
                    remoteEditorApplyPaths.remove(pending.filePath)
                    if (outcome.fileCacheConflictDeferred) {
                        // The current remote payload was derived before IntelliJ's
                        // operator-owned conflict decision. Do not retain or poll it;
                        // the next real editor/VFS/controller edge will pull current
                        // canonical state after the conflict clears.
                        retainedCanonicalProjectionPaths.remove(pending.filePath)
                    } else if (projectionPublished && outcome.editorNormalizedText == null) {
                        retainedCanonicalProjectionPaths.remove(pending.filePath)
                        consecutiveNoOpReschedules.set(0)
                    } else if (outcome.editorNormalizedText != null) {
                        requestRemoteDrain(
                            pending.filePath,
                            "remote-editor-normalization-canonical-reproject",
                        )
                    } else {
                        val delayMs = nextNoOpRescheduleBackoffMs()
                        log.debug(
                            "[crdt-replica] remote editor projection not yet visible for ${File(pending.filePath).name}; " +
                                "backing off retry by ${delayMs}ms",
                        )
                        scheduleRemoteDrainAfterBackoff(delayMs, pending.filePath)
                    }
                }
            }
        } catch (_: RejectedExecutionException) {
            remoteEditorApplyPaths.remove(pending.filePath)
            if (projectionVisible) {
                retainedCanonicalProjectionPaths.remove(pending.filePath)
            }
        }
    }

    private fun editorReplicaBaselineMatches(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
        expectedText: String,
        updates: List<ReplicaRemoteUpdate>,
    ): Boolean {
        val editorText = editorBufferText(filePath) ?: return false
        val replicaText = forwarder.replicaText()
        val editorState =
            templateStructureState(
                filePath,
                editorText,
                TemplateValidationPlane.Lane.Editor,
                "editor-baseline",
            )
        val editorHash = contentHash(editorText)
        val replicaHash = replicaText?.let(::contentHash)
        val editorRemoteGeneration = matchingRemoteTargetGenerationUtil(updates, editorHash)
        val replicaRemoteGeneration = matchingRemoteTargetGenerationUtil(updates, replicaHash)
        val canonicalProjectionRetained = retainedCanonicalProjectionPaths.contains(filePath)
        val recoveryInFlight = templateGuardRecoveryPaths.contains(filePath)
        val decision = replicaBaselineDecisionUtil(
            editorState = editorState,
            editorMatchesExpected = editorText == expectedText,
            replicaMatchesExpected = replicaText == expectedText,
            replicaMatchesEditor = replicaText == editorText,
            editorMatchesRemoteTarget = editorRemoteGeneration != null,
            replicaMatchesRemoteTarget = replicaRemoteGeneration != null,
            recoveryInFlight = recoveryInFlight,
            canonicalProjectionRetained = canonicalProjectionRetained,
        )
        if (
            decision == ReplicaBaselineDecision.ApplyRemote ||
            decision == ReplicaBaselineDecision.ApplyRemoteRepair
        ) {
            if (decision == ReplicaBaselineDecision.ApplyRemoteRepair) {
                log.warn(
                    "[crdt-replica] applying a structurally exact remote correction over the exact expected editor baseline for $filePath: " +
                        "editor_state=$editorState editor_hash=${contentHash(editorText)} expected_hash=${contentHash(expectedText)}",
                )
            }
            return true
        }
        val expectedHash = contentHash(expectedText)
        val visibleReplicaHash = replicaHash ?: "missing"
        if (
            decision == ReplicaBaselineDecision.ProjectRemoteTarget &&
            editorRemoteGeneration != null
        ) {
            shadows[filePath] = editorText
                    if (!projectSettledVisibleState(filePath, forwarder, editorText)) {
                        requestRemoteDrain(filePath, "already-visible-state-projection-retry")
                    }
            log.info(
                "[crdt-replica] projected an already-visible remote target for $filePath: " +
                    "editor_hash=$editorHash generation=$editorRemoteGeneration",
            )
            return false
        }
        if (
            decision == ReplicaBaselineDecision.RebootstrapVisibleRemoteTarget &&
            editorRemoteGeneration != null
        ) {
            val replacement = forwarderFor(
                filePath = filePath,
                initialEditorText = editorText,
                bypassRegisterBackoff = true,
                replaceCached = true,
                expectedEditorTextAtSwap = editorText,
                bootstrapFromControllerCanonical = true,
                expectedCanonicalTextAtSwap = editorText,
            )
            if (replacement == null || replacement === forwarder) {
                scheduleTemplateGuardRecoveryRetry(
                    filePath,
                    "visible-target-canonical-rebootstrap-retry",
                )
                return false
            }
            val replacementText = replacement.replicaText()
            if (replacementText != editorText) {
                retainedCanonicalProjectionPaths.add(filePath)
                log.info(
                    "[crdt-replica] visible remote target advanced during canonical rebootstrap for $filePath: " +
                        "editor_hash=$editorHash replacement_hash=${replacementText?.let(::contentHash) ?: "missing"} " +
                        "generation=$editorRemoteGeneration",
                )
                scheduleTemplateGuardRecoveryRetry(filePath, "visible-target-canonical-advanced")
                return false
            }
            shadows[filePath] = editorText
                    if (!projectSettledVisibleState(filePath, replacement, editorText)) {
                        retainedCanonicalProjectionPaths.add(filePath)
                        scheduleTemplateGuardRecoveryRetry(filePath, "visible-target-projection-retry")
                        return false
            }
            retainedCanonicalProjectionPaths.remove(filePath)
            log.info(
                "[crdt-replica] acknowledged an exact visible remote target after canonical replica rebootstrap for $filePath: " +
                    "editor_hash=$editorHash generation=$editorRemoteGeneration",
            )
            return false
        }
        if (
            decision == ReplicaBaselineDecision.ReplayRemoteTarget &&
            replicaText != null &&
            replicaRemoteGeneration != null
        ) {
            val replayedUpdates =
                updates.filter { it.generation <= replicaRemoteGeneration }
            val replayExpectedText =
                if (
                    canonicalProjectionRetained &&
                    !TypingTracker.hasUnsyncedOperatorEdits(filePath)
                ) {
                    editorText
                } else {
                    expectedText
                }
            log.info(
                "[crdt-replica] replaying a native remote target over its exact editor baseline for $filePath: " +
                    "editor_hash=$editorHash replica_hash=$visibleReplicaHash generation=$replicaRemoteGeneration " +
                    "retained_canonical=$canonicalProjectionRetained",
            )
            queueRemoteTextApply(
                filePath,
                replayExpectedText,
                replicaText,
                forwarder,
                replayedUpdates,
            )
            return false
        }
        if (decision == ReplicaBaselineDecision.RealignShadow) {
            log.warn(
                "[crdt-replica] incoming update deferred after shadow realignment for $filePath: " +
                    "editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$visibleReplicaHash"
            )
            shadows[filePath] = editorText
            requestRemoteDrain(filePath, "shadow-realigned")
            return false
        }
        if (decision == ReplicaBaselineDecision.RetryFailClosed && recoveryInFlight) {
            scheduleTemplateGuardRecoveryRetry(filePath, "baseline-recovery-in-flight")
            return false
        }
        log.warn(
            "[crdt-replica] incoming update retained for lazy canonical projection because the baselines diverged for $filePath: " +
                "editor_state=$editorState editor_hash=$editorHash expected_hash=$expectedHash replica_hash=$visibleReplicaHash",
        )
        retainedCanonicalProjectionPaths.add(filePath)
        requestRemoteDrain(filePath, "baseline-diverged-lazy-canonical-projection")
        return false
    }

    private fun editorBufferText(filePath: String): String? {
        val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return null
        val application = ApplicationManager.getApplication()
        if (application.isReadAccessAllowed) {
            return FileDocumentManager.getInstance().getDocument(targetFile)?.text
        }
        if (SwingUtilities.isEventDispatchThread()) {
            // `#edt-no-implicit-read`: take the read action explicitly; the EDT
            // does not imply read access on modern IntelliJ platforms. Workers
            // still use the non-blocking attempt below.
            return ReadAction.compute<String?, RuntimeException> {
                FileDocumentManager.getInstance().getDocument(targetFile)?.text
            }
        }
        val applicationEx = application as? ApplicationEx ?: return null
        val text = AtomicReference<String?>()
        return if (
            applicationEx.tryRunReadAction {
                text.set(FileDocumentManager.getInstance().getDocument(targetFile)?.text)
            }
        ) {
            text.get()
        } else {
            null
        }
    }

    /**
     * Native/replica workers must never queue indefinitely behind IDEA's
     * write-intent permit: they use ApplicationEx's immediate read attempt and let
     * their retained event retry when a writer has priority.
     *
     * `#edt-no-implicit-read`: an EDT caller does **not** already own safe editor
     * access. Older platforms gave the EDT an implicit read lock, which is why
     * `isEventDispatchThread()` used to stand in for one; modern platforms require
     * an explicit read (or write-intent) action and assert otherwise. An EDT caller
     * therefore takes a real read action here — blocking is correct on the EDT
     * because write actions also run there, so no writer can hold the lock.
     */
    private fun tryReadDocumentText(document: Document): String? {
        val application = ApplicationManager.getApplication()
        if (application.isReadAccessAllowed) {
            return document.text
        }
        if (SwingUtilities.isEventDispatchThread()) {
            return ReadAction.compute<String?, RuntimeException> { document.text }
        }
        val applicationEx = application as? ApplicationEx ?: return null
        val text = AtomicReference<String?>()
        return if (applicationEx.tryRunReadAction { text.set(document.text) }) {
            text.get()
        } else {
            null
        }
    }

    private fun contentHash(text: String): String =
        java.security.MessageDigest.getInstance("SHA-256")
            .digest(text.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }

    private fun templateStructureState(
        filePath: String,
        text: String,
        lane: TemplateValidationPlane.Lane,
        source: String,
    ): TemplateStructureProjectionState {
        val started = System.nanoTime()
        return try {
            templateValidation.publish(filePath, lane, text).state.also { state ->
                if (state != TemplateStructureProjectionState.Exact) {
                    log.warn(
                        "[crdt-replica] $source text rejected by template-structure guard for $filePath; " +
                            "state=$state lane=$lane revision_hash=${contentHash(text)}",
                    )
                }
            }
        } finally {
            logSlow(
                "template-validation-computed",
                filePath,
                started,
                details = "source=$source lane=$lane target_chars=${text.length}",
            )
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
        bootstrapFromControllerCanonical: Boolean = false,
        expectedCanonicalTextAtSwap: String? = null,
        deferCanonicalProjectionForPendingLocal: Boolean = false,
        bypassRetainedProjectionHold: Boolean = false,
    ): CrdtReplicaForwarder? {
        val cached = forwarders[filePath]
        // A three-generation reconciliation hold is a real registration
        // failure, even when a controller event asks for a forced refresh.
        // Honor its exponential backoff so missing-replica notifications do
        // not turn the fail-closed decision into a register/deregister storm.
        val retainedProjectionHeld = retainedProjectionHoldPaths.contains(filePath)
        val retainedProjectionRetryDue =
            !retainedProjectionHeld || shouldAttemptRegister(filePath)
        if (
            !bypassRetainedProjectionHold &&
            !retainedProjectionHoldAllowsRefreshUtil(
                retainedProjectionHeld,
                retainedProjectionRetryDue,
            )
        ) {
            return cached
        }
        if (bypassRegisterBackoff) {
            // Bypass only the retry deadline. The existing projection remains
            // authoritative until transport registration and retained-state
            // reconciliation both commit below.
        } else if (!replaceCached) {
            cached?.let { return it }
        }
        if (!bypassRegisterBackoff && !shouldAttemptRegister(filePath)) return cached
        if (
            expectedEditorTextAtSwap != null &&
            (editorBufferText(filePath) != expectedEditorTextAtSwap ||
                (!allowPendingLocalAtSwap && hasPendingLocal(filePath)))
        ) return cached
        val root = resolveProjectRoot(filePath) ?: return null
        // Allocate from the plugin-lifetime epoch even for an initial attach.
        // CrdtReplicaManager is recreated during native reload, so an instance-local
        // counter (or the bare editor/path identity) can collide with a retiring
        // manager whose deregistration is still in flight.
        val identity = EditorIdentity.nextReplicaConnectionIdentity(filePath)
        val retainedResumeState =
            if (bootstrapFromControllerCanonical) {
                null
            } else {
                cached?.captureResumeState() ?: nativeReloadResumeStates[filePath]
            }
        val forwarder = CrdtReplicaForwarder(
            filePath = filePath,
            identity = identity,
            node = NativeReplicaNode(),
            transport = CpSocketReplicaTransport(root),
            ownershipContext = ownershipContext,
            resumeState = retainedResumeState,
            expectedCanonicalHash = expectedCanonicalTextAtSwap?.let(::contentHash),
            // Registration is not commitment. Keep the prior relay membership
            // alive until every live-buffer and retained-projection check below
            // has accepted this candidate.
            provisionalReplacement = replaceCached && cached != null,
        )
        if (!forwarder.register()) {
            recordRegisterFailure(filePath, forwarder.lastRegisterFailureReason ?: "controller-register")
            forwarder.lastRegisterFailureReason?.let { attachFailureReasons[filePath] = it }
            if (cached != null) {
                log.warn("[crdt-replica] replacement register failed for ${File(filePath).name}; retained cached forwarder")
            }
            return null
        }
        if (
            expectedCanonicalTextAtSwap != null &&
            forwarder.replicaText() != expectedCanonicalTextAtSwap
        ) {
            // The captured operator delta is only meaningful relative to its
            // exact base. Keep the cached endpoint and retry; never derive a
            // replacement edit from a different controller generation.
            forwarder.deregister()
            return null
        }
        val publishedShadowAtRegistration =
            settledShadows[filePath] ?: nativeReloadSettledShadows[filePath]
        val bufferTextAtRegistration = editorBufferText(filePath) ?: initialEditorText
        val retainedProjectionAction =
            retainedRegistrationProjectionActionForAttachUtil(
                deferCanonicalProjectionForPendingLocal = deferCanonicalProjectionForPendingLocal,
                canonicalProjectionRetained = forwarder.canonicalProjectionRetained,
                retainedReplicaReseedPending = forwarder.retainedReplicaReseedPending,
                canonicalCoversRetainedFrontier = forwarder.canonicalCoversRetainedFrontier,
                publishedShadow = publishedShadowAtRegistration,
                bufferText = bufferTextAtRegistration,
                canonicalText = forwarder.replicaText(),
                cleanMergeAvailable =
                    cleanRegistrationMerge(
                        publishedShadowAtRegistration,
                        bufferTextAtRegistration,
                        forwarder.replicaText(),
                    ) != null,
                canonicalContainsOperatorEdits =
                    canonicalContainsOperatorEditsAtRegistration(
                        filePath = filePath,
                        canonicalProjectionRetained = forwarder.canonicalProjectionRetained,
                        publishedShadow = publishedShadowAtRegistration,
                        bufferText = bufferTextAtRegistration,
                        canonicalText = forwarder.replicaText(),
                    ),
            )
        if (forwarder.canonicalProjectionRetained || forwarder.retainedReplicaReseedPending) {
            // Controller state survives an IDEA/plugin restart; this local set
            // does not. Restore the fail-closed baseline before any whole-editor
            // synchronization can publish a stale restarted buffer.
            //
            // `#retainedprojectionclobbersoperatortext`: that reasoning holds for
            // a RESTARTED buffer and inverts for a live one. When registration is
            // refused for hours (see `#registeridentitypid`) the operator keeps
            // typing into a buffer that reaches nothing, and a retained projection
            // computed before those keystrokes then adopts over them on attach.
            // Measured 2026-08-12: a compaction computed at 04:14:47 against disk
            // was retained undelivered, and at 04:23 it replaced a buffer holding a
            // prompt typed in between. The prompt existed only in that buffer, so
            // it was not in git, not in the CRDT, and not in the compaction
            // archive — it was simply gone.
            //
            // The settled shadow separates the two cases exactly: unlike the
            // incremental editing shadow, it advances only after the controller
            // accepts the visible projection and survives a native-generation
            // reload in JVM memory. A restarted IDE has none, so that path is
            // unchanged. A live IDE whose buffer moved past its settled shadow
            // holds operator text that a replacement controller may not have
            // seen, and adopting canonical over it is data loss, not recovery.
            // The live buffer, not the caller's captured cut: this decides whether
            // operator text is about to be destroyed, so it must read what the
            // operator can currently see.
            when (retainedProjectionAction) {
                RetainedRegistrationProjectionAction.ApplyCanonical -> {
                    retainedCanonicalProjectionPaths.add(filePath)
                    log.info(
                        "[crdt-replica] registration retained the controller canonical projection for " +
                            "${File(filePath).name}; canonical_hash=${forwarder.canonicalContentHash ?: "unknown"}",
                    )
                }

                RetainedRegistrationProjectionAction.PublishOperatorBuffer -> {
                    log.warn(
                        "[crdt-replica] registration retained the exact published shadow for " +
                            "${File(filePath).name} while the live buffer holds unpublished operator text; " +
                            "publishing the buffer from that proven base. " +
                            "shadow_hash=${contentHash(publishedShadowAtRegistration!!)} " +
                            "buffer_hash=${contentHash(bufferTextAtRegistration!!)} " +
                            "canonical_hash=${forwarder.canonicalContentHash ?: "unknown"}",
                    )
                    retainedCanonicalProjectionPaths.remove(filePath)
                }

                RetainedRegistrationProjectionAction.HoldOperatorBuffer -> {
                    // There is no proven rebase: either three distinct generations
                    // exist, or an IDE restart discarded the settled shadow needed
                    // to compare the live editor with the retained canonical. Refuse
                    // before the swap; this registration may not choose a generation
                    // by overwriting another.
                    log.warn(
                        "[crdt-replica] refusing ambiguous retained projection for ${File(filePath).name}; " +
                            "the live operator buffer was not derived from this canonical generation. " +
                            "shadow_hash=${publishedShadowAtRegistration?.let(::contentHash) ?: "unavailable"} " +
                            "buffer_hash=${bufferTextAtRegistration?.let(::contentHash) ?: "unavailable"} " +
                            "canonical_hash=${forwarder.canonicalContentHash ?: "unknown"}",
                    )
                    forwarder.deregister()
                    retainedProjectionHoldPaths.add(filePath)
                    recordRegisterFailure(
                        filePath,
                        retainedRegistrationHoldReasonUtil(
                            retainedReplicaReseedPending = forwarder.retainedReplicaReseedPending,
                            publishedShadow = publishedShadowAtRegistration,
                        ),
                    )
                    return cached
                }

                RetainedRegistrationProjectionAction.DeferCanonicalProjection -> {
                    log.info(
                        "[crdt-replica] deferred retained controller projection for ${File(filePath).name}; " +
                            "a captured local delta owns the visible buffer",
                    )
                }

                RetainedRegistrationProjectionAction.MergeForward -> {
                    log.warn(
                        "[crdt-replica] registration merging the live operator buffer forward onto the " +
                            "controller canonical for ${File(filePath).name}; " +
                            "shadow_hash=${contentHash(publishedShadowAtRegistration!!)} " +
                            "buffer_hash=${contentHash(bufferTextAtRegistration!!)} " +
                            "canonical_hash=${forwarder.canonicalContentHash ?: "unknown"}",
                    )
                    retainedCanonicalProjectionPaths.remove(filePath)
                }
            }
        } else if (retainedProjectionAction == RetainedRegistrationProjectionAction.HoldOperatorBuffer) {
            // `#reloadclobbersoperatortext`: the ordinary re-register analogue of the
            // retained ambiguity hold. The buffer stays exactly as the operator left it.
            log.warn(
                "[crdt-replica] refusing re-register projection for ${File(filePath).name}; " +
                    "the live buffer holds operator text typed after its settled shadow. " +
                    "shadow_hash=${contentHash(publishedShadowAtRegistration!!)} " +
                    "buffer_hash=${contentHash(bufferTextAtRegistration!!)} " +
                    "canonical_hash=${forwarder.canonicalContentHash ?: "unknown"}",
            )
            forwarder.deregister()
            retainedProjectionHoldPaths.add(filePath)
            recordRegisterFailure(filePath, "ambiguous-reregister-projection")
            return cached
        } else if (retainedProjectionAction == RetainedRegistrationProjectionAction.PublishOperatorBuffer) {
            log.warn(
                "[crdt-replica] re-register found unpublished operator text for ${File(filePath).name}; " +
                    "publishing the buffer from its settled shadow instead of projecting canonical over it. " +
                    "shadow_hash=${contentHash(publishedShadowAtRegistration!!)} " +
                    "buffer_hash=${contentHash(bufferTextAtRegistration!!)}",
            )
        }
        if (bootstrapFromControllerCanonical) {
            nativeReloadResumeStates.remove(filePath)
        } else if (retainedResumeState != null) {
            nativeReloadResumeStates.remove(filePath, retainedResumeState)
        }
        if (
            expectedEditorTextAtSwap != null &&
            (editorBufferText(filePath) != expectedEditorTextAtSwap ||
                (!allowPendingLocalAtSwap && hasPendingLocal(filePath)))
        ) {
            // Registration can block while the operator keeps typing. Reject
            // the captured editor cut before swapping the endpoint; the
            // still-authoritative cached forwarder will drain those local
            // deltas and a later refresh can retry from a fresh cut.
            forwarder.deregister()
            return null
        }
        if (
            expectedEditorTextAtSwap != null &&
            (editorBufferText(filePath) != expectedEditorTextAtSwap ||
                (!allowPendingLocalAtSwap && hasPendingLocal(filePath)))
        ) {
            // Recheck at the actual swap so a raced local delta keeps the cached
            // forwarder authoritative until the serialized local-delta worker
            // has drained it.
            forwarder.deregister()
            return null
        }
        if (replaceCached && cached != null) {
            if (forwarders.replace(filePath, cached, forwarder)) {
                log.info(
                    "[crdt-replica] atomically replaced cached forwarder for ${File(filePath).name}",
                )
                if (
                    !finalizeRegistrationProjection(
                        filePath = filePath,
                        forwarder = forwarder,
                        action = retainedProjectionAction,
                        publishedShadow = publishedShadowAtRegistration,
                        bufferText = bufferTextAtRegistration,
                    )
                ) {
                    forwarders.replace(filePath, forwarder, cached)
                    forwarder.deregister()
                    return cached
                }
                if (!forwarder.promoteReplacement()) {
                    // The controller canonical moved after bootstrap, or the
                    // provisional candidate disappeared. Restore the still-live
                    // predecessor and retry from a fresh cut.
                    forwarders.replace(filePath, forwarder, cached)
                    forwarder.deregister()
                    recordRegisterFailure(filePath, "replacement-promotion")
                    return cached
                }
                cached.deregister()
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
        if (
            !finalizeRegistrationProjection(
                filePath = filePath,
                forwarder = forwarder,
                action = retainedProjectionAction,
                publishedShadow = publishedShadowAtRegistration,
                bufferText = bufferTextAtRegistration,
            )
        ) {
            forwarders.remove(filePath, forwarder)
            forwarder.deregister()
            return null
        }
        return forwarder
    }

    /**
     * `#editorauth2`: the clean three-way merge of a registration's generations,
     * or null when any is missing, they need no merge, the reconciler is
     * unavailable, or the merge has conflicts (those stay held, never overwritten).
     */
    private fun cleanRegistrationMerge(shadow: String?, buffer: String?, canonical: String?): String? {
        if (shadow == null || buffer == null || canonical == null) return null
        if (buffer == canonical || buffer == shadow || canonical == shadow) return null
        val json = NativePatching.reconcileText(shadow, buffer, canonical, -1)
        val reconciled =
            json?.let {
                try {
                    val root = com.google.gson.JsonParser.parseString(it).asJsonObject
                    if (!root.get("ok").asBoolean || root.get("conflicts").asInt != 0) {
                        null
                    } else {
                        root.get("text").asString
                    }
                } catch (e: Exception) {
                    log.warn("[crdt-replica] registration merge result unreadable: ${e.message}")
                    null
                }
            }
        // `#steerreplicachurn`: a line merge conflicts when canonical already
        // holds the first part of a prompt the operator kept typing (its push
        // was ingested but the receipt was lost, then a native reload dropped
        // the captured splice stream). The native rebase merges exactly that
        // case by inserting only what canonical lacks, and refuses any overlap
        // with controller text, so the hold still covers every real conflict.
        return reconciled
            ?: NativePatching.rebaseCapturedSplices(shadow, canonical, singleSpliceBatchUtil(shadow, buffer))
                ?.resultingText
    }

    /** Complete the causal projection decision only after this endpoint owns the map slot. */
    private fun finalizeRegistrationProjection(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
        action: RetainedRegistrationProjectionAction,
        publishedShadow: String?,
        bufferText: String?,
    ): Boolean {
        val committed =
            when (action) {
            RetainedRegistrationProjectionAction.ApplyCanonical -> {
                retainCanonicalProjectionAfterRegistration(filePath, forwarder)
                true
            }

            RetainedRegistrationProjectionAction.PublishOperatorBuffer -> {
                // Recheck all three facts at the publication edge. Registration
                // may block while the operator continues typing; only the exact
                // captured buffer may be emitted from the exact captured base.
                if (
                    publishedShadow == null ||
                    bufferText == null ||
                    (settledShadows[filePath] ?: nativeReloadSettledShadows[filePath]) !=
                        publishedShadow ||
                    editorBufferText(filePath) != bufferText ||
                    (!forwarder.retainedReplicaReseedPending &&
                        forwarder.replicaText() != publishedShadow)
                ) {
                    log.warn(
                        "[crdt-replica] retained projection publication raced for ${File(filePath).name}; " +
                            "leaving the operator buffer untouched and retrying registration",
                    )
                    false
                } else {
                    if (!forwarder.ensureEditorText(bufferText) || forwarder.replicaText() != bufferText) {
                        false
                    } else {
                        shadows[filePath] = bufferText
                        fenceCapturedEditsSubsumedByPublishedBuffer(
                            filePath,
                            bufferText,
                            "publish-operator-buffer",
                        )
                        retainedCanonicalProjectionPaths.remove(filePath)
                    if (!projectSettledVisibleState(filePath, forwarder, bufferText)) {
                        requestRemoteDrain(filePath, "registration-operator-buffer-projection-retry")
                        }
                        log.info(
                            "[crdt-replica] published retained live buffer for ${File(filePath).name}; " +
                                "buffer_hash=${contentHash(bufferText)} driver=proven-shadow-local-delta",
                        )
                        true
                    }
                }
            }

            RetainedRegistrationProjectionAction.HoldOperatorBuffer -> false

            RetainedRegistrationProjectionAction.DeferCanonicalProjection -> true

            RetainedRegistrationProjectionAction.MergeForward -> {
                // Recheck the captured generations at the publication edge, then
                // recompute the merge from exactly those bytes.
                val canonical = forwarder.replicaText()
                val merged =
                    if (
                        publishedShadow == null ||
                        bufferText == null ||
                        canonical == null ||
                        (settledShadows[filePath] ?: nativeReloadSettledShadows[filePath]) !=
                            publishedShadow ||
                        editorBufferText(filePath) != bufferText
                    ) {
                        null
                    } else {
                        cleanRegistrationMerge(publishedShadow, bufferText, canonical)
                    }
                if (merged == null || !forwarder.ensureEditorText(merged) || forwarder.replicaText() != merged) {
                    log.warn(
                        "[crdt-replica] registration merge-forward raced for ${File(filePath).name}; " +
                            "leaving the operator buffer untouched and retrying registration",
                    )
                    false
                } else {
                    fenceEveryCapturedEdit(filePath, "registration-merge-forward")
                    retainedCanonicalProjectionPaths.remove(filePath)
                    // The replica now holds the merge and has published it; project
                    // it into the buffer through the generation-fenced apply, which
                    // refuses if the operator typed since `bufferText`.
                    queueRemoteTextApply(filePath, bufferText!!, merged, forwarder, emptyList())
                    log.info(
                        "[crdt-replica] merged live buffer forward for ${File(filePath).name}; " +
                            "merged_hash=${contentHash(merged)} driver=registration-merge-forward",
                    )
                    true
                }
            }
        }
        if (committed) {
            // A controller transport registration is provisional until its
            // retained projection has committed. Clearing earlier resets an
            // ambiguous hold to failure_count=1 and recreates a one-second
            // register/deregister/pull loop.
            clearRegisterFailure(filePath)
        }
        return committed
    }

    /**
     * Registration is controller -> editor projection only. A pre-existing
     * editor buffer cannot become a whole-document recovery baseline because it
     * may predate retained queue additions or another editor's projected ops.
     */
    private fun retainCanonicalProjectionAfterRegistration(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
    ) {
        val canonical = forwarder.replicaText() ?: return
        shadows[filePath] = canonical
        val visibleText = editorBufferText(filePath)
        if (visibleText == null) {
            retainedCanonicalProjectionPaths.add(filePath)
            requestRemoteDrain(filePath, "registration-visible-observation-pending")
            return
        }
        if (visibleText == canonical) {
            // Registration proves which canonical generation the replacement
            // replica opened, but not what the IDE buffer currently displays.
            // Publish the exact post-swap view so the controller can discharge
            // this generation's delivery receipt.
            if (!projectSettledVisibleState(filePath, forwarder, visibleText)) {
                requestRemoteDrain(filePath, "registration-visible-projection-retry")
            }
            return
        }
        retainedCanonicalProjectionPaths.add(filePath)
        log.info(
            "[crdt-replica] projecting controller bootstrap into ${File(filePath).name}; " +
            "editor_hash=${contentHash(visibleText)} canonical_hash=${contentHash(canonical)} " +
                "driver=lazy-controller-canonical-projection",
        )
        queueRemoteTextApply(
            filePath = filePath,
        expectedText = visibleText,
            converged = canonical,
            forwarder = forwarder,
            updates = emptyList(),
        )
    }

    /** Record the causal frontier only after the controller accepts visibility. */
    private fun projectSettledVisibleState(
        filePath: String,
        forwarder: CrdtReplicaForwarder,
        visibleText: String,
        diskPersisted: Boolean = false,
    ): Boolean {
        val projected = forwarder.projectVisibleState(visibleText, diskPersisted)
        if (projected) {
            settledShadows[filePath] = visibleText
            nativeReloadSettledShadows.remove(filePath)
        }
        return projected
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
            // `#ctrlkillreregister` Tier 3: this file recovered by noticing its own
            // loss, but a controller that lost its hub stranded every registration
            // this editor holds — including documents nothing is currently draining,
            // which would otherwise wait for an operator to touch them. Ask once,
            // about ourselves, and repair the rest now.
            pullMissingReplicas(project, "controller-transport-recovered")
        } else {
            log.debug(
                "[crdt-replica] controller transport unavailable for ${File(filePath).name}; " +
                    "reason=$reason",
            )
        }
    }

    private fun shouldAttemptRegister(filePath: String): Boolean {
        val projection = registerRetryProjections[filePath] ?: return true
        val now = System.currentTimeMillis()
        if (replicaRegistrationAttemptDueUtil(projection, now)) return true
        log.debug(
            "[crdt-replica] register skipped for $filePath; " +
                "retry_after_ms=${projection.retryAfterMs - now}",
        )
        return false
    }

    private fun recordRegisterFailure(filePath: String, reason: String = "controller-register") {
        // Registration is the source of truth for operator-facing attach
        // diagnostics. Retry state used to remember the backoff but discard the
        // cause, so Compact Exchange could only say "could not be attached".
        attachFailureReasons[filePath] = reason
        val now = System.currentTimeMillis()
        val projection =
            registerRetryProjections.compute(filePath) { _, previous ->
                nextReplicaRegistrationRetryProjection(previous, now)
            }!!
        registerFailureCounts[filePath] = projection.failureCount
        registerRetryAfterMs[filePath] = projection.retryAfterMs
        log.warn(
            "[crdt-replica] register failed for ${File(filePath).name}; " +
                "reason=$reason failure_count=${projection.failureCount} " +
                "retry_backoff_ms=${projection.backoffMs}",
        )
        // `#rundocdispatchrobust`: a retired generation's retry loop re-registered
        // src/haiven-dev documents every 30s under its retired identity after the
        // 2026-09-29 18:12:38 reload. Only the live generation retries.
        if (PluginGeneration.retired) return
        if (registerFailureNeedsLivenessRepublishUtil(reason)) {
            // The owning controller never received this generation's open report, so it
            // still names another generation as the live endpoint. Republish, then retry
            // promptly instead of waiting out a backoff that cannot change the answer.
            ReliableSyncLivenessListener.republishDocument(project, filePath)
            scheduleRegisterRetry(filePath, minOf(projection.backoffMs, STALE_ENDPOINT_REGISTER_RETRY_MS))
            return
        }
        scheduleRegisterRetry(filePath, projection.backoffMs)
    }

    private fun clearRegisterFailure(filePath: String) {
        attachFailureReasons.remove(filePath)
        retainedProjectionHoldPaths.remove(filePath)
        registerFailureCounts.remove(filePath)
        registerRetryAfterMs.remove(filePath)
        registerRetryProjections.remove(filePath)
        registerRetryTasks.remove(filePath)?.cancel(false)
    }

    /**
     * Retain desired open-document registration across controller/native startup
     * races. The VFS event is only the observation edge; this keyed retry owns
     * convergence until the open document has an attached replica.
     */
    private fun scheduleRegisterRetry(filePath: String, delayMs: Long) {
        if (disposed.get() || PluginGeneration.retired) return
        registerRetryTasks.compute(filePath) { _, existing ->
            if (existing != null && !existing.isDone) {
                existing
            } else {
                executor.schedule(
                    {
                        registerRetryTasks.remove(filePath)
                        if (
                            PluginGeneration.retired ||
                            disposed.get() ||
                            project.isDisposed ||
                        (forwarders[filePath]?.attached == true &&
                            !retainedProjectionHoldPaths.contains(filePath))
                        ) {
                            return@schedule
                        }
                        runOnEdtNonBlocking {
                            if (disposed.get() || project.isDisposed) return@runOnEdtNonBlocking
                            val file =
                                FileEditorManager.getInstance(project).openFiles
                                    .firstOrNull { it.path == filePath }
                                    ?: return@runOnEdtNonBlocking
                            val document =
                                FileDocumentManager.getInstance().getDocument(file)
                                    ?: return@runOnEdtNonBlocking
                    ensureOpenDocumentReplica(
                        file.path,
                        document,
                        await = false,
                        forceRefresh = retainedProjectionHoldPaths.contains(filePath),
                    )
                        }
                    },
                    delayMs,
                    TimeUnit.MILLISECONDS,
                )
            }
        }
    }

    /**
     * A whole buffer was just published from the exact captured cut, so every
     * splice captured before it is already in the replica and must not replay
     * (`#subsumedsplicereplay`). A splice typed WHILE the publication was in
     * flight is not in it, though, and retiring it too (the old blanket epoch
     * advance) dropped the operator's text and let the next splice validate
     * against a shadow that lacked it (`retainedtargetdropsedit`).
     *
     * Under one read action — no DocumentEvent can interleave, because they are
     * delivered inside write actions — keep exactly the captured suffix the
     * published cut does not contain, re-stamped to the current epoch. Edits are
     * then forwarded from the published buffer like any other batch.
     */
    private fun fenceCapturedEditsSubsumedByPublishedBuffer(
        filePath: String,
        publishedText: String,
        reason: String,
    ) {
        var owedCount = -1
        val fenced =
            withEditorCaptureCut(filePath) { visible ->
                val epoch = nonOperatorMutationEpoch(filePath)
                pendingLocalEditorEdits.compute(filePath) { _, existing ->
                    val owed =
                        capturedEditsOwedAfterPublishedCutUtil(
                            published = publishedText,
                            visible = visible,
                            captured = existing.orEmpty(),
                            epoch = epoch,
                        )
                    owedCount = owed.size
                    owed.takeIf { it.isNotEmpty() }?.toMutableList()
                }
                true
            } == true
        if (!fenced) {
            // No consistent cut was readable (a write action kept the read lock
            // busy). Fall back to retiring every captured splice; the
            // unforwarded-text self-heal rolls any raced keystroke forward from
            // the editor once the buffer is observable again.
            val epoch = advanceNonOperatorMutationEpoch(filePath)
            log.warn(
                "[crdt-replica] fenced every captured splice for ${File(filePath).name} without a " +
                    "consistent editor cut; epoch=$epoch reason=$reason recovery=unforwarded-operator-text",
            )
            scheduleUnforwardedOperatorTextRecovery(filePath, "publication-fence-without-cut")
            return
        }
        if (owedCount > 0) {
            log.warn(
                "[crdt-replica] kept $owedCount splice(s) typed during the buffer publication for " +
                    "${File(filePath).name}; published_hash=${contentHash(publishedText)} reason=$reason",
            )
            scheduleLocalEditorFlush(filePath)
        } else {
            log.info(
                "[crdt-replica] fenced splices subsumed by the published buffer for ${File(filePath).name}; " +
                    "reason=$reason",
            )
        }
    }

    /**
     * Merge-forward publishes `merge(shadow, buffer, canonical)`, not the buffer,
     * and projects it back through the generation-fenced remote apply, which
     * refuses if the operator typed since the captured buffer. Raced splices are
     * relative to that buffer, not to the replica, so they cannot be forwarded
     * here; retire them all and let the refused apply re-register from the buffer.
     */
    private fun fenceEveryCapturedEdit(filePath: String, reason: String) {
        val epoch = advanceNonOperatorMutationEpoch(filePath)
        log.info(
            "[crdt-replica] fenced splices subsumed by the published buffer for ${File(filePath).name}; " +
                "epoch=$epoch reason=$reason",
        )
    }

    /**
     * Run [block] with the live editor text under a read action, so the text and
     * [pendingLocalEditorEdits] form one consistent cut. Non-blocking attempts
     * only: a worker must never wait on the read lock while the EDT may be
     * waiting on this worker.
     *
     * `#capturecutreadaction`: the Document lookup itself
     * (`FileDocumentManager.getDocument`) is a model read that asserts read
     * access, so it belongs INSIDE the read action together with the text read.
     * Resolving it before the read action threw `Read access is allowed from
     * inside read-action only` on the replica executor (registration →
     * `finalizeRegistrationProjection` → `fenceCapturedEditsSubsumedByPublishedBuffer`).
     */
    private fun <T> withEditorCaptureCut(filePath: String, block: (String) -> T): T? {
        val targetFile = LocalFileSystem.getInstance().findFileByPath(filePath) ?: return null
        val readCut: () -> T? = {
            FileDocumentManager.getInstance().getDocument(targetFile)?.let { document -> block(document.text) }
        }
        val application = ApplicationManager.getApplication()
        if (application.isReadAccessAllowed) return readCut()
        if (SwingUtilities.isEventDispatchThread()) {
            return ReadAction.compute<T?, RuntimeException> { readCut() }
        }
        val applicationEx = application as? ApplicationEx ?: return null
        val result = AtomicReference<T?>()
        repeat(EDITOR_CAPTURE_CUT_ATTEMPTS) { attempt ->
            if (applicationEx.tryRunReadAction { result.set(readCut()) }) {
                return result.get()
            }
            if (attempt + 1 < EDITOR_CAPTURE_CUT_ATTEMPTS) {
                try {
                    Thread.sleep(EDITOR_CAPTURE_CUT_RETRY_MS)
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                    return null
                }
            }
        }
        return null
    }

    /**
     * `retainedtargetdropsedit` self-heal. A replica that lost an operator splice
     * never converges by itself: the controller keeps asking the editor to save
     * the replica's text, the editor rightly refuses because its visible buffer
     * holds more, and nothing reconciles until a controller restart reseeds from
     * the editor. When the capture chain is provably broken (shadow == replica,
     * no splice outstanding, no remote apply in flight, buffer != shadow) and the
     * same divergence is observed twice, roll the visible text forward onto the
     * replica as a local splice — operator-visible text is authoritative.
     */
    private fun scheduleUnforwardedOperatorTextRecovery(filePath: String, reason: String) {
        try {
            documentWorkers.forDocument(filePath).execute {
                if (!disposed.get()) recoverUnforwardedOperatorText(filePath, reason)
            }
        } catch (error: RejectedExecutionException) {
            if (!disposed.get()) {
                log.warn("[crdt-replica] unforwarded-text recovery scheduling rejected for $filePath", error)
            }
        }
    }

    private fun recoverUnforwardedOperatorText(filePath: String, reason: String) {
        val forwarder = forwarders[filePath]
        if (
            forwarder == null ||
            !forwarder.attached ||
            hasPendingLocal(filePath) ||
            isApplyingRemote(filePath) ||
            remoteEditorApplyPaths.contains(filePath) ||
            retainedCanonicalProjectionPaths.contains(filePath) ||
            retainedProjectionHoldPaths.contains(filePath)
        ) {
            unforwardedOperatorTextObservations.remove(filePath)
            return
        }
        val shadow = shadows[filePath]
        val replica = forwarder.replicaText()
        var recovered: CapturedLocalEditorEdit? = null
        withEditorCaptureCut(filePath) { visible ->
            val epoch = nonOperatorMutationEpoch(filePath)
            val splice =
                unforwardedOperatorTextSpliceUtil(
                    shadow = shadow,
                    replica = replica,
                    visible = visible,
                    captured = pendingLocalEditorEdits[filePath].orEmpty(),
                    epoch = epoch,
                )
            if (splice == null) {
                unforwardedOperatorTextObservations.remove(filePath)
                return@withEditorCaptureCut
            }
            val observation = "${contentHash(shadow!!)}:${contentHash(visible)}:$epoch"
            if (unforwardedOperatorTextObservations.put(filePath, observation) != observation) {
                // First sighting: a remote apply or registration may still be
                // settling. Look again shortly; act only on a stable divergence.
                return@withEditorCaptureCut
            }
            unforwardedOperatorTextObservations.remove(filePath)
            pendingLocalEditorEdits.compute(filePath) { _, existing ->
                (existing ?: mutableListOf()).also { it.add(splice) }
            }
            recovered = splice
        }
        val splice = recovered
        if (splice == null) {
            if (unforwardedOperatorTextObservations.containsKey(filePath)) {
                scheduleUnforwardedOperatorTextRecoveryAfter(filePath, reason)
            }
            return
        }
        log.warn(
            "[crdt-replica] rolled unforwarded operator text forward onto the replica for " +
                "${File(filePath).name}; replica_hash=${contentHash(replica!!)} " +
                "removed_chars=${splice.oldFragment.length} inserted_chars=${splice.newFragment.length} " +
                "reason=$reason recovery=editor-text-authoritative",
        )
        scheduleLocalEditorFlush(filePath)
    }

    private fun scheduleUnforwardedOperatorTextRecoveryAfter(filePath: String, reason: String) {
        try {
            documentWorkers.forDocument(filePath).schedule(
                Runnable {
                    if (!disposed.get()) recoverUnforwardedOperatorText(filePath, reason)
                },
                UNFORWARDED_OPERATOR_TEXT_CONFIRM_MS,
                TimeUnit.MILLISECONDS,
            )
        } catch (error: RejectedExecutionException) {
            if (!disposed.get()) {
                log.warn("[crdt-replica] unforwarded-text recovery confirm rejected for $filePath", error)
            }
        }
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
        if (crdtPerfWarns(elapsedMs, warnMs, SwingUtilities.isEventDispatchThread())) {
            log.warn(message)
        } else {
            log.debug(message)
        }
    }

    /**
     * Serialize a native-generation checkpoint behind each document lane's
     * accepted work. Returning null is a hard refusal to unload the generation:
     * an attached replica without encoded state cannot be reconstructed exactly.
     */
    private fun captureNativeReloadResumeStates(
        deadlineNanos: Long,
    ): Map<String, ReplicaResumeState>? {
        val pending =
            forwarders.entries
                .filter { (_, forwarder) -> forwarder.attached }
                .associate { (filePath, forwarder) ->
                    filePath to documentWorkers.forDocument(filePath).submit<ReplicaResumeState?> {
                        if (forwarders[filePath] !== forwarder) {
                            null
                        } else {
                            forwarder.captureResumeState()
                        }
                    }
                }
        val captured = LinkedHashMap<String, ReplicaResumeState>()
        for ((filePath, future) in pending) {
            val remainingMs =
                nativeReloadRemainingWaitMillis(deadlineNanos, System.nanoTime())
            if (remainingMs == null) {
                pending.values.forEach { it.cancel(true) }
                log.warn(
                    "[crdt-replica] native reload checkpoint timed out before ${File(filePath).name}; " +
                        "retaining old generation",
                )
                return null
            }
            val state =
                try {
                    future.get(remainingMs, TimeUnit.MILLISECONDS)
                } catch (_: TimeoutException) {
                    null
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                    null
                } catch (error: Exception) {
                    log.warn(
                        "[crdt-replica] native reload checkpoint failed for ${File(filePath).name}",
                        error,
                    )
                    null
                }
            if (state == null) {
                pending.values.forEach { it.cancel(true) }
                log.warn(
                    "[crdt-replica] native reload checkpoint unavailable for ${File(filePath).name}; " +
                        "retaining old generation",
                )
                return null
            }
            captured[filePath] = state
        }
        return captured
    }

    companion object {
        private val instances = ConcurrentHashMap<Project, CrdtReplicaManager>()
        private val applyingAgentMutations = ConcurrentHashMap.newKeySet<String>()
        private val nonOperatorMutationEpochs = ConcurrentHashMap<String, AtomicLong>()
        private val nativeReloadResumeStates =
            ConcurrentHashMap<String, ReplicaResumeState>()
        private val nativeReloadSettledShadows = ConcurrentHashMap<String, String>()

        fun getInstance(project: Project): CrdtReplicaManager {
            // `#pluginunloadresurrect`: a retired classloader must not start a
            // manager; it would register replicas under a retired identity.
            check(!PluginGeneration.retired) {
                "agent-doc plugin generation was unloaded; refusing to start a CRDT replica manager"
            }
            return instances.getOrPut(project) {
                CrdtReplicaManager(project).also { it.start() }
            }
        }

        fun disposeProject(project: Project) {
            instances.remove(project)?.dispose()
        }

        internal fun quiesceAllForNativeReload(): NativeReloadReplicaHandoff {
            val managers = instances.entries.map { (project, manager) -> project to manager }
            managers.forEach { (_, manager) -> manager.disposed.set(true) }

            // Capture on each document's serialized lane BEFORE stopping or disposing
            // anything. The previous aggregate `if (quiesced)` capture skipped every
            // document when one busy lane missed the worker deadline, but disposal
            // dropped all replicas anyway. A native generation may now be retired only
            // after every attached forwarder has a JVM-owned encoded checkpoint.
            val captureDeadlineNanos =
                System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(NATIVE_RELOAD_WORKER_TIMEOUT_MS)
            val capturedByProject = LinkedHashMap<Project, Map<String, ReplicaResumeState>>()
            for ((project, manager) in managers) {
                val captured = manager.captureNativeReloadResumeStates(captureDeadlineNanos)
                if (captured == null) {
                    managers.forEach { (_, activeManager) -> activeManager.disposed.set(false) }
                    return NativeReloadReplicaHandoff(emptyMap(), reloadSafe = false, replicasTornDown = false)
                }
                capturedByProject[project] = captured
            }

            capturedByProject.values.forEach { captured ->
                captured.forEach { (filePath, state) ->
                    nativeReloadResumeStates[filePath] = state
                }
            }
            managers.forEach { (project, manager) ->
                manager.settledShadows.forEach { (filePath, settled) ->
                    nativeReloadSettledShadows[filePath] = settled
                }
                instances.remove(project, manager)
                manager.executor.shutdownNow()
                manager.documentWorkers.shutdownNow()
            }
            // One native handoff owns one bounded outage. Giving every project
            // its own five-second wait made the no-manager interval grow with
            // the number of open projects (seven minutes in a live workspace).
            val deadlineNanos = System.nanoTime() +
                TimeUnit.MILLISECONDS.toNanos(NATIVE_RELOAD_WORKER_TIMEOUT_MS)
            val quiesced = managers.all { (_, manager) ->
                val remainingMs = nativeReloadRemainingWaitMillis(
                    deadlineNanos,
                    System.nanoTime(),
                ) ?: return@all false
                manager.awaitWorkerTermination(remainingMs)
            }
            managers.forEach { (_, manager) -> manager.dispose() }
            return NativeReloadReplicaHandoff(
                projectDocuments = capturedByProject.mapValues { (_, captured) -> captured.keys },
                reloadSafe = quiesced,
            )
        }

        internal fun restartAfterNativeReload(
            handoff: NativeReloadReplicaHandoff,
            liveProjects: Collection<Project> = emptyList(),
        ): NativeReloadReplicaRestartReport {
            val targets = mutableListOf<Triple<CrdtReplicaManager, String, Document>>()
            val collectTargets = {
                (handoff.projectDocuments.keys + liveProjects)
                    .distinct()
                    .filterNot { it.isDisposed }
                    .forEach { project ->
                        val manager = getInstance(project)
                        val fileDocumentManager = FileDocumentManager.getInstance()
                        FileEditorManager.getInstance(project).openFiles
                            .asSequence()
                            .filter { it.name.endsWith(".md") }
                            .forEach { file ->
                                val document = fileDocumentManager.getDocument(file) ?: return@forEach
                                if (isAgentDocDocumentTextUtil(document.text)) {
                                    targets.add(Triple(manager, file.path, document))
                                }
                            }
                    }
            }
            if (SwingUtilities.isEventDispatchThread()) {
                collectTargets()
            } else {
                ApplicationManager.getApplication().invokeAndWait(collectTargets)
            }

            val expectedPaths = targets.map { it.second }.toSortedSet()
            val attachedPaths = linkedSetOf<String>()
            targets.forEach { (manager, filePath, document) ->
                manager.log.info(
                    "[crdt-replica] awaiting native-generation re-register for ${File(filePath).name}",
                )
                if (
                    manager.ensureOpenDocumentReplica(
                        filePath,
                        document,
                        await = true,
                        forceRefresh = true,
                    )
                ) {
                    attachedPaths.add(filePath)
                }
            }
            return nativeReloadReplicaRestartReport(
                expectedPaths,
                attachedPaths,
                liveProjects = (handoff.projectDocuments.keys + liveProjects)
                    .distinct()
                    .count { !it.isDisposed },
            )
        }

        fun requestRemoteDrain(project: Project, filePath: String? = null, reason: String = "event") {
            val manager = filePath?.let { managerForFile(project, it) } ?: instances[project]
            manager?.requestRemoteDrain(filePath, reason)
        }

        fun requestUrgentRemoteDrain(project: Project, filePath: String, reason: String) {
            managerForFile(project, filePath)?.requestUrgentRemoteDrain(filePath, reason)
        }

        /**
         * #jbmanagerbyfile: the replica manager that serves [filePath] for a request that
         * arrived through [project]'s socket handler. The native socket is per IDE process,
         * so the handler's project need not be the one holding the file's replica -- with
         * two projects open, keying by the handler's project answered every
         * `persist_current` and `editor_replica_reregister` with a silent `false`
         * (2026-10-01: the retained save for agent-doc-bugs.md was refused for epochs 7-33,
         * and preflight refused every later cycle). Prefer the manager that holds the
         * file's replica, then the project's own manager, then the deepest owning project.
         */
        private fun managerForFile(project: Project, filePath: String): CrdtReplicaManager? {
            val projectManager = if (project.isDisposed) null else instances[project]
            val live = instances.values.filter { !it.disposed.get() }
            val chosen = selectReplicaManagerUtil(
                candidates = live,
                preferred = projectManager?.takeUnless { it.disposed.get() },
                holdsReplica = { it.forwarders.containsKey(filePath) },
            ) ?: managerForFilePath(filePath)
            if (chosen != null && chosen !== projectManager) {
                chosen.log.info(
                    "[crdt-replica] routed $filePath to the manager holding its replica " +
                        "(project=${chosen.project.name}, via=${project.name})",
                )
            }
            return chosen
        }

        fun persistCurrentVisibleRevision(
            project: Project,
            filePath: String,
            expectedContentHash: String,
            expectedContentLen: Int,
            // `#netadv5` R2: set when `false` is a document-lane timeout. The
            // submitted save is not cancelled and may still land, so the receipt
            // is "deferred", never a definitive save refusal.
            pendingOut: AtomicBoolean? = null,
        ): Boolean {
            val manager = managerForFile(project, filePath) ?: run {
                com.intellij.openapi.diagnostic.Logger.getInstance(CrdtReplicaManager::class.java).warn(
                    "[crdt-replica] native persist-current rejected reason=no_replica_manager " +
                        "file=$filePath expected_hash=$expectedContentHash expected_len=$expectedContentLen",
                )
                return false
            }
            if (SwingUtilities.isEventDispatchThread()) {
                manager.log.warn(
                    "[crdt-replica] native persist-current rejected reason=edt_cannot_wait_for_document_lane " +
                        "file=$filePath expected_hash=$expectedContentHash expected_len=$expectedContentLen",
                )
                return false
            }
            return try {
                // Native save is an operation on the same per-document lane as
                // accepted local and remote CRDT updates. FIFO submission makes
                // the save observe every update accepted before this command.
                manager.documentWorkers.forDocument(filePath).submit<Boolean> {
                    val result = AtomicBoolean(false)
                    ApplicationManager.getApplication().invokeAndWait {
                        result.set(
                            manager.persistCurrentVisibleRevision(
                                filePath,
                                expectedContentHash,
                                expectedContentLen,
                            ),
                        )
                    }
                    result.get()
                }.get(CRDT_AWAIT_PERSIST_CURRENT_TIMEOUT_MS, TimeUnit.MILLISECONDS)
            } catch (_: TimeoutException) {
                pendingOut?.set(true)
                manager.log.warn(
                    "[crdt-replica] native persist-current deferred reason=document_lane_timeout " +
                        "file=$filePath expected_hash=$expectedContentHash expected_len=$expectedContentLen " +
                        "timeout_ms=$CRDT_AWAIT_PERSIST_CURRENT_TIMEOUT_MS",
                )
                false
            } catch (_: RejectedExecutionException) {
                manager.log.warn(
                    "[crdt-replica] native persist-current rejected reason=document_lane_unavailable " +
                        "file=$filePath expected_hash=$expectedContentHash expected_len=$expectedContentLen",
                )
                false
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                manager.log.warn(
                    "[crdt-replica] native persist-current rejected reason=document_lane_interrupted " +
                        "file=$filePath expected_hash=$expectedContentHash expected_len=$expectedContentLen",
                )
                false
            } catch (failure: RuntimeException) {
                manager.log.warn(
                    "[crdt-replica] native persist-current rejected reason=document_lane_failure " +
                        "file=$filePath expected_hash=$expectedContentHash expected_len=$expectedContentLen",
                    failure,
                )
                false
            }
        }

        /**
         * `#ctrlkillreregister` Tier 3 — ask the controller which of this editor's
         * registrations it holds no replica for, and rebuild exactly those.
         *
         * Replaces the blind "force-refresh every open markdown document" sweep at
         * startup and after a transport recovery. The sweep was both too much and too
         * little: it drops and rebuilds healthy CRDT baselines (the lossiest thing
         * this manager can do), while still missing a registration whose document is
         * not currently open in a tab.
         *
         * `held` is deliberately EMPTY. This editor's own forwarder map is the wrong
         * evidence: after a controller kill the forwarders still look live here, and
         * passing them as held would suppress precisely the documents that need
         * repair. The controller subtracts what its process-local hub can actually
         * serve, which is the only fact that separates "registered" from "registered
         * and backed". Re-register storms are already bounded by
         * [beginProjectionRecoveryReregister]'s coalescing window.
         *
         * A null answer means the question could not be asked (old cdylib, controller
         * unreachable). Only then does this fall back to the compatibility sweep, so
         * an editor is never left stranded by the pull's own unavailability.
         */
        fun pullMissingReplicas(project: Project, reason: String) {
            val manager = instances[project] ?: return
            val root = project.basePath ?: return
            if (!manager.beginPeerReplicaPull()) {
                manager.log.debug("[crdt-replica] coalesced peer replica pull; reason=$reason")
                return
            }
            ApplicationManager.getApplication().executeOnPooledThread {
                if (project.isDisposed) return@executeOnPooledThread
                val pid = ProcessHandle.current().pid()
                val json = PeerReplicaPull.missingRegistrationsJson(root, pid, emptyList())
                val paths = PeerReplicaPull.rebuildPaths(json, pid)
                if (paths == null) {
                    manager.log.info(
                        "[crdt-replica] peer replica pull unavailable; reason=$reason " +
                            "recovery=controller_tier1_fan_out",
                    )
                    forceRefreshOpenDocumentReplicas(project, "$reason-pull-unavailable")
                    return@executeOnPooledThread
                }
                if (paths.isEmpty()) {
                    manager.log.info("[crdt-replica] peer replica pull found nothing to rebuild; reason=$reason")
                    return@executeOnPooledThread
                }
                manager.log.info(
                    "[crdt-replica] peer replica pull names ${paths.size} stranded registration(s); reason=$reason",
                )
                paths.forEach { filePath ->
                    forceRefreshOpenDocumentReplica(project, filePath, "$reason-peer-replica-pull")
                }
            }
        }

        /**
         * Observe every currently open markdown document without replacing a
         * healthy replica. Missing registrations retain their own bounded retry.
         */
        fun ensureOpenDocumentReplicas(project: Project, reason: String) {
            runOnEdtNonBlocking {
                if (project.isDisposed) return@runOnEdtNonBlocking
                val manager = instances[project] ?: return@runOnEdtNonBlocking
                val fileDocumentManager = FileDocumentManager.getInstance()
                FileEditorManager.getInstance(project).openFiles
                    .asSequence()
                    .filter { it.name.endsWith(".md") }
                    .forEach { file ->
                        val document = fileDocumentManager.getDocument(file) ?: return@forEach
                        manager.log.debug(
                            "[crdt-replica] ensuring open-document registration for ${file.name}; reason=$reason",
                        )
                        manager.ensureOpenDocumentReplica(
                            file.path,
                            document,
                            await = false,
                            forceRefresh = false,
                        )
                    }
            }
        }

        /**
         * Reattach every open agent-doc document and report which replicas the controller
         * accepted. Dynamic plugin load keeps editor tabs open, so
         * [ProjectManagerListener.projectOpened] is never replayed for the replacement
         * classloader and the package installer asks for this receipt afterwards.
         *
         * `#jbupgradereattach`: the report is a receipt, not a verdict on the upgrade.
         * A pending path is not evidence that the replacement failed -- it means one
         * document has not re-registered with the controller that owns ITS OWN project
         * root. That convergence is gated by facts this upgrade does not own: whether the
         * other project's controller is live, the terminal non-agent-doc refusal, and the
         * per-document register backoff that `forceRefresh = false` deliberately honors.
         * So a document can stay pending for the entire wait while the plugin bytes are
         * correct and live, and each pending path keeps its own bounded retry armed.
         * Raising here aborted `make install` after the replacement had already landed.
         */
        internal fun ensureOpenDocumentReplicasAndWait(
            project: Project,
            reason: String,
        ): NativeReloadReplicaRestartReport {
            check(!SwingUtilities.isEventDispatchThread()) {
                "open-document replica receipts must be awaited off the EDT"
            }
            val manager = instances[project]
                ?: throw IllegalStateException("CRDT replica manager was not initialized")
            val targets = mutableListOf<Triple<String, Document, String>>()
            ApplicationManager.getApplication().invokeAndWait {
                if (project.isDisposed) return@invokeAndWait
                val fileDocumentManager = FileDocumentManager.getInstance()
                FileEditorManager.getInstance(project).openFiles
                    .asSequence()
                    .filter { it.name.endsWith(".md") }
                    .forEach { file ->
                        val document = fileDocumentManager.getDocument(file) ?: return@forEach
                        val text = document.text
                        if (isAgentDocDocumentTextUtil(text)) {
                            targets.add(Triple(file.path, document, text))
                        }
                    }
            }
            val attachedPaths = linkedSetOf<String>()
            targets.forEach { (filePath, document, text) ->
                manager.log.info(
                    "[crdt-replica] awaiting dynamic-plugin-load registration for ${File(filePath).name}; reason=$reason",
                )
                val accepted = manager.ensureOpenDocumentReplica(
                        filePath,
                        document,
                        editorText = text,
                        await = true,
                        forceRefresh = false,
                    )
                if (!accepted) {
                    // The ordinary 750ms request timeout protects interactive recovery,
                    // but a large controller bootstrap can legitimately outlive it. The
                    // original task remains queued and publishes the authoritative
                    // `attached` receipt; wait for that task instead of submitting a
                    // duplicate registration or falsely failing the package upgrade.
                    val deadline = System.nanoTime() +
                        TimeUnit.MILLISECONDS.toNanos(DYNAMIC_PLUGIN_ATTACH_RECEIPT_TIMEOUT_MS)
                    while (
                        manager.forwarders[filePath]?.attached != true &&
                        System.nanoTime() < deadline
                    ) {
                        try {
                            Thread.sleep(25L)
                        } catch (_: InterruptedException) {
                            Thread.currentThread().interrupt()
                            break
                        }
                    }
                }
                if (manager.forwarders[filePath]?.attached == true) {
                    attachedPaths.add(filePath)
                }
            }
            return nativeReloadReplicaRestartReport(
                targets.map { it.first },
                attachedPaths,
                liveProjects = if (project.isDisposed) 0 else 1,
            )
        }

        fun ensureOpenDocumentReplica(project: Project, filePath: String, reason: String) {
            runOnEdtNonBlocking {
                if (project.isDisposed) return@runOnEdtNonBlocking
                val manager = instances[project] ?: return@runOnEdtNonBlocking
                val file =
                    FileEditorManager.getInstance(project).openFiles
                        .firstOrNull { it.path == filePath }
                        ?: return@runOnEdtNonBlocking
                val document =
                    FileDocumentManager.getInstance().getDocument(file)
                        ?: return@runOnEdtNonBlocking
                manager.log.debug(
                    "[crdt-replica] ensuring open-document registration for ${file.name}; reason=$reason",
                )
                manager.ensureOpenDocumentReplica(
                    file.path,
                    document,
                    await = false,
                    forceRefresh = false,
                )
            }
        }

        fun rebindOpenDocumentPath(
            project: Project,
            oldPath: String,
            newPath: String,
        ): Boolean {
            if (project.isDisposed) return false
            val manager = instances[project] ?: return false
            val documentRef = AtomicReference<Document?>()
            val textRef = AtomicReference<String?>()
            val capture = {
                val file =
                    FileEditorManager.getInstance(project).openFiles
                        .firstOrNull { it.path == newPath }
                        ?: LocalFileSystem.getInstance().findFileByPath(newPath)
                val document = file?.let { FileDocumentManager.getInstance().getDocument(it) }
                documentRef.set(document)
                textRef.set(document?.text)
            }
            if (SwingUtilities.isEventDispatchThread()) {
                capture()
            } else {
                ApplicationManager.getApplication().invokeAndWait { capture() }
            }
            val document = documentRef.get() ?: return false
            val editorText = textRef.get() ?: return false
            return manager.rebindOpenDocumentPath(oldPath, newPath, document, editorText)
        }

        fun forceRefreshOpenDocumentReplicas(project: Project, reason: String) {
            runOnEdtNonBlocking {
                if (project.isDisposed) return@runOnEdtNonBlocking
                val manager = instances[project] ?: return@runOnEdtNonBlocking
                val fileDocumentManager = FileDocumentManager.getInstance()
                val openDocuments =
                    FileEditorManager.getInstance(project).openFiles
                        .asSequence()
                        .filter { it.name.endsWith(".md") }
                        .mapNotNull { file ->
                            fileDocumentManager.getDocument(file)?.let { document ->
                                Triple(file.path, file.name, document)
                            }
                        }
                        .toList()
                ApplicationManager.getApplication().executeOnPooledThread {
                    if (project.isDisposed) return@executeOnPooledThread
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

        /**
         * Close the Rust-owned operator-op epoch before an editor projection is
         * handed to the EDT. The native generation proxy deliberately rejects
         * calls from the event-dispatch thread, so this durable transition and
         * the local editor mutation are a two-stage handoff.
         *
         * This operation is idempotent. Returning false keeps the projection
         * retained for retry instead of applying text without its causal fence.
         */
        fun prepareNonOperatorEditorMutationOnWorker(filePath: String): Boolean {
            check(!javax.swing.SwingUtilities.isEventDispatchThread()) {
                "native op-capture fencing must complete before dispatching an editor mutation to the EDT"
            }
            val lib = AgentDocLib.get() ?: return true
            return lib.agent_doc_clear_editor_op_epoch(filePath) == 1
        }

        fun forceRefreshOpenDocumentReplica(project: Project, filePath: String, reason: String) {
            runOnEdtNonBlocking {
                if (project.isDisposed) return@runOnEdtNonBlocking
                val manager = instances[project] ?: return@runOnEdtNonBlocking
                val file =
                    LocalFileSystem.getInstance().findFileByPath(filePath)
                        ?: return@runOnEdtNonBlocking
                val document =
                    FileDocumentManager.getInstance().getDocument(file)
                        ?: return@runOnEdtNonBlocking
                val resolvedFilePath = file.path
                val fileName = file.name
                ApplicationManager.getApplication().executeOnPooledThread {
                    if (project.isDisposed) return@executeOnPooledThread
                    if (!manager.beginProjectionRecoveryReregister(resolvedFilePath)) {
                        manager.log.info(
                            "[crdt-replica] coalesced projection-recovery re-register for $fileName reason=$reason",
                        )
                        manager.requestUrgentRemoteDrain(
                            resolvedFilePath,
                            "projection-recovery-reregister-coalesced",
                        )
                        return@executeOnPooledThread
                    }
                    manager.log.info(
                        "[crdt-replica] forcing projection-recovery re-register for $fileName reason=$reason",
                    )
                    manager.ensureOpenDocumentReplica(
                        resolvedFilePath,
                        document,
                        await = false,
                        forceRefresh = true,
                    )
                }
            }
        }

        /**
         * Re-register one editor-owned replica and return only after the native
         * controller accepted the replacement member.
         *
         * The typed `editor_replica_reregister` IPC uses its reply as a recovery
         * receipt. The ordinary refresh entrypoint above intentionally remains
         * asynchronous for controller wakeups and native-generation handoff, but
         * returning from this method before [ensureOpenDocumentReplica] completed
         * would let the controller spend its bounded observation window against
         * work that had only been queued.
         */
        fun refreshOpenDocumentReplicaForRecoveryAndWait(
            project: Project,
            filePath: String,
            reason: String,
            // `#netadv5` R2: set when `false` means "slow, still trying" (attach
            // still running past the bounded wait, or a re-register already in
            // flight) rather than "this editor cannot attach".
            pendingOut: AtomicBoolean? = null,
        ): Boolean {
            var captured: Triple<CrdtReplicaManager, String, Document>? = null
            var captureMiss: String? = null
            val captureOnEdt = {
                val disposed = project.isDisposed
                val manager = if (disposed) null else managerForFile(project, filePath)
                val file = if (disposed) null else LocalFileSystem.getInstance().findFileByPath(filePath)
                val document = file?.let { FileDocumentManager.getInstance().getDocument(it) }
                captureMiss = replicaRecoveryCaptureMissReasonUtil(
                    projectDisposed = disposed,
                    managerPresent = manager != null,
                    filePresent = file != null,
                    documentPresent = document != null,
                )
                if (manager != null && file != null && document != null) {
                    captured = Triple(manager, file.path, document)
                }
            }

            try {
                if (javax.swing.SwingUtilities.isEventDispatchThread()) {
                    captureOnEdt()
                } else {
                    ApplicationManager.getApplication().invokeAndWait { captureOnEdt() }
                }
            } catch (e: Exception) {
                instances[project]?.log?.warn(
                    "[crdt-replica] recovery re-register could not capture editor state for $filePath reason=$reason",
                    e,
                )
                return false
            }

            val (manager, resolvedFilePath, document) = captured ?: run {
                // #jbrejectlog: with no manager there is no manager log, so this
                // miss used to return a bare rejected receipt and log nothing.
                com.intellij.openapi.diagnostic.Logger.getInstance(CrdtReplicaManager::class.java).warn(
                    "[crdt-replica] recovery re-register rejected for $filePath reason=$reason " +
                        "cause=${captureMiss ?: "capture_not_run"} receipt=not_attached",
                )
                return false
            }
            val fileName = File(resolvedFilePath).name
            if (!manager.beginProjectionRecoveryReregister(resolvedFilePath)) {
                manager.log.info(
                    "[crdt-replica] coalesced projection-recovery re-register for $fileName reason=$reason receipt=deferred",
                )
                pendingOut?.set(true)
                return false
            }
            manager.log.info(
                "[crdt-replica] forcing projection-recovery re-register for $fileName reason=$reason receipt=awaiting_attach",
            )
            val attached = manager.ensureOpenDocumentReplica(
                resolvedFilePath,
                document,
                await = true,
                forceRefresh = true,
                requireFreshRegistration = true,
                onAwaitTimeout = { pendingOut?.set(true) },
            )
            if (!attached) {
                // The controller owns a bounded retry loop. A failed attempt must
                // release the editor-side cooldown so the next controller attempt
                // can perform work instead of receiving a coalesced false negative.
                manager.projectionRecoveryReregisterStartedAtMs.remove(resolvedFilePath)
                manager.log.warn(
                    "[crdt-replica] projection-recovery re-register failed for $fileName reason=$reason receipt=not_attached",
                )
            }
            return attached
        }

        private fun runOnEdtNonBlocking(block: () -> Unit) {
            if (javax.swing.SwingUtilities.isEventDispatchThread()) {
                block()
            } else {
                ApplicationManager.getApplication().invokeLater(block)
            }
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

        /**
         * Project-aware attach used by operator actions after a native-generation
         * handoff. Reload temporarily removes every manager; once the handoff is
         * complete it is safe to recreate the current project's manager instead
         * of misreporting that the open editor has no owning controller.
         */
        fun ensureReplicaForOpenDocument(
            project: Project,
            filePath: String,
            document: Document,
            editorText: String? = null,
            await: Boolean = false,
            forceRefresh: Boolean = false,
        ): Boolean {
            if (project.isDisposed) return false
            val manager = managerForFilePath(filePath)
                ?: getInstance(project).takeIf { it.ownsFilePath(filePath) }
                ?: return false
            return manager.ensureOpenDocumentReplica(filePath, document, editorText, await, forceRefresh)
        }

        fun publishClosingDocumentCut(filePath: String, document: Document): Boolean {
            val manager = managerForFilePath(filePath) ?: return false
            return manager.publishClosingDocumentCut(filePath, document)
        }

        fun isApplyingRemote(filePath: String): Boolean =
            instances.values.any { it.applyingRemote.contains(filePath) }

        private fun isReloadingFileContent(filePath: String): Boolean {
            val nowMs = System.currentTimeMillis()
            var reloading = false
            for (instance in instances.values) {
                val startedAtMs = instance.fileContentReloadingPaths[filePath] ?: continue
                if (fileContentReloadInProgressUtil(startedAtMs, nowMs, FILE_CONTENT_RELOAD_BOUND_MS)) {
                    reloading = true
                } else {
                    // A reload that never posted its completion callback. Drop the
                    // marker so operator classification recovers, and say so once.
                    instance.fileContentReloadingPaths.remove(filePath)
                    instance.log.warn(
                        "[crdt-replica] file-content reload marker for $filePath expired after " +
                            "${nowMs - startedAtMs}ms without fileContentReloaded; " +
                            "restoring operator edit classification (#opcapturedormant)",
                    )
                }
            }
            return reloading
        }

        /**
         * #ensurereregister: true when some open project already holds a CRDT
         * replica (forwarder) for [filePath].
         *
         * This guard prevents needless endpoint replacement while a live
         * consumer is already projecting the controller bootstrap. When there
         * is no forwarder, registration reconstructs that downstream consumer
         * without promoting its editor buffer to whole-document authority.
         */
        fun hasOpenDocumentReplica(filePath: String): Boolean =
            instances.values.any { it.forwarders.containsKey(filePath) }

        /**
         * Why the most recent replica attach for [filePath] was refused, or `null`
         * when the last attempt succeeded or none has run (`#replicarefusalreason`).
         *
         * Operator-facing commands gate on `ensureReplicaForOpenDocument` returning
         * false. Reporting only that fact sends the operator to the controller,
         * which is typically healthy — the actual cause lives here.
         */
        fun lastAttachFailureReason(filePath: String): String? =
            instances.values.firstNotNullOfOrNull { it.attachFailureReasons[filePath] }

        /**
         * A one-line operator remedy for [reason], or `null` when there is no
         * specific advice beyond the reason itself.
         *
         * `detached_authority` on a *registration* means the controller saw no
         * live editor PID for the request. Registration is what establishes editor
         * authority, so the controller does not refuse it for lacking authority —
         * it refuses when the request carries no live PID, which is what a plugin
         * generation loaded before that field existed sends. The IDE keeps the
         * generation it started with, so the installed jar can read current on disk
         * while the loaded one is not: only an IDE restart replaces it. This is the
         * same conclusion the controller reaches in its refusal-storm advisory.
         */
        fun attachFailureRemedy(reason: String): String? = when {
            reason.contains("detached_authority") ->
                "Restart the IDE. The controller saw no live editor PID on the registration, which is " +
                    "what a plugin generation loaded before the current install sends. Reloading the " +
                    "native library or recycling the controller cannot replace a loaded plugin generation."
            reason.contains("socket_unavailable") ->
                "No project controller is listening for that document's project root; start or recycle it."
            reason.contains("not_agent_doc_document") ->
                "That file is not an agent-doc session document (no agent_doc_* frontmatter, no <!-- agent: markers)."
            reason.contains("retained-reseed-missing-settled-shadow") ->
                "The restarted editor has no settled ancestor from which it can safely seed the retained " +
                    "controller projection. Copy any unsaved text, close this editor tab, run `agent-doc repair " +
                    "<file>` while it is detached, and reopen it; restarting the editor again cannot create that ancestor."
            reason.contains("ambiguous-retained-projection") ||
                reason.contains("ambiguous-reregister-projection") ->
                "The editor buffer and controller canonical have diverged from their last settled ancestor. " +
                    "Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, " +
                    "and reopen it."
            reason.contains("attach-timeout-pending") ->
                "Registration is still running; wait for the next editor status update before retrying."
            reason.contains("native-ffi-unavailable") ->
                "Inspect About Agent Doc or the IDE log for the executable and library that failed to load. " +
                    "Fix or remove that incompatible install so PATH resolves to the intended agent-doc; the native " +
                    "loader retries automatically. `agent-doc admin reload-lib` cannot reach this editor until FFI loads."
            reason.contains("native-handoff-timeout") ->
                "The native-generation handoff did not finish within the editor action budget. Wait for reload to " +
                    "settle, then run `agent-doc admin reload-lib` once before retrying."
            reason.contains("attach-exception") || reason.contains("attach-worker-failed") ->
                "Inspect the IDE log and the `editor_surface_event` attach-refused entry in `.agent-doc/logs/ops.log`, " +
                    "then repair the named cause before retrying."
            reason.contains("unknown-attach-refusal") || reason.contains("controller-register") ->
                "Run `agent-doc admin inspect <file> --json` and compare editor_replica.live_editors with live_replicas."
            else -> null
        }

        fun isApplyingNonOperatorMutation(filePath: String): Boolean =
            applyingAgentMutations.contains(filePath) ||
                isApplyingRemote(filePath) ||
                isReloadingFileContent(filePath)

        /**
         * The live op-capture epoch generation for [filePath], or `-1` when none.
         *
         * `#opcaptureliveread`: a captured operator burst is replayable only while
         * its path is still at the epoch it was captured in — every remote/agent
         * projection closes the epoch first (`agent_doc_clear_editor_op_epoch`). The
         * op-capture proof receipt states this generation so a burst that was
         * recorded against a retired epoch is readable in `ops.log` rather than
         * inferred from the absence of a refusal.
         */
        fun liveOpCaptureEpochGeneration(filePath: String): Long =
            managerForFilePath(filePath)?.remoteEditorEffectGeneration(filePath) ?: -1L

        fun isOperatorDocumentEvent(filePath: String, event: DocumentEvent): Boolean =
            isOperatorDocumentEventUtil(
                nonOperatorMutation = isApplyingNonOperatorMutation(filePath),
                wholeTextReplaced = event.isWholeTextReplaced,
                documentUnsaved =
                    FileDocumentManager.getInstance().isDocumentUnsaved(event.document),
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
            return nonOperatorMutationEpochs
                .computeIfAbsent(filePath) { AtomicLong(0L) }
                .incrementAndGet()
        }
    }

    /** Last refusal reason per document path; cleared on a successful register. */
    private val attachFailureReasons = java.util.concurrent.ConcurrentHashMap<String, String>()

    private fun ownsFilePath(filePath: String): Boolean {
        val base = project.basePath ?: return false
        return try {
            File(filePath).absoluteFile.toPath().startsWith(File(base).absoluteFile.toPath())
        } catch (_: Exception) {
            false
        }
    }
}

/**
 * Whether [text] is an agent-doc session document.
 *
 * Mirrors the controller's `is_agent_doc_document`: a session document carries
 * `agent_doc_*` frontmatter or an `<!-- agent:` component marker. A README,
 * CONTRIBUTING, plan note, or draft has neither, never attaches an editor
 * authority, and is refused terminally — so registering one is work that cannot
 * succeed. Kept as a plain content test, deliberately identical to the
 * controller's, so the two ends cannot disagree about what a session document is.
 */
internal fun isAgentDocDocumentTextUtil(text: CharSequence): Boolean =
    text.contains("agent_doc_session") ||
        text.contains("agent_doc_format") ||
        text.contains("agent_doc_write") ||
        text.contains("<!-- agent:")

/**
 * Whether adopting a retained controller canonical projection would destroy
 * operator text (`#retainedprojectionclobbersoperatortext`).
 *
 * [publishedShadow] is the last text this plugin PROVED reached the controller's
 * visible projection, held in memory; [bufferText] is what the operator can
 * currently see. The two cases
 * that reach the retained-canonical branch are opposite and must not be
 * conflated:
 *
 *  - a RESTARTED IDE has no shadow (the map does not survive a restart), so its
 *    buffer is a stale reconstruction and adopting canonical is the recovery;
 *  - a LIVE IDE whose buffer has moved past its shadow is holding text the
 *    controller has never seen — because registration was being refused — and
 *    adopting canonical over it is data loss.
 *
 * Unknown buffer text is not divergence: without both facts this returns false
 * and the historical fail-closed adoption stands.
 */
internal enum class RetainedRegistrationProjectionAction {
    ApplyCanonical,
    PublishOperatorBuffer,
    HoldOperatorBuffer,
    DeferCanonicalProjection,
    /**
     * `#editorauth2`: three generations differ, but reconciling them (base =
     * settled shadow, yours = buffer, agent = canonical) is clean. Registration
     * adopts canonical into the replica, publishes the merged text as a local
     * delta, and projects it into the buffer. Neither side's text is lost, and the
     * ambiguity hold gets an exit that does not overwrite the operator.
     */
    MergeForward,
}

/**
 * Name the causal proof missing from a retained-registration hold.
 *
 * A full editor restart intentionally loses the JVM-local settled shadow. The
 * fresh controller reseed path must not dereference that absent shadow while
 * formatting diagnostics (which previously threw after registration and left
 * a transient member behind), and it deserves a different remedy from an
 * ordinary three-generation conflict.
 */
internal fun retainedRegistrationHoldReasonUtil(
    retainedReplicaReseedPending: Boolean,
    publishedShadow: String?,
): String =
    if (retainedReplicaReseedPending && publishedShadow == null) {
        "retained-reseed-missing-settled-shadow"
    } else {
        "ambiguous-retained-projection"
    }

internal fun retainedRegistrationProjectionActionForAttachUtil(
    deferCanonicalProjectionForPendingLocal: Boolean,
    canonicalProjectionRetained: Boolean,
    retainedReplicaReseedPending: Boolean = false,
    canonicalCoversRetainedFrontier: Boolean? = null,
    publishedShadow: String?,
    bufferText: String?,
    canonicalText: String?,
    canonicalContainsOperatorEdits: Boolean? = null,
    cleanMergeAvailable: Boolean? = null,
): RetainedRegistrationProjectionAction =
    if (deferCanonicalProjectionForPendingLocal) {
        RetainedRegistrationProjectionAction.DeferCanonicalProjection
    } else if (retainedReplicaReseedPending) {
        // A fresh controller's empty hub is a synchronization placeholder, not
        // a document projection. A retained editor may seed it only from a live
        // buffer with an independently settled controller-accepted ancestor.
        if (publishedShadow != null && bufferText != null) {
            RetainedRegistrationProjectionAction.PublishOperatorBuffer
        } else {
            RetainedRegistrationProjectionAction.HoldOperatorBuffer
        }
    } else if (canonicalProjectionRetained) {
        retainedRegistrationProjectionActionUtil(
            canonicalCoversRetainedFrontier = canonicalCoversRetainedFrontier,
            publishedShadow = publishedShadow,
            bufferText = bufferText,
            canonicalText = canonicalText,
            canonicalContainsOperatorEdits = canonicalContainsOperatorEdits,
            cleanMergeAvailable = cleanMergeAvailable,
        )
    } else if (
        publishedShadow != null &&
        bufferText != null &&
        bufferText != publishedShadow &&
        canonicalText != bufferText
    ) {
        // `#reloadclobbersoperatortext`: an ordinary native-generation re-register
        // (a `make install` library reload) is not a retained projection, but the
        // live buffer can still hold text typed while the replica was detached.
        // tasks/api.md, 2026-09-28 23:08:16: the operator typed and pressed Run
        // Agent Doc during the reload window; re-register then projected the
        // controller's pre-edit canonical over that buffer and the text was gone.
        // A buffer past its settled shadow gets the same causal decision as the
        // retained path: publish it when canonical is that shadow, adopt canonical
        // only when it provably contains the edits, otherwise hold.
        retainedRegistrationProjectionActionUtil(
            publishedShadow = publishedShadow,
            bufferText = bufferText,
            canonicalText = canonicalText,
            canonicalContainsOperatorEdits = canonicalContainsOperatorEdits,
            cleanMergeAvailable = cleanMergeAvailable,
        )
    } else {
        RetainedRegistrationProjectionAction.ApplyCanonical
    }

/**
 * Decide registration direction from causal facts, not wall-clock arrival.
 *
 * A live buffer can be published only when canonical is exactly the in-memory
 * shadow it was derived from. If all three generations differ, registration
 * must hold instead of selecting one by destructive whole-document projection.
 */
internal fun retainedRegistrationProjectionActionUtil(
    canonicalCoversRetainedFrontier: Boolean? = null,
    publishedShadow: String?,
    bufferText: String?,
    canonicalText: String?,
    canonicalContainsOperatorEdits: Boolean? = null,
    cleanMergeAvailable: Boolean? = null,
): RetainedRegistrationProjectionAction =
    when {
        // Convergence is safe and must win before the three-generation test;
        // this is also the terminal edge for a prior ambiguity hold.
        bufferText != null && canonicalText == bufferText ->
            RetainedRegistrationProjectionAction.ApplyCanonical
        publishedShadow == null || bufferText == null ->
            RetainedRegistrationProjectionAction.ApplyCanonical
        // The buffer has not advanced past the last controller-accepted
        // projection, and canonical is causally at or beyond the cached native
        // CRDT frontier. A forced full bootstrap is therefore a safe recovery,
        // even when its text differs from that settled projection.
        bufferText == publishedShadow && canonicalCoversRetainedFrontier == true ->
            RetainedRegistrationProjectionAction.ApplyCanonical
        canonicalText == publishedShadow -> RetainedRegistrationProjectionAction.PublishOperatorBuffer
        // `#ambiguousholdforever`: the proven rebase. Every operator change from
        // the shadow is already present in canonical (the controller ingested
        // those deltas before the endpoint dropped), so adopting canonical keeps
        // all operator text. Without this edge the hold had no exit: the cut
        // never changes, and registration was refused every second for hours.
        canonicalContainsOperatorEdits == true -> RetainedRegistrationProjectionAction.ApplyCanonical
        // `#editorauth2`: publish the editor state and merge forward instead of
        // holding forever, when the three-way reconcile has no conflicts.
        cleanMergeAvailable == true -> RetainedRegistrationProjectionAction.MergeForward
        else -> RetainedRegistrationProjectionAction.HoldOperatorBuffer
    }

private val BINARY_OWNED_BOUNDARY_LINE =
    Regex("""(?m)^[ \t]*<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->[ \t]*(?:\r?\n|$)""")
private val BINARY_OWNED_HEAD_SUFFIX = Regex("""(?m)^(#{1,6} .*?) \(HEAD\)[ \t]*$""")

/**
 * `#ambiguousholdforever2`: [text] with the markers agent-doc alone writes removed: boundary
 * marker lines and the transient ` (HEAD)` heading suffix. Their placement differs between a
 * controller disk projection and canonical without any operator having typed, so they are never
 * operator edits for a containment proof.
 */
internal fun withoutBinaryOwnedMarkersUtil(text: String): String =
    text.replace(BINARY_OWNED_BOUNDARY_LINE, "").replace(BINARY_OWNED_HEAD_SUFFIX, "$1")

/** One splice turning [before] into [after]: the common code-point prefix and suffix stay. */
internal fun singleSpliceBatchUtil(before: String, after: String): PreparedLocalEditorBatch {
    val old = before.codePoints().toArray()
    val new = after.codePoints().toArray()
    var prefix = 0
    while (prefix < old.size && prefix < new.size && old[prefix] == new[prefix]) prefix++
    var suffix = 0
    while (
        suffix < old.size - prefix &&
        suffix < new.size - prefix &&
        old[old.size - 1 - suffix] == new[new.size - 1 - suffix]
    ) {
        suffix++
    }
    val insert = String(new, prefix, new.size - prefix - suffix)
    val edits =
        if (old.size - prefix - suffix == 0 && insert.isEmpty()) {
            emptyList()
        } else {
            listOf(PreparedLocalEditorEdit(prefix, old.size - prefix - suffix, insert))
        }
    return PreparedLocalEditorBatch(edits = edits, resultingText = after)
}

internal fun retainedCanonicalWouldClobberOperatorTextUtil(
    publishedShadow: String?,
    bufferText: String?,
): Boolean = publishedShadow != null && bufferText != null && publishedShadow != bufferText

internal fun retainedProjectionHoldAllowsRefreshUtil(
    holdActive: Boolean,
    registrationAttemptDue: Boolean,
): Boolean = !holdActive || registrationAttemptDue

internal fun shouldApplyRemoteCrdtUpdateUtil(update: ReplicaRemoteUpdate, clientId: Long): Boolean =
    update.origin != clientId

/**
 * Upper bound on how long a `beforeFileContentReload` marker may stand.
 *
 * A file-content reload is a synchronous EDT operation; it completes in
 * milliseconds. Anything older than this never posted `fileContentReloaded`.
 */
internal const val FILE_CONTENT_RELOAD_BOUND_MS = 5_000L

/**
 * True while a file-content reload started at [startedAtMs] is still plausibly
 * in flight (`#opcapturedormant`).
 *
 * A clock that moved backwards must not extend the window indefinitely either, so
 * a negative elapsed time expires the marker.
 */
internal fun fileContentReloadInProgressUtil(
    startedAtMs: Long,
    nowMs: Long,
    boundMs: Long,
): Boolean {
    val elapsedMs = nowMs - startedAtMs
    return elapsedMs in 0..boundMs
}

internal fun isOperatorDocumentEventUtil(
    nonOperatorMutation: Boolean,
    wholeTextReplaced: Boolean,
    documentUnsaved: Boolean,
): Boolean = !nonOperatorMutation && !wholeTextReplaced && documentUnsaved

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

/**
 * A REPLACE delivery installs [canonicalText] wholesale and re-bootstraps the
 * local replica, so any operator text the canonical lacks is destroyed rather
 * than merged. The incremental editing shadow cannot detect this: an operator
 * keystroke is a local CRDT op, so shadow, buffer and local replica all agree
 * on text the controller may have quarantined. Only the settled shadow (the
 * last projection the controller acknowledged) separates operator text the
 * controller has seen from text it never accepted.
 *
 * True when the live buffer moved past its settled shadow and the canonical is
 * not that buffer. Without a settled shadow (a restarted IDE) this is false and
 * the historical replace semantics stand.
 */
internal fun replaceDeliveryWouldClobberUnsettledOperatorTextUtil(
    settledShadow: String?,
    bufferText: String?,
    canonicalText: String,
): Boolean =
    settledShadow != null &&
        bufferText != null &&
        bufferText != settledShadow &&
        bufferText != canonicalText

internal fun remoteCrdtReplaceStillCurrentUtil(
    expectedText: String,
    currentText: String,
    replicaText: String?,
): Boolean =
    currentText == expectedText && replicaText == expectedText
