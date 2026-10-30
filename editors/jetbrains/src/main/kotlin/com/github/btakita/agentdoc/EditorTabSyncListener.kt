package com.github.btakita.agentdoc

import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.FileEditorManagerEvent
import com.intellij.openapi.fileEditor.FileEditorManagerListener
import com.intellij.openapi.fileEditor.ex.FileEditorManagerEx
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.IdeFocusManager
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

internal data class FocusProjectionEffectRequest(
    val generation: Long,
    val projectRoot: String,
    val filePath: String,
    val surfaceJson: String,
    val surfaceId: String = "project",
    val remoteControllerSelectionFocusEvidence: Boolean = false,
)

internal data class FocusProjectionEffectResponse(
    val request: FocusProjectionEffectRequest,
    val transport: CpEditorRouteResult,
)

/**
 * Whether stopping a local editor-surface adapter also withdraws its retained controller Source.
 *
 * A project close is an authority transition: the editor surface is gone and the controller must
 * forget it. A dynamic plugin replacement is only an adapter-generation transition: the project
 * and its visible layout survive, so the old controller fact remains authoritative until the
 * replacement generation publishes its first observation.
 */
internal enum class EditorSurfaceShutdownPolicy(
    val retireControllerProjection: Boolean,
) {
    RetireControllerProjection(true),
    PreserveForDynamicReplacement(false),
}

/**
 * Reports what this editor looks like; the tmux consequence is derived elsewhere (`#jbsurfaceswap`
 * / `#jbpluginlazilyeffects`).
 *
 * Every tab selection publishes selected-document intent. A later editor-layout event projects that
 * intent with the current visible markdown set and column layout, then hands the complete
 * observation directly to the already-running Project Controller. Socket transport adds an
 * ordered reload generation/cursor without entering the reloadable native library. The
 * controller's process-scoped graph folds it against locally observed tmux state, derives
 * focus-vs-sync, and runs the tmux consequence as an `Effect`.
 *
 * The plugin therefore holds no plan or previous-signature field: an observation identical to the
 * last one is idle and costs nothing, so repeat events need no dedup here. A failed controller
 * handoff retains the observation and retries it with a capped backoff; otherwise a one-time socket
 * refusal would strand auto-sync until an unrelated editor event. What remains is event-storm
 * handling that is genuinely the editor's: a generation guard so a burst reports only its final
 * state. Capture is projected by the next EDT event; socket delivery remains off the EDT, so neither
 * side waits on the other.
 *
 * Focus has a separate micro-coalesced state lane because a visible editor surface may span
 * documents owned by different project controllers. The focused file resolves its own controller
 * root and publishes one generation-fenced, focus-only surface Source; that controller's Lazily
 * graph derives pane selection while the spanning surface remains the authority for layout.
 *
 * Registered from [PluginLifecycleListener] via [install] so it survives hot-reload.
 */
class EditorTabSyncListener : FileEditorManagerListener {
    private val latestSurfaceObservation = AtomicReference<PendingSurfaceObservation?>(null)

    /**
     * One editor surface has one active controller root. When a cross-root split changes which
     * root spans the whole surface, the old controller projection must be retired immediately;
     * otherwise both controllers keep reconciling the same tmux window from incompatible retained
     * layouts.
     */
    private val surfaceRoots = SurfaceRootOwnership()
    private val surfaceEndpoints = SurfaceEndpointOwnership()

    /**
     * Debounce generation. Per-instance, so one project's tab churn cannot supersede another
     * project's pending observation.
     */
    private val generation = AtomicLong(0)

    /**
     * Advances only when this listener's controller projection is retired as a whole (an
     * authenticated frontend surface took authority). Unlike [generation], a newer editor
     * observation never advances it: superseding an in-flight delivery is not a reason to retire
     * the endpoint that delivery published (`#tmuxautosyncretired`).
     */
    private val projectionAuthorityEpoch = AtomicLong(0)
    private val focusProjectionGeneration = AtomicLong(0)
    private val selectionFocusProbeGeneration = AtomicLong(0)
    private val surfaceDeliveryRetryAttempt = AtomicLong(0)
    private val activeFocusProjection = AtomicReference<ScheduledFuture<*>?>(null)
    private val lifecycleLock = Any()

    @Volatile
    private var closed = false

    /**
     * True between [retireControllerProjection] (an authenticated frontend took authority) and
     * [resumeControllerProjection] (the backend fallback owns the selection edge again). Guarded by
     * [lifecycleLock]. A retired-endpoint receipt re-registers only while this is false: the model's
     * `live` (`formal/tla/PassiveTmuxSyncRetireNet.tla`), so a frame this listener no longer owns stays
     * fenced (`#surfaceretireformal`).
     */
    private var projectionRetired = false

    /**
     * Whether this listener still owns the frames it publishes, so a typed retired-endpoint receipt
     * is a muted LIVE endpoint that must re-register rather than a correct fence
     * ([CpRouteClient.SurfaceEndpointReregistration]).
     */
    internal fun projectionLive(): Boolean =
        synchronized(lifecycleLock) {
            !closed && !projectionRetired
        }

    private val surfaceDeliveryExecutor = GenerationWork.newSingleThreadScheduledExecutor("agent-doc-editor-surface-delivery")
    private val focusProjectionExecutor = GenerationWork.newSingleThreadScheduledExecutor("agent-doc-editor-focus-projection")

    internal enum class FocusProjectionReceiptDecision {
        Applied,
        RepairLayout,
        Retained,
        Superseded,
        TransportUnavailable,
    }

    internal enum class FocusProjectionPublicationGate(val logName: String) {
        Ready("ready"),
        Superseded("superseded"),
        Inactive("inactive"),
        Disposed("disposed"),
        Closed("closed"),
    }

    companion object {
        private val LOG = Logger.getInstance(EditorTabSyncListener::class.java)
        private val GSON = com.google.gson.Gson()
        private const val FOCUS_COALESCE_MS = 12L
        private val FOCUS_LAYOUT_REPAIR_REASONS =
            setOf(
                "actor_pane_not_visible",
                "actor_not_focusable",
                "missing_actor_record",
                "actor_pane_not_alive",
                "registry_pane_not_alive",
                "live_owner_pane_not_alive",
            )

        private val instances = ConcurrentHashMap<Project, EditorTabSyncListener>()

        fun install(project: Project): EditorTabSyncListener =
            instances.compute(project) { _, current ->
                if (current == null || current.closed) EditorTabSyncListener() else current
            }!!

        /** The installed, open listener for [project], if any. */
        internal fun forProject(project: Project): EditorTabSyncListener? =
            instances[project]?.takeUnless { it.closed }

        /** Release local surface graphs and apply the lifecycle-owned controller retirement policy. */
        internal fun disposeProject(
            project: Project,
            policy: EditorSurfaceShutdownPolicy = EditorSurfaceShutdownPolicy.RetireControllerProjection,
        ) {
            instances.remove(project)?.shutdown(policy)
        }

        internal fun shouldPublishFocusProjection(
            requestedGeneration: Long,
            currentGeneration: Long,
            projectWindowActive: Boolean,
            remoteControllerSelectionFocusEvidence: Boolean = false,
        ): Boolean =
            focusProjectionPublicationGate(
                requestedGeneration = requestedGeneration,
                currentGeneration = currentGeneration,
                projectWindowActive = projectWindowActive,
                remoteControllerSelectionFocusEvidence = remoteControllerSelectionFocusEvidence,
                listenerClosed = false,
                projectDisposed = false,
            ) == FocusProjectionPublicationGate.Ready

        internal fun focusProjectionPublicationGate(
            requestedGeneration: Long,
            currentGeneration: Long,
            projectWindowActive: Boolean,
            remoteControllerSelectionFocusEvidence: Boolean,
            listenerClosed: Boolean,
            projectDisposed: Boolean,
        ): FocusProjectionPublicationGate =
            when {
                listenerClosed -> FocusProjectionPublicationGate.Closed
                projectDisposed -> FocusProjectionPublicationGate.Disposed
                requestedGeneration != currentGeneration -> FocusProjectionPublicationGate.Superseded
                !projectWindowActive && !remoteControllerSelectionFocusEvidence ->
                    FocusProjectionPublicationGate.Inactive
                else -> FocusProjectionPublicationGate.Ready
            }

        internal fun shouldClaimSettledSelectionFocus(
            selectionPath: String,
            activeWindowPath: String?,
            requestedGeneration: Long,
            currentGeneration: Long,
            projectWindowActive: Boolean,
        ): Boolean =
            requestedGeneration == currentGeneration &&
                projectWindowActive &&
                selectionPath == activeWindowPath

        internal fun decideFocusProjectionReceipt(
            response: FocusProjectionEffectResponse,
            currentGeneration: Long,
            projectWindowActive: Boolean,
        ): FocusProjectionReceiptDecision {
            if (
                !shouldPublishFocusProjection(
                    requestedGeneration = response.request.generation,
                    currentGeneration = currentGeneration,
                    projectWindowActive = projectWindowActive,
                    remoteControllerSelectionFocusEvidence =
                        response.request.remoteControllerSelectionFocusEvidence,
                )
            ) {
                return FocusProjectionReceiptDecision.Superseded
            }
            if (response.transport.exitCode != 0) {
                return FocusProjectionReceiptDecision.TransportUnavailable
            }
            if (focusProjectionApplied(response.transport.output)) {
                return FocusProjectionReceiptDecision.Applied
            }
            if (focusProjectionRequiresLayoutRepair(response.transport.output)) {
                return FocusProjectionReceiptDecision.RepairLayout
            }
            return FocusProjectionReceiptDecision.Retained
        }

    /**
     * Exact effect receipt for the retained focus-only surface. Admission is not success: install
     * the reverse-focus handoff lease only after the controller proves `select-pane`.
     */
        internal fun focusProjectionApplied(receiptJson: String): Boolean {
            return try {
                val receipt = JsonParser.parseString(receiptJson).asJsonObject
            val outcome =
                receipt.get("outcome")
                    ?.takeUnless { it.isJsonNull }
                    ?.asString
                    ?.let(JsonParser::parseString)
                    ?.asJsonObject
                    ?: return false
            val data =
                outcome.get("data")
                    ?.takeIf { it.isJsonObject }
                    ?.asJsonObject
                    ?: outcome
            data.get("focused")?.asBoolean == true
        } catch (_: Exception) {
                false
            }
        }

        /**
         * A focus-only projection cannot move or create panes, but its exact receipt can prove
         * that the selected document has no usable visible owner. Re-publish the complete editor
         * surface in that case so the layout graph, which owns structural reconciliation, can
         * restore a reaped/dead owner or move a stashed owner back without letting this narrow
         * projection grow the layout itself.
         */
        internal fun focusProjectionRequiresLayoutRepair(receiptJson: String): Boolean {
            return try {
                val receipt = JsonParser.parseString(receiptJson).asJsonObject
                val outcome =
                    receipt.get("outcome")
                        ?.takeUnless { it.isJsonNull }
                        ?.asString
                        ?.let(JsonParser::parseString)
                        ?.asJsonObject
                        ?: return false
                val data =
                    outcome.get("data")
                        ?.takeIf { it.isJsonObject }
                        ?.asJsonObject
                        ?: outcome
                val reason = data.get("reason")?.asString
                data.get("focused")?.asBoolean == false &&
                    reason in FOCUS_LAYOUT_REPAIR_REASONS
            } catch (_: Exception) {
                false
            }
        }
    }

internal enum class ObservationAuthority {
Layout,
DocumentSelection,
EditorFocus,
FileOpened,
IdeActivation,
}

internal enum class SelectionFocusAuthority {
ActiveEditorSplit,
BackgroundOrUnknownSplit;

companion object {
        /**
         * GH #88 / #102: a JetBrains Remote Dev backend can enumerate editor windows while still
         * exposing no `currentWindow`, because the editor UI lives in the thin client. With no
         * active window path to match, a selection event is the only focus signal there is, so it
         * owns focus; gating this on the window count drops every selection once enumeration works.
         */
        fun decide(
            selectionPath: String,
            activeWindowPath: String?,
            previousSelectionPath: String? = null,
        ): SelectionFocusAuthority =
            if (
                activeWindowPath == null ||
                activeWindowPath == selectionPath ||
                    (previousSelectionPath != null && activeWindowPath == previousSelectionPath)
) {
    ActiveEditorSplit
} else {
    BackgroundOrUnknownSplit
}
}
}

    internal object SurfaceObservationOrdering {
        private fun ownsFocus(authority: ObservationAuthority?): Boolean =
            authority == ObservationAuthority.DocumentSelection ||
                authority == ObservationAuthority.EditorFocus

        fun shouldReplace(
            currentAuthority: ObservationAuthority?,
            incomingAuthority: ObservationAuthority,
        ): Boolean {
            if (ownsFocus(currentAuthority) && !ownsFocus(incomingAuthority)) {
                return false
            }
            if (
                currentAuthority == ObservationAuthority.DocumentSelection &&
                incomingAuthority == ObservationAuthority.EditorFocus
            ) {
                return false
            }
            return true
        }
    }

    /**
     * The pending slot protects editor-event ordering only until the EDT has captured a
     * self-consistent surface. Controller delivery can take seconds while it proves pane
     * ownership and realizes a layout, so the slot must not retain an already-captured selection
     * while that I/O runs. Release before entering the delivery executor; restore only when the
     * same generation failed and no newer observation has occupied the slot.
     */
    internal object ObservationDeliveryOwnership {
        fun <T : Any> releaseForDelivery(
            slot: AtomicReference<T?>,
            captured: T,
        ): Boolean = slot.compareAndSet(captured, null)

        fun <T : Any> retainAfterFailure(
            slot: AtomicReference<T?>,
            captured: T,
        ): Boolean = slot.compareAndSet(null, captured)
    }

    /**
     * What a controller-accepted delivery does with the endpoint it just published, once it
     * returns from the controller call (`#tmuxautosyncretired`).
     *
     * `editor_surface_forget` of an exact `(client, surface)` endpoint is terminal for the
     * plugin's process-lifetime connection generation: the controller rejects every later
     * observation of that endpoint (`rejected=retired_endpoint`) until a new plugin generation or
     * a fresh controller. So the delivery may retire its endpoint only when this listener's
     * whole projection was retired while it was in flight (an authority handoff). A newer
     * observation from the same editor merely superseded it; that observation publishes the same
     * endpoint at a higher sequence, so retiring here muted every later tab switch (the GH #260
     * regression).
     */
    internal object InFlightSurfaceDelivery {
        enum class Decision {
            RecordEndpoint,
            RetireEndpoint,
        }

        /** Deliberately blind to the observation generation: a newer observation is not a retirement. */
        fun decide(
            requestedAuthorityEpoch: Long,
            currentAuthorityEpoch: Long,
        ): Decision =
            if (requestedAuthorityEpoch != currentAuthorityEpoch) {
                Decision.RetireEndpoint
            } else {
                Decision.RecordEndpoint
            }
    }

    internal class SurfaceRootOwnership {
        private val activeRoot = AtomicReference<String?>(null)
        private val observedRoots: MutableSet<String> = ConcurrentHashMap.newKeySet()

        fun recordAttempt(root: String) {
            observedRoots.add(root)
        }

        fun recordAttempts(roots: Collection<String>) {
            observedRoots.addAll(roots)
        }

        fun rootsToRetireBeforePublishing(root: String): List<String> =
            observedRoots.filter { it != root }.sorted()

        fun markPublished(root: String): List<String> {
            observedRoots.add(root)
            activeRoot.set(root)
            return observedRoots.filter { it != root }.sorted()
        }

        fun markForgotten(root: String): Boolean =
            activeRoot.get() != root && observedRoots.remove(root)

        /** The root this listener's surface projection was last published to. */
        fun active(): String? = activeRoot.get()

        fun drain(): List<String> {
            activeRoot.set(null)
            val roots = observedRoots.toList().sorted()
            observedRoots.clear()
            return roots
        }
    }

    internal data class SurfaceEndpoint(val projectRoot: String, val surfaceId: String)

    /** Retained per-frame endpoints, used to retire a closed DockWindow without its siblings. */
    internal class SurfaceEndpointOwnership {
        private val observed: MutableSet<SurfaceEndpoint> = ConcurrentHashMap.newKeySet()

        fun record(endpoint: SurfaceEndpoint) {
            observed.add(endpoint)
        }

        fun missing(liveSurfaceIds: Set<String>): List<SurfaceEndpoint> =
            observed
                .filter { it.surfaceId !in liveSurfaceIds }
                .sortedWith(compareBy(SurfaceEndpoint::projectRoot, SurfaceEndpoint::surfaceId))

        fun markPublished(endpoint: SurfaceEndpoint, liveSurfaceIds: Set<String>?): List<SurfaceEndpoint> {
            record(endpoint)
            // The observation itself is positive liveness evidence. Never let a lagging IDE window
            // enumeration retire the endpoint that just published this exact observation.
            return liveSurfaceIds?.let { missing(it + endpoint.surfaceId) }.orEmpty()
        }

        fun markForgotten(endpoint: SurfaceEndpoint): Boolean = observed.remove(endpoint)

        fun drain(): List<SurfaceEndpoint> {
            val endpoints = observed
                .toList()
                .sortedWith(compareBy(SurfaceEndpoint::projectRoot, SurfaceEndpoint::surfaceId))
            observed.clear()
            return endpoints
        }
    }

    /**
     * IDEA may emit selection, structural-layout, and file-open edges before `selectedFiles`
     * and every restored split window agree. Re-read the editor projection on a bounded
     * number of later EDT turns. This is event settling, not a timer/retry ladder: every
     * pass yields the EDT, the generation guard cancels stale work, and only exhausted
     * document selection applies an authoritative old-to-new edge.
     */
internal object SelectionProjectionSettling {
    const val MAX_REPROJECTION_PASSES = 3

    data class SettledProjection(
        val visibleMdFiles: List<String>,
        val editorLayout: EditorLayout?,
    )

    fun shouldReproject(
        authority: ObservationAuthority,
        remainingPasses: Int,
    ): Boolean =
        when (authority) {
                ObservationAuthority.Layout,
                ObservationAuthority.DocumentSelection,
                ObservationAuthority.EditorFocus,
                ObservationAuthority.FileOpened,
                ObservationAuthority.IdeActivation -> remainingPasses > 0
            }

    fun reconcileEventEdge(
        preferredFile: String?,
        previousFile: String?,
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout?,
    ): SettledProjection {
            if (
                preferredFile.isNullOrBlank() ||
                    previousFile.isNullOrBlank()
            ) {
                return SettledProjection(visibleMdFiles, editorLayout)
            }

            fun replacePreviousIfStale(files: List<String>): List<String> =
                if (preferredFile in files || previousFile !in files) {
                    files
                } else {
                    files
                        .map { file -> if (file == previousFile) preferredFile else file }
                        .distinct()
                }

            return SettledProjection(
                visibleMdFiles = replacePreviousIfStale(visibleMdFiles),
                editorLayout =
                    editorLayout?.copy(
                        columns =
                            editorLayout.columns.map { column ->
                                column.copy(files = replacePreviousIfStale(column.files))
                            },
                    ),
            )
    }
}

    internal object SurfaceDeliveryRetry {
        private const val BASE_DELAY_MS = 100L
        private const val MAX_DELAY_MS = 2_000L

        fun delayMs(attempt: Long): Long {
            val exponent = (attempt.coerceAtLeast(1L) - 1L).coerceAtMost(5L).toInt()
            return (BASE_DELAY_MS shl exponent).coerceAtMost(MAX_DELAY_MS)
        }
    }

    /** One column of the reported split layout. Wire shape of Rust `SurfaceColumn`. */
    internal data class SurfaceColumnPayload(val files: List<String>)

    /**
     * What the editor looks like right now. Wire shape of Rust `EditorSurface`.
     *
     * Every field is something the editor saw. Notably absent is any notion of whether tmux agrees:
     * that is derived by comparing this observation against the controller's own, so the plugin
     * never reports a fact it would have to ask the controller for.
     */
    internal data class EditorSurfacePayload(
        val focused: String,
        val visible: List<String>,
        val open: List<String>,
        val columns: List<SurfaceColumnPayload>,
        @SerializedName("force_reconcile") val forceReconcile: Boolean,
        @SerializedName("focus_only") val focusOnly: Boolean,
        @SerializedName("preserve_focus") val preserveFocus: Boolean,
        /**
         * GH #320: provenance of [columns]' left-to-right order, as observed state. `editor` only
         * for measured split geometry; a Remote Dev backend (no client split structure) and
         * unmeasured local windows publish `unknown`, or `retained` from an anchored fold, so the
         * controller never presents membership order as geometry.
         */
        @SerializedName("column_order") val columnOrder: String = "editor",
    )

private data class PendingSurfaceObservation(
    val project: Project,
    val preferredFile: VirtualFile?,
    val previousFile: VirtualFile? = null,
    val forceReconcile: Boolean,
    val authority: ObservationAuthority,
) {
    val preserveFocus: Boolean
        get() =
            authority == ObservationAuthority.Layout ||
                authority == ObservationAuthority.FileOpened ||
                authority == ObservationAuthority.IdeActivation
}

private data class PendingSurface(
    val projectRoot: String,
    val relativePath: String,
    val surfaceId: String,
    val surfaceJson: String,
    val columnGeometry: SurfaceCommandReference.ColumnGeometry,
    val knownControllerRoots: List<String>,
    val liveSurfaceIds: Set<String>?,
)

/**
 * Immutable editor-only snapshot captured on the EDT. Project-root discovery, filesystem walks,
 * patch-watch registration, JSON construction, and controller delivery all happen later on the
 * serialized background lane.
 *
 * GH #326 phase 2: the capture holds evidence only. What the evidence means (visible documents,
 * readiness, active document, open-tab proximity, stale-selection settling, layout, payload) is the
 * native `surface_projection` derivation's answer on the delivery worker ([SurfaceProjection]).
 */
internal data class CapturedSurface(
    val project: Project,
    val projectBasePath: String?,
    val surfaceId: String,
    val liveSurfaceIds: Set<String>?,
    val evidence: SurfaceProjection.SurfaceProjectionEvidence,
)

    /**
     * GH #326 phase 2: the pre-#326 surface rules, now the frozen compatibility floor that
     * [SurfaceProjection.frozen] composes when no native generation answers `surface_projection`.
     * Never extend them; change the native derivation and its parity fixture instead.
     */
    internal object SurfaceReport {
        enum class ProjectionReadiness {
            Current,
            AwaitingSelectedDocument,
        }

        enum class WindowSelectionReadiness {
            Current,
            AwaitingBackendWindowSelection,
        }

        fun projectionReadiness(
            preferredActiveFile: String?,
            visibleMdFiles: List<String>,
            layoutMdFiles: List<String>? = null,
        ): ProjectionReadiness =
            if (
                !preferredActiveFile.isNullOrBlank() &&
                (
                    preferredActiveFile !in visibleMdFiles ||
                        layoutMdFiles?.let { preferredActiveFile !in it } == true
                )
            ) {
                ProjectionReadiness.AwaitingSelectedDocument
            } else {
                ProjectionReadiness.Current
            }

        /**
         * GH #88: the selected file of each editor split. A JetBrains Remote Dev backend
         * exposes no `EditorWindow` (its editor UI lives in the thin client), so the window
         * list is empty while `FileEditorManager.selectedFiles` still names one file per
         * visible split -- the source the manual Sync Tmux Panes action already used, which
         * is why only the manual action moved panes there.
         */
        fun splitSelections(
            windowSelections: List<String?>,
            managerSelectedFiles: List<String>,
        ): List<String?> = windowSelections.ifEmpty { managerSelectedFiles }

        /**
         * GH #326: [projectionReadiness]'s layout half, applied where the local layout now resolves
         * (the delivery worker). The layout survives only when it holds the preferred document.
         */
        fun layoutForPreferredDocument(
            preferredActiveFile: String?,
            editorLayout: EditorLayout?,
        ): EditorLayout? =
            editorLayout?.takeIf {
                projectionReadiness(
                    preferredActiveFile = preferredActiveFile,
                    visibleMdFiles = it.columns.flatMap(LayoutColumn::files),
                ) == ProjectionReadiness.Current
            }

        fun restoredEditorWindowsReady(selectedWindowFiles: List<String?>): Boolean =
            selectedWindowFiles.isNotEmpty() && selectedWindowFiles.all { it != null }

        /**
         * Remote Dev client state owns the visible layout even when the backend happens to expose
         * an incomplete local [com.intellij.openapi.fileEditor.impl.EditorWindow]. The backend gate
         * therefore applies only after the remote-client authority decision (GH #177).
         */
        fun windowSelectionReadiness(
            selectedWindowFiles: List<String?>,
            useRemoteClientLayout: Boolean,
        ): WindowSelectionReadiness =
            if (useRemoteClientLayout || restoredEditorWindowsReady(selectedWindowFiles)) {
                WindowSelectionReadiness.Current
            } else {
                WindowSelectionReadiness.AwaitingBackendWindowSelection
            }

        /**
         * `#stickymdpane`: `stickyWindowFallbacks[i]` is the last document
         * window `i` showed, used when that window's current tab is a source
         * file. Dropping such a window collapsed the mirrored tmux layout to a
         * single pane the moment the operator opened source next to their
         * session document. A window that never held a document contributes
         * nothing, so closing a split still collapses the layout.
         */
        fun visibleMarkdownFilesFromRestoredWindows(
            selectedWindowFiles: List<String?>,
            stickyWindowFallbacks: List<String?> = emptyList(),
            sessionDocumentPaths: Set<String>? = null,
        ): List<String> =
            selectedWindowFiles
                .mapIndexed { index, selected ->
                    selected?.takeIf { path ->
                        sessionDocumentPaths?.contains(path) ?: path.endsWith(".md")
                    }
                        ?: stickyWindowFallbacks.getOrNull(index)?.takeIf { path ->
                            sessionDocumentPaths?.contains(path) ?: path.endsWith(".md")
                        }
                }
                .filterNotNull()
                .distinct()

        /**
         * GH #320: the session documents a Remote Dev backend can see. Its own
         * `FileEditorManager.selectedFiles` often names no session document (`activeWindow=<none>`),
         * so deferring on it threw the remote-client state away and a split change reached tmux only
         * on the next Run. The remote clients' visible and selected session documents (project
         * relative, mapped back to absolute paths through the open session documents) are the same
         * focus/liveness evidence used for diagnosis, so they join the backend's own selection;
         * they never become structural columns.
         */
        fun remoteClientVisibleSessionFiles(
            backendVisible: List<String>,
            clientVisible: List<List<String>>,
            clientSelected: List<List<String>>,
            absoluteByRelative: Map<String, String>,
        ): List<String> =
            sequenceOf(
                    backendVisible.asSequence(),
                    clientVisible.asSequence().flatten().mapNotNull(absoluteByRelative::get),
                    clientSelected.asSequence().flatten().mapNotNull(absoluteByRelative::get),
                )
                .flatten()
                .filter(String::isNotBlank)
                .distinct()
                .toList()

        fun resolveActiveFilePath(
            preferredActiveFile: String?,
            selectedEditorFile: String?,
            visibleMdFiles: List<String>,
        ): String? {
            if (!preferredActiveFile.isNullOrBlank()) {
                return preferredActiveFile
            }
            if (!selectedEditorFile.isNullOrBlank()) {
                return selectedEditorFile
            }
            return visibleMdFiles.firstOrNull()
        }

        fun tabsByProximity(focusedFile: String, tabs: List<String>): List<String> {
            val focusedIndex = tabs.indexOf(focusedFile).takeIf { it >= 0 } ?: 0
            return tabs
                .withIndex()
                .sortedWith(
                    compareBy<IndexedValue<String>>(
                        { kotlin.math.abs(it.index - focusedIndex) },
                        { it.index },
                    )
                )
                .map { it.value }
                .distinct()
        }

        fun prioritizeOpenDocuments(
            focusedFile: String,
            nearbyTabs: List<String>,
            visibleMdFiles: List<String>,
            openMdFiles: List<String>,
        ): List<String> =
            sequenceOf(
                    sequenceOf(focusedFile),
                    nearbyTabs.asSequence(),
                    visibleMdFiles.asSequence(),
                    openMdFiles.asSequence(),
                )
                .flatten()
                .filter(String::isNotBlank)
                .distinct()
                .toList()

        /**
         * Build the observation. An undetected layout reports **no** columns rather than a
         * synthesized single column, so the graph can tell "the editor has one column" apart from
         * "the editor could not see its layout" and skip the drift comparison in the latter case.
         */
        fun buildSurface(
            focusedFile: String,
            visibleMdFiles: List<String>,
            openMdFiles: List<String> = visibleMdFiles,
            editorLayout: EditorLayout?,
            forceReconcile: Boolean,
            preserveFocus: Boolean = false,
        ): EditorSurfacePayload =
            EditorSurfacePayload(
                focused = focusedFile,
                visible = visibleMdFiles.distinct(),
                open =
                    prioritizeOpenDocuments(
                        focusedFile = focusedFile,
                        nearbyTabs = openMdFiles,
                        visibleMdFiles = visibleMdFiles,
                        openMdFiles = emptyList(),
                    ),
                columns =
                    editorLayout
                        ?.columns
                        ?.map { column ->
                            SurfaceColumnPayload(column.files.filter { it.isNotBlank() }.distinct())
                        }
                        ?.filter { it.files.isNotEmpty() }
                        .orEmpty(),
                forceReconcile = forceReconcile,
                focusOnly = false,
                preserveFocus = preserveFocus,
                columnOrder = editorLayout?.columnOrder ?: "editor",
            )

        /**
         * Selected-document state published to that document's own controller.
         *
         * This is intentionally a separate retained source from the spanning layout surface:
         * cross-root editor splits need the subproject controller to own pane selection, but the
         * narrow payload must never be mistaken for a one-column layout replacement.
         */
        fun buildFocusProjection(focusedFile: String): EditorSurfacePayload =
            EditorSurfacePayload(
                focused = focusedFile,
                visible = listOf(focusedFile),
                open = listOf(focusedFile),
                columns = emptyList(),
                forceReconcile = true,
                focusOnly = true,
                preserveFocus = false,
            )
    }

    private fun log(msg: String) {
        LOG.debug("[layout-sync] $msg")
    }

    /** A projection the derivation deferred: the editor has not settled into a publishable surface. */
    private fun captureDeferred(reason: String) {
        LOG.info("[layout-sync] capture deferred reason=$reason")
    }

    /** Call only while holding [lifecycleLock]. */
    private fun invalidateFocusProjection(expectedGeneration: Long? = null) {
        if (
            expectedGeneration != null &&
                focusProjectionGeneration.get() != expectedGeneration
        ) {
            return
        }
        focusProjectionGeneration.incrementAndGet()
        if (activeFocusProjection.getAndSet(null)?.cancel(true) == true) {
            LOG.info(
                "[focus] focus projection gated gate=${if (closed) "closed" else "superseded"} " +
                    "phase=invalidate",
            )
        }
    }

    private fun requestObservation(
        observation: PendingSurfaceObservation,
        requiredFocusGeneration: Long? = null,
    ) {
        var retainedPending = false
        synchronized(lifecycleLock) {
            if (closed) return
            if (
                requiredFocusGeneration != null &&
                    focusProjectionGeneration.get() != requiredFocusGeneration
            ) {
                log(
                    "observe: rejected superseded focus generation " +
                        "required=$requiredFocusGeneration current=${focusProjectionGeneration.get()}",
                )
                return
            }
            while (true) {
                val current = latestSurfaceObservation.get()
                if (
                    !SurfaceObservationOrdering.shouldReplace(
                        currentAuthority = current?.authority,
                        incomingAuthority = observation.authority,
                    )
                ) {
                    log(
                        "observe: retained pending ${current?.authority} over " +
                            "${observation.authority}",
                    )
                    if (requiredFocusGeneration != null) {
                        invalidateFocusProjection(requiredFocusGeneration)
                    }
                    // GH #326: the pending observation stays in the slot until the delivery worker
                    // derives a ready projection, so its capture may predate this edge. Re-read it.
                    retainedPending = true
                    break
                }
                if (latestSurfaceObservation.compareAndSet(current, observation)) break
            }
            if (!retainedPending) {
                if (requiredFocusGeneration == null && !observation.preserveFocus) {
                    // Only an admitted focus-authoritative observation may supersede the focus
                    // effect. Passive layout facts neither cancel nor replace explicit pane selection.
                    invalidateFocusProjection()
                }
            }
        }
        val requested = generation.incrementAndGet()
        surfaceDeliveryRetryAttempt.set(0)
        projectLatestSurfaceOnEditorThread(requested)
    }

    private fun scheduleRetainedSurfaceRetry(
        requestedGeneration: Long,
        observation: PendingSurfaceObservation,
    ) {
        if (
            closed ||
                generation.get() != requestedGeneration ||
                latestSurfaceObservation.get() !== observation
        ) {
            return
        }
        val attempt = surfaceDeliveryRetryAttempt.incrementAndGet()
        val delayMs = SurfaceDeliveryRetry.delayMs(attempt)
        try {
            surfaceDeliveryExecutor.schedule(
                {
                    if (
                        !closed &&
                            generation.get() == requestedGeneration &&
                            latestSurfaceObservation.get() === observation
                    ) {
                        projectLatestSurfaceOnEditorThread(requestedGeneration)
                    }
                },
                delayMs,
                TimeUnit.MILLISECONDS,
            )
            log("observe: retained retry attempt=$attempt delay_ms=$delayMs")
        } catch (_: RejectedExecutionException) {
            // Project disposal owns executor shutdown; there is no surface left to deliver.
        }
    }

    private fun projectLatestSurfaceOnEditorThread(requestedGeneration: Long) {
        projectLatestSurfaceOnEditorThread(
            requestedGeneration,
            SelectionProjectionSettling.MAX_REPROJECTION_PASSES,
        )
    }

    private fun projectLatestSurfaceOnEditorThread(
        requestedGeneration: Long,
        remainingSelectionPasses: Int,
    ) {
        GenerationWork.invokeLater {
            try {
                if (generation.get() != requestedGeneration) {
                    log("editor projection: superseded gen=$requestedGeneration")
                    return@invokeLater
                }
                val observation = latestSurfaceObservation.get() ?: return@invokeLater
                // GH #326: the EDT only captures evidence; the delivery worker derives what it means.
                val captured =
                    captureSurface(
                        project = observation.project,
                        preferredFile = observation.preferredFile,
                        previousFile = observation.previousFile,
                        forceReconcile = observation.forceReconcile,
                        preserveFocus = observation.preserveFocus,
                        authority = observation.authority,
                        remainingSelectionPasses = remainingSelectionPasses,
                    )
                val requestedAuthorityEpoch = projectionAuthorityEpoch.get()
                surfaceDeliveryExecutor.execute {
                    try {
                        deliverCapturedSurface(
                            captured = captured,
                            observation = observation,
                            requestedGeneration = requestedGeneration,
                            requestedAuthorityEpoch = requestedAuthorityEpoch,
                            remainingSelectionPasses = remainingSelectionPasses,
                        )
                    } catch (e: Exception) {
                        LOG.warn("[layout-sync] surface delivery failed: ${e.message}", e)
                    }
                }
            } catch (e: Exception) {
                LOG.warn("[layout-sync] observation failed: ${e.message}")
            }
        }
    }

    /**
     * The delivery worker's half of one projection pass: derive the surface, settle a deferral, or
     * publish. A deferred projection keeps the observation pending (it was never released), so the
     * next pass re-reads the editor on a later EDT turn exactly as the EDT capture used to.
     */
    private fun deliverCapturedSurface(
        captured: CapturedSurface,
        observation: PendingSurfaceObservation,
        requestedGeneration: Long,
        requestedAuthorityEpoch: Long,
        remainingSelectionPasses: Int,
    ) {
        if (generation.get() != requestedGeneration) {
            log("socket delivery: superseded gen=$requestedGeneration")
            return
        }
        val pending =
            when (
                val resolution =
                    try {
                        resolveSurface(captured)
                    } catch (e: Exception) {
                        LOG.warn("[layout-sync] background surface resolution failed: ${e.message}")
                        return
                    }
            ) {
                is SurfaceResolution.Ready -> resolution.pending
                is SurfaceResolution.Deferred -> {
                    captureDeferred(resolution.reason)
                    // The selection event can precede IDEA's visible-editor projection. Re-read on a
                    // bounded later EDT turn so a single tab switch is a complete reactive edge even
                    // when IDEA emits no follow-up event.
                    if (
                        SelectionProjectionSettling.shouldReproject(
                            authority = observation.authority,
                            remainingPasses = remainingSelectionPasses,
                        )
                    ) {
                        log("observe: editor projection settling; remaining=$remainingSelectionPasses")
                        projectLatestSurfaceOnEditorThread(
                            requestedGeneration,
                            remainingSelectionPasses - 1,
                        )
                    } else {
                        val missingEndpoints =
                            captured.liveSurfaceIds?.let(surfaceEndpoints::missing).orEmpty()
                        if (missingEndpoints.isNotEmpty()) {
                            retireClosedSurfaceEndpoints(observation.project, missingEndpoints)
                        }
                        log("observe: retained until editor-surface dependency changes")
                    }
                    return
                }
            }
        if (
            !ObservationDeliveryOwnership.releaseForDelivery(
                latestSurfaceObservation,
                observation,
            )
        ) {
            log("socket delivery: observation superseded before delivery gen=$requestedGeneration")
            return
        }
        synchronized(lifecycleLock) {
            if (closed) return
            surfaceRoots.recordAttempts(pending.knownControllerRoots)
        }
        val rootsToRetire =
            synchronized(lifecycleLock) {
                if (closed) {
                    emptyList()
                } else {
                    surfaceRoots.rootsToRetireBeforePublishing(pending.projectRoot)
                }
            }
        for (obsoleteRoot in rootsToRetire) {
            val surfaceForgotten = CpRouteClient.forgetEditorSurface(obsoleteRoot)
            CpRouteClient.forgetEditorFocus(obsoleteRoot)
            if (surfaceForgotten) {
                surfaceRoots.markForgotten(obsoleteRoot)
                log(
                    "observe: retired superseded surface root before publish " +
                        "root=$obsoleteRoot active=${pending.projectRoot}",
                )
            } else {
                log(
                    "observe: pre-publish surface root retirement deferred root=" +
                        "$obsoleteRoot active=${pending.projectRoot}",
                )
            }
        }
        val receipt =
            CpRouteClient.observeEditorSurface(
                projectRoot = pending.projectRoot,
                surfaceJson = pending.surfaceJson,
                surfaceId = pending.surfaceId,
                mayReregister = ::projectionLive,
            )
        if (receipt.exitCode != 0) {
            val retained =
                if (generation.get() == requestedGeneration) {
                    ObservationDeliveryOwnership.retainAfterFailure(
                        latestSurfaceObservation,
                        observation,
                    )
                } else {
                    false
                }
            LOG.warn(
                "[layout-sync] surface observation unavailable for " +
                    "${pending.relativePath}: ${receipt.output}",
            )
            if (retained || latestSurfaceObservation.get() === observation) {
                scheduleRetainedSurfaceRetry(requestedGeneration, observation)
            }
            return
        }
        surfaceDeliveryRetryAttempt.set(0)
        val publishedEndpoint = SurfaceEndpoint(pending.projectRoot, pending.surfaceId)
        // Decide under the lock that retireControllerProjection drains under: either this endpoint is
        // recorded before the drain (and the handoff forgets it), or the handoff already happened and
        // this delivery retires its own endpoint.
        val retiredEndpoints =
            synchronized(lifecycleLock) {
                when (
                    InFlightSurfaceDelivery.decide(
                        requestedAuthorityEpoch = requestedAuthorityEpoch,
                        currentAuthorityEpoch = projectionAuthorityEpoch.get(),
                    )
                ) {
                    InFlightSurfaceDelivery.Decision.RetireEndpoint -> null
                    InFlightSurfaceDelivery.Decision.RecordEndpoint ->
                        surfaceEndpoints.markPublished(
                            publishedEndpoint,
                            pending.liveSurfaceIds,
                        )
                }
            }
        if (retiredEndpoints == null) {
            // A frontend-authority handoff retired this listener's projection while the controller
            // call was in flight; the handoff's own drain could not see this endpoint, so retire it
            // here before the frontend source proceeds.
            CpRouteClient.forgetEditorSurface(pending.projectRoot, pending.surfaceId)
            CpRouteClient.forgetEditorFocus(pending.projectRoot, pending.surfaceId)
            log(
                "observe: retired in-flight endpoint after authority handoff " +
                    "root=${pending.projectRoot} surface=${pending.surfaceId}",
            )
            return
        }
        retireClosedSurfaceEndpoints(observation.project, retiredEndpoints)
        val obsoleteRoots =
            synchronized(lifecycleLock) {
                if (closed) {
                    null
                } else {
                    surfaceRoots.markPublished(pending.projectRoot)
                }
            }
        if (obsoleteRoots == null) {
            CpRouteClient.forgetEditorSurface(pending.projectRoot)
            CpRouteClient.forgetEditorFocus(pending.projectRoot)
            surfaceRoots.markForgotten(pending.projectRoot)
            return
        }
        for (obsoleteRoot in obsoleteRoots) {
            val surfaceForgotten = CpRouteClient.forgetEditorSurface(obsoleteRoot)
            CpRouteClient.forgetEditorFocus(obsoleteRoot)
            if (surfaceForgotten) {
                surfaceRoots.markForgotten(obsoleteRoot)
                log(
                    "observe: retired superseded surface root=$obsoleteRoot " +
                        "active=${pending.projectRoot}",
                )
            } else {
                log(
                    "observe: superseded surface root retirement deferred root=" +
                        "$obsoleteRoot active=${pending.projectRoot}",
                )
            }
        }
        log("observe: published file=${pending.relativePath}")
    }

    private fun retireClosedSurfaceEndpoints(
        project: Project,
        endpoints: List<SurfaceEndpoint>,
    ) {
        for (endpoint in endpoints) {
            CpRouteClient.forgetEditorSurface(endpoint.projectRoot, endpoint.surfaceId)
            val focusRetirement =
                CpRouteClient.forgetEditorFocusWithReceipt(
                    endpoint.projectRoot,
                    endpoint.surfaceId,
                )
            if (focusRetirement.exitCode != 0) continue
            GenerationWork.invokeLater {
                if (!project.isDisposed) {
                    IdeTerminalHost.applySurfaceDecision(project, focusRetirement.output)
                }
            }
            surfaceEndpoints.markForgotten(endpoint)
            log(
                "observe: retired closed editor surface " +
                    "root=${endpoint.projectRoot} surface=${endpoint.surfaceId}",
            )
        }
    }

    /**
     * GH #326 phase 2: read what the editor IS on this EDT turn, and nothing more. Every rule that
     * turns this evidence into a surface runs on the delivery worker through the hot-reloaded
     * native `surface_projection` derivation ([SurfaceProjection.derive]).
     */
    private fun captureSurface(
        project: Project,
        preferredFile: VirtualFile? = null,
        previousFile: VirtualFile? = null,
        forceReconcile: Boolean = false,
        preserveFocus: Boolean = false,
        authority: ObservationAuthority? = null,
        remainingSelectionPasses: Int? = null,
    ): CapturedSurface {
        val manager = FileEditorManager.getInstance(project)
        val managerEx = FileEditorManagerEx.getInstanceEx(project)
        val editorWindows = managerEx.windows
        // Which frame this observation reads: the window the event selected, else the current one.
        val anchorWindow =
            managerEx.currentWindow
                ?.takeIf { current ->
                    preferredFile == null || current.selectedFile?.path == preferredFile.path
                }
                ?: preferredFile?.let { preferred ->
                    editorWindows.firstOrNull { it.selectedFile?.path == preferred.path }
                }
                ?: managerEx.currentWindow
                ?: editorWindows.firstOrNull()
        val editorSurface = JetBrainsEditorSurfaces.forEditorWindow(project, anchorWindow)
        val liveSurfaceIds =
            JetBrainsEditorSurfaces.liveSurfaceIds(project, editorSurface.surfaceId)
        val surfaceWindows =
            anchorWindow?.owner?.let { owner ->
                editorWindows.filter { it.owner === owner }
            } ?: editorWindows.toList()
        val backendOpenSessionFiles = manager.openFiles.filter(AgentDocSessionFiles::isSessionDocument)
        // GH #177: Remote Dev client evidence is captured with the backend windows; the derivation
        // decides which of the two owns the layout.
        // GH #334: classify the client-scoped files through ClientFileEditorManager. Restricting
        // this read to the backend manager's open set erased the whole Remote Dev surface.
        val remoteLayoutCandidate = LayoutDetector.snapshotRemoteLayout(project)
        val allOpenSessionDocumentPaths =
            (backendOpenSessionFiles.map { it.path } +
                LayoutDetector.remoteSessionDocumentPaths(remoteLayoutCandidate))
                .distinct()
        val allSessionDocumentPaths = allOpenSessionDocumentPaths.toSet()
        val windows =
            try {
                LayoutDetector.captureEditorWindows(project, surfaceWindows, sessionDocumentPaths = null)
            } catch (e: Exception) {
                LOG.warn("[layout-detect] editor window evidence unavailable: ${e.message}", e)
                LayoutDetector.CapturedEditorWindows(
                    evidence =
                        LayoutDetector.WindowLayoutEvidence(
                            basePath = project.basePath,
                            windows =
                                surfaceWindows.map { window ->
                                    LayoutDetector.EditorWindowEvidence(
                                        x = null,
                                        y = null,
                                        selected = window.selectedFile?.path,
                                        sessionTabs =
                                            window.fileList
                                                .filter(AgentDocSessionFiles::isSessionDocument)
                                                .map { it.path },
                                    )
                                },
                            history = emptyList(),
                        ),
                    geometryUnavailable = true,
                )
            }
        val evidence =
            SurfaceProjection.SurfaceProjectionEvidence(
                basePath = project.basePath,
                backendWindowCount = editorWindows.size,
                windows = windows.evidence.windows,
                windowGeometryUnavailable = windows.geometryUnavailable,
                history = windows.evidence.history,
                managerSelected = manager.selectedFiles.map { it.path },
                openSessionDocuments = allOpenSessionDocumentPaths,
                remote =
                    SurfaceProjection.RemoteSurfaceEvidence(
                        projectRoot = remoteLayoutCandidate.projectRoot,
                        clients = remoteLayoutCandidate.clients,
                        focused = remoteLayoutCandidate.focusedSessionFiles,
                    ),
                measuredLayoutAuthority = RemoteClientLayoutAuthority.isActive(project),
                anchorSelectedSession =
                    anchorWindow?.selectedFile
                        ?.takeIf(AgentDocSessionFiles::isSessionDocument)
                        ?.path,
                selectedTextEditorSession =
                    manager.selectedTextEditor?.virtualFile
                        ?.takeIf(AgentDocSessionFiles::isSessionDocument)
                        ?.path,
                preferred = preferredFile?.path,
                preferredSessionDocument =
                    preferredFile?.let { it.isValid && AgentDocSessionFiles.isSessionDocument(it) } == true,
                previous = previousFile?.path,
                authority = authority?.let(SurfaceProjection::wireAuthority),
                remainingSelectionPasses = remainingSelectionPasses,
                forceReconcile = forceReconcile,
                preserveFocus = preserveFocus,
            )
        return CapturedSurface(
            project = project,
            projectBasePath = project.basePath,
            surfaceId = editorSurface.surfaceId,
            liveSurfaceIds = liveSurfaceIds,
            evidence = evidence,
        )
    }

    /** What the delivery worker derived from one capture. */
    private sealed interface SurfaceResolution {
        data class Ready(val pending: PendingSurface) : SurfaceResolution

        data class Deferred(val reason: String) : SurfaceResolution
    }

    /**
     * Derive and route one capture. Off the EDT: the native `surface_projection` derivation decides
     * the surface, and the native `sync_layout` derivation (GH #326 phase 3) chooses the filesystem
     * root that spans the visible layout and normalizes the columns against it.
     */
    private fun resolveSurface(captured: CapturedSurface): SurfaceResolution {
        val surface =
            when (val projection = SurfaceProjection.derive(captured.evidence)) {
                is SurfaceProjection.Result.Deferred -> return SurfaceResolution.Deferred(projection.reason)
                is SurfaceProjection.Result.Ready -> projection.surface.deepCopy()
            }
        fun strings(name: String): List<String> =
            surface.getAsJsonArray(name)?.map { it.asString }.orEmpty()
        val focusedFile = surface.get("focused")?.takeIf { !it.isJsonNull }?.asString.orEmpty()
        if (focusedFile.isBlank()) {
            return SurfaceResolution.Deferred("surface_projection_without_focused_document")
        }
        val visibleMdFiles = strings("visible")
        val (focusedProjectRoot, focusedRelativePath) =
            TerminalUtil.resolveProject(captured.project, focusedFile)
        val derivedLayout =
            surface.getAsJsonArray("columns")
                ?.map { column ->
                    LayoutColumn(
                        column.asJsonObject.getAsJsonArray("files")?.map { it.asString }.orEmpty(),
                    )
                }
                ?.takeIf { it.isNotEmpty() }
                ?.let { columns ->
                    EditorLayout(
                        columns,
                        surface.get("column_order")?.takeIf { !it.isJsonNull }?.asString ?: "editor",
                    )
                }
        // One root keys the surface graph, and it has to be the one that spans
        // the whole visible layout — a surface is the layout, not one document.
        // The surface carries its own focus, so no exact-visible refusal applies.
        val rooted =
            SyncLayoutSnapshot.derive(
                SyncLayoutSnapshot.evidence(
                    basePath = captured.projectBasePath,
                    fallbackRoot = focusedProjectRoot,
                    focused = focusedFile,
                    visible = visibleMdFiles,
                    editorLayout = derivedLayout,
                    foreground = false,
                ),
            )
        val surfaceProjectRoot = rooted.projectRoot
        val knownControllerRoots =
            (
                strings("open").mapNotNull(TerminalUtil::nearestAgentDocProjectRoot) +
                    surfaceProjectRoot
            ).distinct()
        val absoluteEditorLayout = rooted.editorLayout
        surface.add(
            "columns",
            GSON.toJsonTree(
                absoluteEditorLayout?.columns
                    ?.map { SurfaceColumnPayload(it.files) }
                    ?.filter { it.files.isNotEmpty() }
                    .orEmpty(),
            ),
        )
        return SurfaceResolution.Ready(
            PendingSurface(
                projectRoot = surfaceProjectRoot,
                relativePath = focusedRelativePath,
                surfaceId = captured.surfaceId,
                surfaceJson = GSON.toJson(surface),
                columnGeometry =
                    if (absoluteEditorLayout?.columns?.any { it.files.isNotEmpty() } == true) {
                        SurfaceCommandReference.ColumnGeometry.Known
                    } else {
                        SurfaceCommandReference.ColumnGeometry.Unknown
                    },
                knownControllerRoots = knownControllerRoots,
                liveSurfaceIds = captured.liveSurfaceIds,
            ),
        )
    }

    /**
     * An EDT capture of the current surface for an explicit editor command (Run Agent Doc, Sync
     * Tmux Panes), published later by [publishCommandSurface] on the command's background thread.
     */
    internal class CommandSurfaceCapture internal constructor(internal val captured: CapturedSurface)

    /** What [publishCommandSurface] retained on the controller, or why the command keeps its snapshot. */
    internal sealed interface CommandSurfacePublication {
        data class Published(
            val projectRoot: String,
            val reference: SurfaceLayoutReference,
        ) : CommandSurfacePublication

        /** The flat surface was retained, but exact structure remains unchanged by policy. */
        data class PreservedLayout(
            val projectRoot: String,
            val reference: SurfaceLayoutReference,
        ) : CommandSurfacePublication

        data class Unavailable(val reason: String) : CommandSurfacePublication
    }

    /**
     * Capture the current surface for an explicit command. EDT only: this reads the same editor
     * state, through the same [captureSurface], as the automatic capture. Null when this listener
     * no longer owns its projection (a Remote Dev frontend surface took authority). An editor
     * projection that has not settled yet is [publishCommandSurface]'s `surface_capture_deferred`.
     */
    internal fun captureCommandSurface(
        project: Project,
        preferredFile: VirtualFile?,
    ): CommandSurfaceCapture? {
        if (!projectionLive()) return null
        return CommandSurfaceCapture(captureSurface(project = project, preferredFile = preferredFile))
    }

    /**
     * Publish [capture] as this listener's surface Source on the controller at [commandRoot], so
     * the command can name it instead of sending a `--col` snapshot (`surface_layout_reference_v1`,
     * `specs/13-state-backbone.md` "State Propagation, Not Effect Inference"). Background thread.
     *
     * The publication is an ordinary ordered observation of this listener's endpoint: it folds
     * into the same graph as every automatic observation, and its endpoint bookkeeping is the
     * serialized delivery's (record it, or retire it when an authority handoff overtook it). It
     * never publishes to a root other than the one this listener's projection is active on, so it
     * cannot open a second controller projection of the same surface.
     */
    internal fun publishCommandSurface(
        capture: CommandSurfaceCapture,
        commandRoot: String,
        command: SurfaceCommandReference.Command,
    ): CommandSurfacePublication {
        if (!projectionLive()) return CommandSurfacePublication.Unavailable("listener_projection_retired")
        val captured = capture.captured
        val pending =
            try {
                when (val resolution = resolveSurface(captured)) {
                    is SurfaceResolution.Ready -> resolution.pending
                    is SurfaceResolution.Deferred ->
                        return CommandSurfacePublication.Unavailable(
                            "surface_capture_deferred reason=${resolution.reason}",
                        )
                }
            } catch (e: Exception) {
                return CommandSurfacePublication.Unavailable("surface_resolution_failed: ${e.message}")
            }
        if (pending.projectRoot != commandRoot) {
            return CommandSurfacePublication.Unavailable(
                "cross_root surface_root=${pending.projectRoot} command_root=$commandRoot",
            )
        }
        if (surfaceRoots.active() != pending.projectRoot) {
            return CommandSurfacePublication.Unavailable(
                "surface_root_not_active root=${pending.projectRoot} active=${surfaceRoots.active()}",
            )
        }
        val requestedAuthorityEpoch = projectionAuthorityEpoch.get()
        val receipt =
            CpRouteClient.observeEditorSurface(
                projectRoot = pending.projectRoot,
                surfaceJson = pending.surfaceJson,
                surfaceId = pending.surfaceId,
                mayReregister = ::projectionLive,
            )
        if (receipt.exitCode == 0) {
            val endpoint = SurfaceEndpoint(pending.projectRoot, pending.surfaceId)
            val retiredEndpoints =
                synchronized(lifecycleLock) {
                    when (
                        InFlightSurfaceDelivery.decide(
                            requestedAuthorityEpoch = requestedAuthorityEpoch,
                            currentAuthorityEpoch = projectionAuthorityEpoch.get(),
                        )
                    ) {
                        InFlightSurfaceDelivery.Decision.RetireEndpoint -> null
                        InFlightSurfaceDelivery.Decision.RecordEndpoint ->
                            if (closed) null else surfaceEndpoints.markPublished(endpoint, pending.liveSurfaceIds)
                    }
                }
            if (retiredEndpoints == null) {
                CpRouteClient.forgetEditorSurface(pending.projectRoot, pending.surfaceId)
                CpRouteClient.forgetEditorFocus(pending.projectRoot, pending.surfaceId)
                return CommandSurfacePublication.Unavailable("authority_handoff_during_publish")
            }
            retireClosedSurfaceEndpoints(captured.project, retiredEndpoints)
        }
        return when (
            val decision =
                SurfaceCommandReference.decide(
                    surfaceRoot = pending.projectRoot,
                    commandRoot = commandRoot,
                    surfaceId = pending.surfaceId,
                    publishExitCode = receipt.exitCode,
                    receiptJson = receipt.output,
                    publishedSequence = receipt.surfaceSequence,
                    command = command,
                    columnGeometry = pending.columnGeometry,
                )
        ) {
            is SurfaceCommandReference.Decision.Reference ->
                CommandSurfacePublication.Published(pending.projectRoot, decision.reference)
            is SurfaceCommandReference.Decision.PreserveRetainedLayout ->
                CommandSurfacePublication.PreservedLayout(pending.projectRoot, decision.reference)
            is SurfaceCommandReference.Decision.SnapshotFallback ->
                CommandSurfacePublication.Unavailable(decision.reason)
        }
    }

    private fun requestFocusProjection(
        project: Project,
        file: VirtualFile,
        remoteControllerSelectionFocusEvidence: Boolean = false,
    ): Long? =
        synchronized(lifecycleLock) {
            if (closed) {
                LOG.info(
                    "[focus] focus projection gated gate=closed phase=request file=${file.path}",
                )
                return@synchronized null
            }
            val requestedGeneration = focusProjectionGeneration.incrementAndGet()
            // FileEditorManagerEx.currentWindow and editor-window surface discovery are EDT-only
            // platform reads. Capture them while handling the editor event, then carry the stable
            // surface id into the delayed transport task.
            val focusWindow = FileEditorManagerEx.getInstanceEx(project).let { manager ->
                manager.currentWindow?.takeIf { it.selectedFile?.path == file.path }
                    ?: manager.windows.firstOrNull { it.selectedFile?.path == file.path }
            }
            val focusSurfaceId =
                JetBrainsEditorSurfaces.forEditorWindow(project, focusWindow).surfaceId
            try {
                val scheduled =
                    focusProjectionExecutor.schedule(
                    focus@{
                        try {
                            fun publishGate(): FocusProjectionPublicationGate =
                                focusProjectionPublicationGate(
                                    requestedGeneration = requestedGeneration,
                                    currentGeneration = focusProjectionGeneration.get(),
                                    projectWindowActive =
                                        JetBrainsEditorSurfaces.isProjectSurfaceActive(project),
                                    remoteControllerSelectionFocusEvidence =
                                        remoteControllerSelectionFocusEvidence,
                                    listenerClosed = closed,
                                    projectDisposed = project.isDisposed,
                                )

                            fun continueAfterGate(phase: String): Boolean {
                                val gate = publishGate()
                                if (gate == FocusProjectionPublicationGate.Ready) return true
                                LOG.info(
                                    "[focus] focus projection gated gate=${gate.logName} " +
                                        "phase=$phase file=${file.path} gen=$requestedGeneration",
                                )
                                return false
                            }

                            if (!continueAfterGate("before_root_resolution")) {
                                return@focus
                            }

                            // The focused document, not the spanning editor surface, owns the
                            // controller root for this immediate handoff. This is what lets a
                            // superproject document and submodule document share one visible tmux
                            // window without routing both focus intents through one controller.
                            val (projectRoot, _) = TerminalUtil.resolveProject(project, file)
                            if (!continueAfterGate("after_root_resolution")) {
                                return@focus
                            }

                            val request =
                                FocusProjectionEffectRequest(
                                    generation = requestedGeneration,
                                    projectRoot = projectRoot,
                                    filePath = file.path,
                                    surfaceId = focusSurfaceId,
                                    surfaceJson =
                                        GSON.toJson(SurfaceReport.buildFocusProjection(file.path)),
                                    remoteControllerSelectionFocusEvidence =
                                        remoteControllerSelectionFocusEvidence,
                                )
                            val response =
                                FocusProjectionEffectResponse(
                                    request = request,
                                    transport =
                                        CpRouteClient.observeEditorFocus(
                                            projectRoot = request.projectRoot,
                                            surfaceJson = request.surfaceJson,
                                            surfaceId = request.surfaceId,
                                            mayReregister = ::projectionLive,
                                        ),
                                )
                            if (!continueAfterGate("after_transport")) {
                                return@focus
                            }
                            synchronized(lifecycleLock) {
                                if (response.transport.exitCode == 0) {
                                    surfaceEndpoints.record(
                                        SurfaceEndpoint(request.projectRoot, request.surfaceId),
                                    )
                                }
                                when (
                                    decideFocusProjectionReceipt(
                                        response = response,
                                        currentGeneration = focusProjectionGeneration.get(),
                                        projectWindowActive =
                                            JetBrainsEditorSurfaces.isProjectSurfaceActive(project),
                                    )
                                ) {
                                    FocusProjectionReceiptDecision.Applied -> {
                                        // Reverse tmux→editor mirroring is suppressed only after the
                                        // controller proves that this exact retained projection selected
                                        // the pane. A missing actor must not install a stale lease.
                                        TmuxPaneFocusSync.recordEditorFocusIntent(project, request.filePath)
                                        GenerationWork.invokeLater {
                                            if (!project.isDisposed) {
                                                IdeTerminalHost.applySurfaceDecision(
                                                    project,
                                                    response.transport.output,
                                                )
                                            }
                                        }
                                        LOG.info(
                                            "[focus] focus projection completed outcome=applied " +
                                                "file=${request.filePath} gen=$requestedGeneration",
                                        )
                                    }

                                    FocusProjectionReceiptDecision.RepairLayout -> {
                                        // A late receipt must not replace a newer document's surface.
                                        requestObservation(
                                            PendingSurfaceObservation(
                                                project = project,
                                                preferredFile = file,
                                                forceReconcile = true,
                                                authority = ObservationAuthority.EditorFocus,
                                            ),
                                            requiredFocusGeneration = requestedGeneration,
                                        )
                                        LOG.info(
                                            "[focus] focus projection completed outcome=repair_layout " +
                                                "file=${request.filePath} gen=$requestedGeneration",
                                        )
                                    }

                                    FocusProjectionReceiptDecision.Superseded -> {
                                        LOG.info(
                                            "[focus] focus projection completed outcome=superseded_receipt " +
                                                "file=${request.filePath} gen=$requestedGeneration",
                                        )
                                        return@focus
                                    }

                                    FocusProjectionReceiptDecision.Retained -> {
                                        LOG.info(
                                            "[focus] focus projection completed outcome=retained " +
                                                "file=${request.filePath} gen=$requestedGeneration " +
                                                "receipt=${response.transport.output}",
                                        )
                                    }

                                    FocusProjectionReceiptDecision.TransportUnavailable -> {
                                        LOG.warn(
                                            "[focus] retained focus projection unavailable for " +
                                                "${request.filePath}: ${response.transport.output}",
                                        )
                                    }
                                }
                            }
                        } catch (error: Throwable) {
                            LOG.warn(
                                "[focus] focus projection task failed file=${file.path} " +
                                    "gen=$requestedGeneration",
                                error,
                            )
                        }
                    },
                    FOCUS_COALESCE_MS,
                    TimeUnit.MILLISECONDS,
                )
                if (activeFocusProjection.getAndSet(scheduled)?.cancel(true) == true) {
                    LOG.info(
                        "[focus] focus projection gated gate=superseded phase=coalesced " +
                            "superseded_by_gen=$requestedGeneration",
                    )
                }
            } catch (rejected: RejectedExecutionException) {
                if (!closed) {
                    LOG.warn(
                        "[focus] focus projection rejected while listener is active",
                        rejected,
                    )
                }
            }
            requestedGeneration
        }

    /**
     * Retires retained classic surface/focus sources without closing this listener.
     *
     * The modular Remote Dev backend uses this when an authenticated frontend snapshot becomes
     * authoritative. If that frontend later retires, the same listener can accept a fresh guest
     * selection edge and rebuild fallback ownership.
     */
    internal fun retireControllerProjection() {
        generation.incrementAndGet()
        invalidateFocusProjection()
        latestSurfaceObservation.set(null)
        val (roots, endpoints) = synchronized(lifecycleLock) {
            projectionRetired = true
            projectionAuthorityEpoch.incrementAndGet()
            surfaceRoots.drain() to surfaceEndpoints.drain()
        }
        GenerationWork.newThread(
                {
                    roots.forEach { root ->
                        CpRouteClient.forgetEditorSurface(root)
                        CpRouteClient.forgetEditorFocus(root)
                    }
                    endpoints.forEach { endpoint ->
                        CpRouteClient.forgetEditorSurface(endpoint.projectRoot, endpoint.surfaceId)
                        CpRouteClient.forgetEditorFocus(endpoint.projectRoot, endpoint.surfaceId)
                    }
                },
                "agent-doc-editor-surface-authority-handoff",
            )
            .apply {
                isDaemon = true
                start()
            }
    }

    /**
     * The Remote Dev backend selection fallback owns this listener's selection edge again (the
     * frontend surface authority went away). Frames this listener publishes from now on are live, so
     * the first observation into an endpoint the earlier handoff retired re-registers above the
     * retired generation instead of staying muted at the process generation (`#surfaceretireformal`,
     * model action `Fallback`). No-op for a classic listener, which is never retired.
     */
    internal fun resumeControllerProjection() {
        val resumed =
            synchronized(lifecycleLock) {
                val wasRetired = projectionRetired && !closed
                if (wasRetired) projectionRetired = false
                wasRetired
            }
        if (resumed) log("observe: controller projection resumed by the backend selection fallback")
    }

    private fun shutdown(policy: EditorSurfaceShutdownPolicy) {
        val (roots, endpoints) =
            synchronized(lifecycleLock) {
                closed = true
                invalidateFocusProjection()
                focusProjectionExecutor.shutdownNow()
                surfaceDeliveryExecutor.shutdownNow()
                surfaceRoots.drain() to surfaceEndpoints.drain()
            }
        latestSurfaceObservation.set(null)
        if (!policy.retireControllerProjection) {
            // Dynamic replacement keeps the controller's last acknowledged Source live. The new
            // classloader publishes the same client family with a newer generation, atomically
            // superseding it without an idle/null interval in tmux layout auto-sync.
            return
        }
        GenerationWork.newThread(
                {
                    for (root in roots) {
                        CpRouteClient.forgetEditorSurface(root)
                        CpRouteClient.forgetEditorFocus(root)
                    }
                    for (endpoint in endpoints) {
                        CpRouteClient.forgetEditorSurface(endpoint.projectRoot, endpoint.surfaceId)
                        CpRouteClient.forgetEditorFocus(endpoint.projectRoot, endpoint.surfaceId)
                    }
                },
                "agent-doc-editor-surface-forget",
            )
            .apply {
                isDaemon = true
                start()
            }
    }

override fun selectionChanged(event: FileEditorManagerEvent) {
    selectionChanged(event, remoteControllerSelectionFocusEvidence = false)
}

internal fun selectionChanged(
    event: FileEditorManagerEvent,
    remoteControllerSelectionFocusEvidence: Boolean,
) {
val selectionProbeGeneration = selectionFocusProbeGeneration.incrementAndGet()
val file = event.newFile ?: return
val project = event.manager.project
        if (!AgentDocSessionFiles.isSessionDocument(file)) {
            // A plain Markdown/source tab can occupy one editor split while that window still
            // stands for its last agent-doc tab. Reproject the spanning layout, but never route
            // focus or pane creation to the non-session file itself.
            requestObservation(
                PendingSurfaceObservation(
                    project = project,
                    preferredFile = null,
                    forceReconcile = false,
                    authority = ObservationAuthority.Layout,
                ),
            )
log("selectionChanged: non-session file=${file.name}; spanning projection queued")
return
}
val selectionManagerEx = FileEditorManagerEx.getInstanceEx(project)
val selectionWindowCount = selectionManagerEx.windows.size
val selectionActiveWindowPath = selectionManagerEx.currentWindow?.selectedFile?.path
val selectionFocusAuthority =
SelectionFocusAuthority.decide(
                selectionPath = file.path,
                activeWindowPath = selectionActiveWindowPath,
                previousSelectionPath = event.oldFile?.path,
            )
val selectionOwnsFocus =
selectionFocusAuthority == SelectionFocusAuthority.ActiveEditorSplit
// GH #88: an arriving selection is observable at INFO, so "never ran" and
// "ran and was gated" are distinguishable from an operator's idea.log.
LOG.info(
"[layout-sync] selection file=${file.name} authority=$selectionFocusAuthority " +
    "windows=$selectionWindowCount activeWindow=${selectionActiveWindowPath?.let { java.io.File(it).name } ?: "<none>"}",
)
val requestedFocusGeneration =
if (selectionOwnsFocus) {
requestFocusProjection(
    project,
    file,
    remoteControllerSelectionFocusEvidence = remoteControllerSelectionFocusEvidence,
)
} else {
null
}
if (requestedFocusGeneration != null) {
log("selectionChanged: active-split newFile=${file.name}; projection queued")
} else {
// IDEA emits selectionChanged for tabs in editor windows that do not own
// keyboard focus. That event is a valid spanning-layout edge, but treating
// its file as focused redirects tmux from the operator's active split.
log("selectionChanged: background-split newFile=${file.name}; layout-only projection queued")
scheduleSettledSelectionFocusProbe(project, file, selectionProbeGeneration)
}

// Focus owns targeted session recovery. Re-forcing a full surface
// reconcile on every tab switch made ordinary focus navigation contend
        // with layout sync and briefly expose stale extra panes.
        requestObservation(
PendingSurfaceObservation(
project = project,
preferredFile = file.takeIf { selectionOwnsFocus },
previousFile = event.oldFile.takeIf { selectionOwnsFocus },
forceReconcile = false,
authority =
if (selectionOwnsFocus) {
ObservationAuthority.DocumentSelection
} else {
ObservationAuthority.Layout
},
),
requiredFocusGeneration = requestedFocusGeneration,
)
}

    /**
     * A tab-chrome click can emit [selectionChanged] before IDEA moves [FileEditorManagerEx.currentWindow]
     * to the clicked split, and it need not emit an editor-component focus event afterward. Re-check
     * after IDEA's focus manager settles and claim focus only if that exact selection is now
     * authoritative. A generic next-EDT callback is too early on real split/tab transfers.
     * The selection generation prevents an older callback from stealing focus after a newer tab edge.
     */
    private fun scheduleSettledSelectionFocusProbe(
        project: Project,
        file: VirtualFile,
        requestedGeneration: Long,
    ) {
        IdeFocusManager.getInstance(project).doWhenFocusSettlesDown {
            if (closed || project.isDisposed) return@doWhenFocusSettlesDown
            val activeWindowPath =
                FileEditorManagerEx.getInstanceEx(project).currentWindow?.selectedFile?.path
            val projectWindowActive =
                JetBrainsEditorSurfaces.isProjectSurfaceActive(project)
            if (
                !shouldClaimSettledSelectionFocus(
                    selectionPath = file.path,
                    activeWindowPath = activeWindowPath,
                    requestedGeneration = requestedGeneration,
                    currentGeneration = selectionFocusProbeGeneration.get(),
                    projectWindowActive = projectWindowActive,
                )
            ) {
                return@doWhenFocusSettlesDown
            }
            log("selectionChanged: settled active-split newFile=${file.name}; focus projection queued")
            onEditorFocusGained(project, file)
        }
    }

    /**
     * Re-publish the settled visible surface when IDEA becomes active. i3 workspace changes do not
     * necessarily emit a selection, file-open, or layout event, even though the terminal tool
     * window may have been rebuilt while the IDE was hidden.
     */
    fun onIdeActivated(project: Project) {
        val activationGeneration = selectionFocusProbeGeneration.incrementAndGet()
        IdeFocusManager.getInstance(project).doWhenFocusSettlesDown {
            if (closed || project.isDisposed) return@doWhenFocusSettlesDown
            if (selectionFocusProbeGeneration.get() != activationGeneration) {
                return@doWhenFocusSettlesDown
            }
            val selectedFile =
                FileEditorManagerEx.getInstanceEx(project).currentWindow?.selectedFile
            if (
                JetBrainsEditorSurfaces.isProjectSurfaceActive(project) &&
                    selectedFile != null &&
                    AgentDocSessionFiles.isSessionDocument(selectedFile)
            ) {
                log("ideActivated: selected session file=${selectedFile.name}; focus projection queued")
                onEditorFocusGained(project, selectedFile)
                return@doWhenFocusSettlesDown
            }
            log("ideActivated: settled surface projection queued")
            requestObservation(
                PendingSurfaceObservation(
                    project = project,
                    preferredFile = null,
                    forceReconcile = false,
                    authority = ObservationAuthority.IdeActivation,
                ),
            )
        }
    }

    /**
     * The operator moved editor focus to [file] — e.g. clicked into the other split editor window
     * showing an already-open agent-doc document.
     *
     * #panefocussplit: [FileEditorManagerListener.selectionChanged] does NOT fire for focus
     * movement between two existing split editors (only for tab / visible-file-set changes), so
     * without this entry point split navigation never moves the tmux active pane.
     * [EditorFocusSyncListener] wires the per-editor focus events that call this.
     *
    * Focus events fire repeatedly for the same editor. The focus lane and surface generation guard
    * independently collapse those repeats. Republish the spanning surface as well as the targeted
    * focus: the fenced focus projection can recover the selected stashed pane, while the spanning
    * projection remains the exact-layout authority. IDEA does not emit [selectionChanged] when the
    * operator moves between existing splits, so both edges are required.
     */
    fun onEditorFocusGained(project: Project, file: VirtualFile) {
        selectionFocusProbeGeneration.incrementAndGet()
        if (!AgentDocSessionFiles.isSessionDocument(file)) {
            requestObservation(
                PendingSurfaceObservation(
                    project = project,
                    preferredFile = null,
                    forceReconcile = false,
                    authority = ObservationAuthority.Layout,
                ),
            )
            log("focusGained: non-session file=${file.name}; spanning projection queued")
            return
        }
        val requestedFocusGeneration = requestFocusProjection(project, file) ?: return
        requestObservation(
            PendingSurfaceObservation(
                project = project,
                preferredFile = file,
                forceReconcile = false,
                authority = ObservationAuthority.EditorFocus,
            ),
            requiredFocusGeneration = requestedFocusGeneration,
        )
        log("focusGained: file=${file.name}; spanning projection queued")
    }

    /**
     * A structural split/container change occurred. [LayoutChangeDetector] reports it through the
     * same surface graph as tab and focus events, keeping one layout planner and one debounce
     * owner.
     */
    fun onEditorLayoutChanged(project: Project) {
        val pendingSelection =
            latestSurfaceObservation.get()?.takeIf {
                it.project === project &&
                    it.authority == ObservationAuthority.DocumentSelection &&
                    it.preferredFile != null
            }
        requestObservation(
            pendingSelection
                ?: PendingSurfaceObservation(
                    project = project,
                    preferredFile = null,
                    forceReconcile = false,
                    authority = ObservationAuthority.Layout,
            ),
        )
    }

    /**
     * IDEA can restore editor containers before it finishes restoring their files. The delayed
     * structural seed then observes the right split count with an incomplete selected-file set,
     * and opening the restored files does not necessarily emit either `selectionChanged` or a new
     * container event. Publish the completed surface from the file lifecycle edge so startup does
     * not depend on an explicit Sync Tmux Layout action.
     */
    override fun fileOpened(source: FileEditorManager, file: VirtualFile) {
        if (!AgentDocSessionFiles.isSessionDocument(file)) {
            requestObservation(
                PendingSurfaceObservation(
                    project = source.project,
                    preferredFile = null,
                    forceReconcile = false,
                    authority = ObservationAuthority.Layout,
                ),
            )
            log("fileOpened: non-session file=${file.name}; spanning projection queued")
            return
        }
        log("fileOpened: file=${file.name}; start-before-surface queued")
        startOpenedSessionBeforeObservation(source.project, file)
    }

    /**
     * Opening a session document is a liveness edge as well as a layout edge. Start the document's
     * tmux owner before publishing its surface: publishing first lets the automatic layout effect
     * provision an idle `layout-provision` pane, after which an idempotent start sees an existing
     * owner and the reopened document never receives a real agent-doc start.
     *
     * Project-root resolution stays off the EDT. [IdeTerminalCoordinator] owns binary start
     * idempotence; this listener only preserves the causal order between that receipt and the
     * ordinary reactive surface publication. The automatic edge does not reveal or select a
     * terminal — the surface effect still owns layout and focus.
     */
    private fun startOpenedSessionBeforeObservation(project: Project, file: VirtualFile) {
        GenerationWork.executeOnPooledThread {
            if (project.isDisposed || !file.isValid) return@executeOnPooledThread
            val (cwd, relativePath) =
                try {
                    TerminalUtil.resolveProject(project, file)
                } catch (failure: Throwable) {
                    LOG.warn(
                        "[layout-sync] fileOpened start resolution failed for ${file.path}",
                        failure,
                    )
                    return@executeOnPooledThread
                }
            IdeTerminalCoordinator.startInPane(
                project = project,
                cwd = cwd,
                relativePath = relativePath,
                onReady = {
                    if (
                        project.isDisposed ||
                            !file.isValid ||
                            !FileEditorManager.getInstance(project).isFileOpen(file)
                    ) {
                        return@startInPane
                    }
                    requestObservation(
                        PendingSurfaceObservation(
                            project = project,
                            preferredFile = file,
                            forceReconcile = false,
                            authority = ObservationAuthority.FileOpened,
                        ),
                    )
                    log("fileOpened: file=${file.name}; start ready, surface queued")
                },
                onFailure = { message ->
                    LOG.warn(
                        "[layout-sync] fileOpened start failed for ${file.path}: $message",
                    )
                },
            )
        }
    }

    /**
     * Evict the per-document state-projection mirror + generation counters when an editor tab
     * closes (`#jbmirrorevict` / `#nsq2`). Without this the `StateProjectionBridge` maps grow
     * monotonically and a reused path (move/symlink/reopen) surfaces the prior document's stale
     * projection state. Re-subscription lazily re-creates the mirror from a fresh cold snapshot.
     */
    override fun fileClosed(source: FileEditorManager, file: VirtualFile) {
        if (!file.name.endsWith(".md")) return
        StateProjectionBridge.evictForFile(file.path)
        requestObservation(
            PendingSurfaceObservation(
                project = source.project,
                preferredFile = null,
                forceReconcile = false,
                authority = ObservationAuthority.Layout,
            ),
        )
    }
}
