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
    )

    /** What the derivation answered. [decision] carries [columns] when it publishes. */
    internal data class Result(
        val projectRoot: String,
        val editorLayout: EditorLayout?,
        val columns: List<String>,
        val columnOrder: String,
        val decision: ExactVisibleSyncDecision,
    )

    internal fun evidence(
        basePath: String?,
        fallbackRoot: String?,
        focused: String?,
        visible: List<String>,
        editorLayout: EditorLayout?,
        foreground: Boolean = true,
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
                derivation.derive("sync_layout", GSON.toJson(evidence))?.let(::parseNative)
            }
        return native ?: frozen(evidence, nearestRoot)
    }

    /** Parse the native `sync_layout` output; `null` takes the frozen floor. */
    internal fun parseNative(json: String): Result? =
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
                Result(
                    projectRoot = root.get("project_root").asString,
                    editorLayout = root.get("layout")?.takeIf { it.isJsonObject }?.asJsonObject?.let(::parseLayout),
                    columns = columns,
                    columnOrder = root.get("column_order")?.takeIf { !it.isJsonNull }?.asString ?: "unknown",
                    decision = it,
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
        val projectRoot =
            SyncLayoutAction.chooseSyncProjectRoot(evidence.basePath, fallbackRoot, evidence.visible, nearestRoot)
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
        )
    }
}
