package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import javax.swing.SwingUtilities

/**
 * GH #326 phase 2: the automatic editor surface, derived by the hot-reloaded native library.
 *
 * [EditorTabSyncListener] captures what IntelliJ state *is* on the EDT ([SurfaceProjectionEvidence])
 * and the surface delivery worker asks the native `surface_projection` derivation what it *means*:
 * the visible-document union (including the GH #320 Remote Dev union), readiness, active-document
 * resolution, open-tab proximity, stale-selection settling, the layout (local `window_layout` or the
 * Remote Dev fold), and the controller payload including `column_order`. A rule change there is a
 * native change with no plugin generation. [frozen] answers only when no native generation does;
 * it is a compatibility floor and is never extended.
 */
internal object SurfaceProjection {
    private val LOG = Logger.getInstance(SurfaceProjection::class.java)
    private val GSON = Gson()

    /** Remote Dev client state captured on the EDT. Wire shape of Rust `RemoteSurfaceEvidence`. */
    internal data class RemoteSurfaceEvidence(
        @SerializedName("project_root") val projectRoot: String,
        val clients: List<LayoutDetector.RemoteClientSessionEditors>,
        val focused: List<String>,
    )

    /**
     * One editor-surface observation as IntelliJ reported it. Wire shape of Rust
     * `SurfaceProjectionEvidence`; absent (null) fields are omitted, never defaulted.
     */
    internal data class SurfaceProjectionEvidence(
        @SerializedName("base_path") val basePath: String?,
        @SerializedName("backend_window_count") val backendWindowCount: Int,
        val windows: List<LayoutDetector.EditorWindowEvidence>,
        @SerializedName("window_geometry_unavailable") val windowGeometryUnavailable: Boolean,
        val history: List<String>,
        @SerializedName("manager_selected") val managerSelected: List<String>,
        @SerializedName("open_session_documents") val openSessionDocuments: List<String>,
        val remote: RemoteSurfaceEvidence?,
        @SerializedName("measured_layout_authority") val measuredLayoutAuthority: Boolean,
        @SerializedName("anchor_selected_session") val anchorSelectedSession: String?,
        @SerializedName("selected_text_editor_session") val selectedTextEditorSession: String?,
        val preferred: String?,
        @SerializedName("preferred_session_document") val preferredSessionDocument: Boolean,
        val previous: String?,
        val authority: String?,
        @SerializedName("remaining_selection_passes") val remainingSelectionPasses: Int?,
        @SerializedName("force_reconcile") val forceReconcile: Boolean,
        @SerializedName("preserve_focus") val preserveFocus: Boolean,
    )

    internal sealed interface Result {
        /**
         * [surface] is the controller payload as derived. It stays a JSON object so a field a newer
         * native library adds reaches the controller without a plugin generation.
         */
        data class Ready(val surface: JsonObject) : Result

        /** The editor projection has not settled; [reason] is the `capture deferred` reason. */
        data class Deferred(val reason: String) : Result
    }

    /** The derivation's name for an [EditorTabSyncListener.ObservationAuthority]. */
    internal fun wireAuthority(authority: EditorTabSyncListener.ObservationAuthority): String =
        when (authority) {
            EditorTabSyncListener.ObservationAuthority.Layout -> "layout"
            EditorTabSyncListener.ObservationAuthority.DocumentSelection -> "document_selection"
            EditorTabSyncListener.ObservationAuthority.EditorFocus -> "editor_focus"
            EditorTabSyncListener.ObservationAuthority.FileOpened -> "file_opened"
            EditorTabSyncListener.ObservationAuthority.IdeActivation -> "ide_activation"
        }

    /**
     * Derive [evidence]. Never on the EDT: the surface delivery worker and command background
     * threads call this. An EDT caller is refused the native call and gets the frozen floor.
     */
    internal fun derive(
        evidence: SurfaceProjectionEvidence,
        derivation: EditorDerivation = EditorDerivation.Native,
        remoteLayout: (LayoutDetector.RemoteLayoutSnapshot) -> EditorLayout? = { snapshot ->
            LayoutDetector.detectEditorLayout(snapshot)
        },
    ): Result {
        val native =
            if (SwingUtilities.isEventDispatchThread()) {
                LOG.warn("[layout-sync] surface_projection asked on the EDT; using the compatibility floor")
                null
            } else {
                derivation.derive("surface_projection", GSON.toJson(evidence))?.let(::parseNative)
            }
        if (native != null) {
            native.observedLine?.let(LayoutDetector::logObservedLayout)
            native.info.forEach(LOG::info)
            native.debug.forEach { LOG.debug(it) }
            return native.result
        }
        val windowDerivation =
            if (SwingUtilities.isEventDispatchThread()) EditorDerivation.Unavailable else derivation
        return frozen(
            evidence,
            windowLayout = { windows -> LayoutDetector.detectEditorLayout(windows, windowDerivation) },
            remoteLayout = remoteLayout,
        )
    }

    internal data class NativeAnswer(
        val result: Result,
        val observedLine: String?,
        val info: List<String>,
        val debug: List<String>,
    )

    /** Parse the native `surface_projection` output; `null` takes the frozen floor. */
    internal fun parseNative(json: String): NativeAnswer? =
        try {
            val root = JsonParser.parseString(json).asJsonObject
            fun string(name: String): String? = root.get(name)?.takeIf { !it.isJsonNull }?.asString
            fun lines(name: String): List<String> =
                root.getAsJsonArray(name)?.map { it.asString }.orEmpty()
            val result =
                when (string("outcome")) {
                    "ready" ->
                        root.get("surface")
                            ?.takeIf(com.google.gson.JsonElement::isJsonObject)
                            ?.asJsonObject
                            ?.let(Result::Ready)
                    "deferred" -> Result.Deferred(string("reason") ?: "unspecified")
                    else -> null
                }
            result?.let {
                NativeAnswer(
                    result = it,
                    observedLine = string("observed_line"),
                    info = lines("info"),
                    debug = lines("debug"),
                )
            }
        } catch (e: Exception) {
            LOG.warn("[layout-sync] native surface projection unreadable: ${e.message}")
            null
        }

    /**
     * GH #326 compatibility floor: the pre-#326 `captureSurface` / `resolveSurface` rules over the
     * same evidence, frozen. Used only when no native generation answers `surface_projection`.
     * Never extend it; change the native derivation and its parity fixture instead.
     */
    internal fun frozen(
        evidence: SurfaceProjectionEvidence,
        windowLayout: (LayoutDetector.WindowLayoutEvidence) -> EditorLayout?,
        remoteLayout: (LayoutDetector.RemoteLayoutSnapshot) -> EditorLayout?,
    ): Result {
        val report = EditorTabSyncListener.SurfaceReport
        val clients = evidence.remote?.clients.orEmpty()
        val windows = evidence.windows
        val useRemoteClientLayout =
            LayoutDetector.shouldUseRemoteClientLayout(
                backendWindowCount = evidence.backendWindowCount,
                remoteClientSessionCount = clients.size,
            )
        if (useRemoteClientLayout && evidence.measuredLayoutAuthority) {
            return Result.Deferred("frontend_measured_layout_authority")
        }
        val openSessionFiles =
            if (useRemoteClientLayout) {
                evidence.openSessionDocuments
            } else {
                windows.flatMap(LayoutDetector.EditorWindowEvidence::sessionTabs).distinct()
            }
        val sessionDocumentPaths = openSessionFiles.toSet()
        val selectedWindowPaths =
            report.splitSelections(
                if (useRemoteClientLayout) emptyList() else windows.map { it.selected },
                evidence.managerSelected,
            )
        if (
            report.windowSelectionReadiness(
                selectedWindowFiles = selectedWindowPaths,
                useRemoteClientLayout = useRemoteClientLayout,
            ) == EditorTabSyncListener.SurfaceReport.WindowSelectionReadiness.AwaitingBackendWindowSelection
        ) {
            return Result.Deferred(
                "backend_window_selection_incomplete " +
                    "windows=${windows.size} selected=${selectedWindowPaths.size}",
            )
        }
        val windowVisibleMdFiles =
            report.visibleMarkdownFilesFromRestoredWindows(
                selectedWindowPaths,
                windows.map { window ->
                    LayoutDetector.stickyMarkdownForWindow(
                        selectedPath = window.selected,
                        windowMarkdownTabsMruLast =
                            LayoutDetector.markdownTabsMruLast(
                                window.sessionTabs.filter(sessionDocumentPaths::contains),
                                evidence.history,
                            ),
                    )
                },
                sessionDocumentPaths,
            )
        val rawVisibleMdFiles =
            if (useRemoteClientLayout) {
                report.remoteClientVisibleSessionFiles(
                    backendVisible = windowVisibleMdFiles,
                    clientVisible = clients.map { it.visible },
                    clientSelected = clients.map { it.selected },
                    absoluteByRelative =
                        evidence.openSessionDocuments.associateBy { path ->
                            LayoutDetector.projectRelativePath(evidence.basePath, path)
                        },
                )
            } else {
                windowVisibleMdFiles
            }
        val reconcileStaleSelection =
            evidence.authority == "document_selection" && evidence.remainingSelectionPasses == 0
        val visibleMdFiles =
            if (reconcileStaleSelection) {
                EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                    preferredFile = evidence.preferred,
                    previousFile = evidence.previous,
                    visibleMdFiles = rawVisibleMdFiles,
                    editorLayout = null,
                ).visibleMdFiles
            } else {
                rawVisibleMdFiles
            }
        if (visibleMdFiles.isEmpty()) {
            return Result.Deferred("no_visible_session_documents")
        }
        val preferred =
            evidence.preferred?.takeIf { evidence.preferredSessionDocument && it in sessionDocumentPaths }
        if (evidence.preferred != null && preferred == null) {
            return Result.Deferred("preferred_file_not_open_session_document")
        }
        if (
            report.projectionReadiness(
                preferredActiveFile = preferred,
                visibleMdFiles = visibleMdFiles,
            ) != EditorTabSyncListener.SurfaceReport.ProjectionReadiness.Current
        ) {
            return Result.Deferred("preferred_document_not_in_current_projection")
        }
        val selectedEditorFile =
            if (useRemoteClientLayout) evidence.selectedTextEditorSession else evidence.anchorSelectedSession
        val activeFilePath =
            report.resolveActiveFilePath(
                preferredActiveFile = preferred,
                selectedEditorFile = selectedEditorFile,
                visibleMdFiles = visibleMdFiles,
            ) ?: return Result.Deferred("active_session_document_unresolved")
        if (
            listOfNotNull(preferred, selectedEditorFile, openSessionFiles.firstOrNull { it == activeFilePath })
                .none { it == activeFilePath }
        ) {
            return Result.Deferred("active_virtual_file_unresolved")
        }
        val focusedWindowTabs =
            windows.firstOrNull { it.selected == activeFilePath }?.sessionTabs.orEmpty()
        val openMdFiles =
            report.prioritizeOpenDocuments(
                focusedFile = activeFilePath,
                nearbyTabs = report.tabsByProximity(focusedFile = activeFilePath, tabs = focusedWindowTabs),
                visibleMdFiles = visibleMdFiles,
                openMdFiles = openSessionFiles,
            )
        val usesRemoteLayout = useRemoteClientLayout || windows.isEmpty()
        val local = !usesRemoteLayout && !evidence.windowGeometryUnavailable
        val resolvedEditorLayout =
            when {
                usesRemoteLayout ->
                    remoteLayout(
                        LayoutDetector.RemoteLayoutSnapshot(
                            projectRoot = evidence.remote?.projectRoot.orEmpty(),
                            clients = clients,
                            focusedSessionFiles = evidence.remote?.focused.orEmpty(),
                        ),
                    )
                local ->
                    windowLayout(
                        LayoutDetector.WindowLayoutEvidence(
                            basePath = evidence.basePath,
                            windows = windows,
                            history = evidence.history,
                        ),
                    )?.let { relative ->
                        evidence.basePath
                            ?.let { basePath -> SyncLayoutAction.absolutizeEditorLayout(basePath, relative) }
                            ?: relative
                    }
                else -> null
            }
        val reconciledEditorLayout =
            if (reconcileStaleSelection) {
                EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                    preferredFile = evidence.preferred,
                    previousFile = evidence.previous,
                    visibleMdFiles = visibleMdFiles,
                    editorLayout = resolvedEditorLayout,
                ).editorLayout
            } else {
                resolvedEditorLayout
            }
        val settledEditorLayout =
            if (local) {
                report.layoutForPreferredDocument(
                    preferredActiveFile = evidence.preferred,
                    editorLayout = reconciledEditorLayout,
                ) ?: run {
                    if (reconciledEditorLayout != null) {
                        LOG.info(
                            "[layout-sync] layout withheld reason=preferred_document_not_in_layout " +
                                "preferred=${evidence.preferred}",
                        )
                    }
                    null
                }
            } else {
                reconciledEditorLayout
            }
        return Result.Ready(
            GSON.toJsonTree(
                report.buildSurface(
                    focusedFile = activeFilePath,
                    visibleMdFiles = visibleMdFiles,
                    openMdFiles = openMdFiles,
                    editorLayout = settledEditorLayout,
                    forceReconcile = evidence.forceReconcile,
                    preserveFocus = evidence.preserveFocus,
                ),
            ).asJsonObject,
        )
    }
}
