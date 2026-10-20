package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.google.gson.annotations.SerializedName
import com.intellij.openapi.diagnostic.Logger
import javax.swing.SwingUtilities

/**
 * GH #326 phase 3: the Sync Tmux Layout `--col` snapshot decisions, derived by the hot-reloaded
 * native library.
 *
 * [SyncLayoutAction] reads what the editor *is* on the EDT (the focused and visible session
 * documents and the split layout evidence) and its background thread asks the native `sync_layout`
 * derivation what that *means*: the filesystem root that spans the visible layout, the layout's
 * paths normalized against that root and made absolute, the `--col` columns with their order label,
 * and whether the exact-visible snapshot may be published (GH #154). The automatic surface asks the
 * same derivation for its root choice and path normalization. [frozen] answers only when no native
 * generation does; it composes the pre-#326 [SyncLayoutAction] rules and is never extended.
 *
 * Phase 3b moved the last two callers of those rules here. Run Agent Doc's `--col` fallback
 * (`TerminalUtil.editorSnapshotRouteLayout`) passes the route's own root as [Evidence.chosenRoot]
 * and reads [Result.columns], [Result.columnOrder], and [Result.layoutMode]. The
 * `TmuxPaneFocusSync` poll reads [Result.candidateRoots].
 */
internal object SyncLayoutSnapshot {
    private val LOG = Logger.getInstance(SyncLayoutSnapshot::class.java)
    private val GSON = Gson()

    /** One layout column. Wire shape of Rust `SyncLayoutColumn`. */
    internal data class ColumnEvidence(val files: List<String>)

    /** A detected split layout. Wire shape of Rust `SyncEditorLayout`. */
    internal data class LayoutEvidence(
        val columns: List<ColumnEvidence>,
        @SerializedName("column_order") val columnOrder: String,
    )

    /**
     * One sync observation. Wire shape of Rust `SyncLayoutEvidence`; absent (null) fields are
     * omitted, never defaulted.
     */
    internal data class Evidence(
        @SerializedName("base_path") val basePath: String?,
        @SerializedName("fallback_root") val fallbackRoot: String?,
        val focused: String?,
        val visible: List<String>,
        val layout: LayoutEvidence?,
        val foreground: Boolean,
        /**
         * GH #326 phase 3b: the root the caller already chose (Run Agent Doc's route root). A
         * library older than phase 3b ignores it, so [derive] takes the floor when the answer
         * addresses another root.
         */
        @SerializedName("chosen_root") val chosenRoot: String? = null,
    )

    /** What the derivation answered. [decision] carries [columns] when it publishes. */
    internal data class Result(
        val projectRoot: String,
        val editorLayout: EditorLayout?,
        val columns: List<String>,
        val columnOrder: String,
        val decision: ExactVisibleSyncDecision,
        /** What the columns assert to the controller (GH #111): `exact` or `ensure`. */
        val layoutMode: String,
        /** The controller roots that may own the tmux focus, most specific first. */
        val candidateRoots: List<String>,
    )

    internal fun evidence(
        basePath: String?,
        fallbackRoot: String?,
        focused: String?,
        visible: List<String>,
        editorLayout: EditorLayout?,
        foreground: Boolean = true,
        chosenRoot: String? = null,
    ): Evidence =
        Evidence(
            basePath = basePath,
            fallbackRoot = fallbackRoot,
            focused = focused,
            visible = visible,
            layout = editorLayout?.let { layout ->
                LayoutEvidence(layout.columns.map { ColumnEvidence(it.files) }, layout.columnOrder)
            },
            foreground = foreground,
            chosenRoot = chosenRoot,
        )

    /**
     * Derive [evidence]. Never on the EDT: the sync's background thread and the surface delivery
     * worker call this. An EDT caller is refused the native call and gets the frozen floor.
     */
    internal fun derive(
        evidence: Evidence,
        derivation: EditorDerivation = EditorDerivation.Native,
        nearestRoot: (String) -> String? = TerminalUtil::nearestAgentDocProjectRoot,
    ): Result {
        val native =
            if (SwingUtilities.isEventDispatchThread()) {
                LOG.warn("[sync] sync_layout asked on the EDT; using the compatibility floor")
                null
            } else {
                derivation.derive("sync_layout", GSON.toJson(evidence))
                    ?.let { parseNative(it, evidence, nearestRoot) }
                    ?.takeIf { answer -> honorsChosenRoot(evidence, answer) }
            }
        return native ?: frozen(evidence, nearestRoot)
    }

    /**
     * A library older than phase 3b ignores `chosen_root` and answers the root it chose itself;
     * that answer is normalized against the wrong root, so the floor answers instead.
     */
    private fun honorsChosenRoot(evidence: Evidence, answer: Result): Boolean {
        val chosen = evidence.chosenRoot?.takeIf { it.isNotBlank() } ?: return true
        if (answer.projectRoot == chosen) return true
        LOG.warn(
            "[sync] native sync_layout ignored chosen_root=$chosen (answered ${answer.projectRoot}); " +
                "using the compatibility floor",
        )
        return false
    }

    /**
     * Parse the native `sync_layout` output; `null` takes the frozen floor. An answer from a library
     * older than phase 3b has no `layout_mode` or `candidate_roots`; the floor's rules fill them from
     * [evidence].
     */
    internal fun parseNative(
        json: String,
        evidence: Evidence? = null,
        nearestRoot: (String) -> String? = TerminalUtil::nearestAgentDocProjectRoot,
    ): Result? =
        try {
            val root = JsonParser.parseString(json).asJsonObject
            val columns = root.getAsJsonArray("columns").map { it.asString }
            val decision =
                when (root.get("decision")?.asString) {
                    "publish" -> ExactVisibleSyncDecision.Publish(columns)
                    "refuse_unknown_layout" -> ExactVisibleSyncDecision.RefuseUnknownLayout
                    "refuse_stale_focus_projection" -> ExactVisibleSyncDecision.RefuseStaleFocusProjection
                    else -> null
                }
            decision?.let {
                val projectRoot = root.get("project_root").asString
                val editorLayout =
                    root.get("layout")?.takeIf { it.isJsonObject }?.asJsonObject?.let(::parseLayout)
                Result(
                    projectRoot = projectRoot,
                    editorLayout = editorLayout,
                    columns = columns,
                    columnOrder = root.get("column_order")?.takeIf { !it.isJsonNull }?.asString ?: "unknown",
                    decision = it,
                    layoutMode =
                        root.get("layout_mode")?.takeIf { !it.isJsonNull }?.asString
                            ?: TerminalUtil.routeLayoutMode(editorLayout),
                    candidateRoots =
                        root.get("candidate_roots")?.takeIf { it.isJsonArray }?.asJsonArray?.map { it.asString }
                            ?: evidence?.let { frozenCandidateRoots(it, projectRoot, nearestRoot) }
                            ?: emptyList(),
                )
            } ?: run {
                LOG.warn("[sync] native sync_layout decision unreadable: ${root.get("decision")}")
                null
            }
        } catch (e: Exception) {
            LOG.warn("[sync] native sync_layout unreadable: ${e.message}")
            null
        }

    /** Empty columns inside a layout are kept: a split without a session document keeps its pane. */
    private fun parseLayout(layout: JsonObject): EditorLayout =
        EditorLayout(
            layout.getAsJsonArray("columns")
                ?.map { column ->
                    LayoutColumn(column.asJsonObject.getAsJsonArray("files")?.map { it.asString }.orEmpty())
                }
                .orEmpty(),
            layout.get("column_order")?.takeIf { !it.isJsonNull }?.asString ?: "editor",
        )

    /**
     * GH #326 compatibility floor: the pre-#326 [SyncLayoutAction] rules, frozen. Used only when no
     * native generation answers. Never extend it; change the native derivation instead.
     */
    internal fun frozen(
        evidence: Evidence,
        nearestRoot: (String) -> String? = TerminalUtil::nearestAgentDocProjectRoot,
    ): Result {
        val fallbackRoot = evidence.fallbackRoot?.takeIf { it.isNotBlank() } ?: evidence.basePath ?: ""
        // Phase 3b: a chosen root is the pre-#326 Run Agent Doc fallback, which normalized against
        // the route's own root.
        val projectRoot =
            evidence.chosenRoot?.takeIf { it.isNotBlank() }
                ?: SyncLayoutAction.chooseSyncProjectRoot(evidence.basePath, fallbackRoot, evidence.visible, nearestRoot)
        val editorLayout =
            SyncLayoutAction.absolutizeEditorLayout(
                projectRoot,
                SyncLayoutAction.normalizeEditorLayout(
                    evidence.basePath,
                    projectRoot,
                    evidence.layout?.let { layout ->
                        EditorLayout(layout.columns.map { LayoutColumn(it.files) }, layout.columnOrder)
                    },
                ),
            )
        return Result(
            projectRoot = projectRoot,
            editorLayout = editorLayout,
            columns = SyncLayoutAction.buildSyncColumns(evidence.visible, editorLayout),
            columnOrder = SyncLayoutAction.syncColumnOrder(editorLayout),
            decision =
                SyncLayoutAction.exactVisibleSyncDecision(
                    evidence.visible,
                    editorLayout,
                    evidence.focused?.takeIf { it.isNotEmpty() },
                    evidence.foreground,
                ),
            layoutMode = TerminalUtil.routeLayoutMode(editorLayout),
            candidateRoots = frozenCandidateRoots(evidence, projectRoot, nearestRoot),
        )
    }

    /**
     * The pre-#326 `TmuxPaneFocusSync.candidateProjectRoots` rule, frozen: the focused document's
     * own root, the root spanning the visible documents (only with a focused root), each visible
     * document's own root (`TerminalUtil.resolveProjectPath`: its nearest `.agent-doc` root, else
     * the base path, else its directory), then the base path. Never extend it.
     */
    private fun frozenCandidateRoots(
        evidence: Evidence,
        projectRoot: String,
        nearestRoot: (String) -> String?,
    ): List<String> {
        val roots = linkedSetOf<String>()
        val focusedRoot = evidence.fallbackRoot?.takeIf { it.isNotBlank() }
        focusedRoot?.let { roots.add(it) }
        if (evidence.visible.isNotEmpty() && focusedRoot != null) roots.add(projectRoot)
        for (file in evidence.visible) {
            val normalized = java.io.File(file).toPath().toAbsolutePath().normalize()
            roots.add(
                nearestRoot(file)
                    ?: evidence.basePath?.let { java.io.File(it).toPath().toAbsolutePath().normalize().toString() }
                    ?: normalized.parent?.toString()
                    ?: "/",
            )
        }
        evidence.basePath?.let { roots.add(it) }
        return roots.filter { it.isNotBlank() }
    }
}
