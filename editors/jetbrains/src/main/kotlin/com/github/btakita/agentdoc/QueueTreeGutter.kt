package com.github.btakita.agentdoc

import com.google.gson.JsonParser

/**
 * `#queuetreegutter`: editor presentation of the native queue tree (`agent_doc_queue_tree_json`).
 *
 * The native projection owns every queue node's state, glyph and tooltip (FFI-first); this file
 * only converts its 1-based lines into document offsets for one text snapshot:
 * - [QueueTreeGutterNode.rowStart]..[QueueTreeGutterNode.rowEnd] is the row's own line, where the
 *   gutter icon goes;
 * - a closed node with a subtree folds `line + 1 ..= end_line`, i.e. from the end of its row
 *   line to the end of its last subtree line, so the row itself stays visible.
 */
internal data class QueueTreeGutterNode(
    val line: Int,
    val rowStart: Int,
    val rowEnd: Int,
    val glyph: String,
    val tooltip: String,
    val closed: Boolean,
    /** Fold range for a closed node with a subtree, else null. */
    val foldStart: Int?,
    val foldEnd: Int?,
)

/** One fold of a closed subtree, in document offsets. */
internal data class QueueTreeFold(val start: Int, val end: Int, val placeholder: String)

/** An existing agent-doc queue fold region: its current range and whether it is expanded. */
internal data class ExistingQueueTreeFold<H>(val region: H, val start: Int, val end: Int, val expanded: Boolean)

internal data class QueueTreeFoldPlan<H>(
    val remove: List<H>,
    /** New folds paired with whether each starts expanded. */
    val add: List<Pair<QueueTreeFold, Boolean>>,
)

internal object QueueTreeGutter {
    /** Highlighter kind of a queue tree gutter icon in [VisualHighlighterManager]. */
    const val KIND = "queue_tree_node"

    /** The tooltip prefix that names the source of the icon. */
    private const val TOOLTIP_PREFIX = "agent-doc queue: "

    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(QueueTreeGutter::class.java)

    private data class NativeNode(
        val line: Int,
        val endLine: Int,
        val glyph: String,
        val tooltip: String,
        val closed: Boolean,
    )

    /**
     * Queue tree nodes of [text], or null when the library or its symbol is unavailable or the
     * document does not parse mid-edit, so the caller keeps the last presentation. A document with
     * no queue component yields an empty list. Background thread only (native call).
     */
    fun nodesOrNull(text: String): List<QueueTreeGutterNode>? {
        if (!text.contains("<!-- agent:queue")) return emptyList()
        val lib = AgentDocLib.get() ?: return null
        val ptr =
            try {
                lib.agent_doc_queue_tree_json(text)
            } catch (error: UnsatisfiedLinkError) {
                LOG.debug("[native] queue_tree_json unavailable: ${error.message}")
                return null
            } catch (error: Throwable) {
                LOG.debug("[native] queue_tree_json failed: ${error.message}")
                return null
            } ?: return null
        return try {
            project(text, parse(ptr.readNativeUtf8()) ?: return null)
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /** Parse the native JSON, or null when it is not a queue tree. */
    fun parseJson(raw: String, text: String): List<QueueTreeGutterNode>? = parse(raw)?.let { project(text, it) }

    private fun parse(raw: String): List<NativeNode>? =
        try {
            val nodes = JsonParser.parseString(raw).asJsonObject.getAsJsonArray("nodes") ?: return null
            nodes.mapNotNull { element ->
                val obj = element.asJsonObject
                val line = obj.get("line")?.asInt ?: return@mapNotNull null
                val state = obj.get("state")?.asString ?: return@mapNotNull null
                NativeNode(
                    line = line,
                    endLine = obj.get("end_line")?.asInt ?: line,
                    glyph = obj.get("glyph")?.asString ?: return@mapNotNull null,
                    // An older library without `tooltip`: its state label is the same words.
                    tooltip = obj.get("tooltip")?.asString ?: state.replace('-', ' '),
                    closed = obj.get("closed")?.asBoolean ?: false,
                )
            }
        } catch (error: Exception) {
            LOG.warn("[native] queue_tree_json error: ${error.message}")
            null
        }

    private fun project(text: String, nodes: List<NativeNode>): List<QueueTreeGutterNode> {
        val lineStarts = ArrayList<Int>()
        lineStarts.add(0)
        for (index in text.indices) {
            if (text[index] == '\n') lineStarts.add(index + 1)
        }
        fun lineEnd(line: Int): Int? {
            if (line < 1 || line > lineStarts.size) return null
            var end = if (line < lineStarts.size) lineStarts[line] - 1 else text.length
            if (end > lineStarts[line - 1] && text[end - 1] == '\r') end -= 1
            return end
        }
        return nodes.mapNotNull { node ->
            val rowEnd = lineEnd(node.line) ?: return@mapNotNull null
            val rowStart = lineStarts[node.line - 1]
            val foldEnd = if (node.closed && node.endLine > node.line) lineEnd(node.endLine) else null
            val folds = foldEnd != null && foldEnd > rowEnd
            QueueTreeGutterNode(
                line = node.line,
                rowStart = rowStart,
                rowEnd = rowEnd,
                glyph = node.glyph,
                tooltip = TOOLTIP_PREFIX + node.tooltip,
                closed = node.closed,
                foldStart = if (folds) rowEnd else null,
                foldEnd = if (folds) foldEnd else null,
            )
        }
    }

    /** The closed-subtree folds of [nodes]. */
    fun folds(nodes: List<QueueTreeGutterNode>, text: CharSequence): List<QueueTreeFold> =
        nodes.mapNotNull { node ->
            val start = node.foldStart ?: return@mapNotNull null
            val end = node.foldEnd ?: return@mapNotNull null
            if (end > text.length) return@mapNotNull null
            val hidden = text.subSequence(start, end).count { it == '\n' }
            QueueTreeFold(start, end, " … $hidden ${if (hidden == 1) "line" else "lines"}")
        }
}

/**
 * Reconcile the editor's queue folds with the desired closed subtrees. A fold whose range still
 * matches is kept with whatever expansion the operator gave it. A new fold starts collapsed, unless
 * it replaces a fold at the same start the operator had expanded (its subtree grew or shrank while
 * open) or a caret is inside it (never hide the line being edited).
 */
internal fun <H> planQueueTreeFolds(
    existing: List<ExistingQueueTreeFold<H>>,
    desired: List<QueueTreeFold>,
    caretOffsets: List<Int>,
): QueueTreeFoldPlan<H> {
    val unmatched = existing.toMutableList()
    val add = ArrayList<Pair<QueueTreeFold, Boolean>>()
    val pending = ArrayList<QueueTreeFold>()
    for (fold in desired) {
        val match = unmatched.indexOfFirst { it.start == fold.start && it.end == fold.end }
        if (match >= 0) unmatched.removeAt(match) else pending.add(fold)
    }
    for (fold in pending) {
        val replaced = unmatched.any { it.start == fold.start && it.expanded }
        val caretInside = caretOffsets.any { it > fold.start && it <= fold.end }
        add.add(fold to (replaced || caretInside))
    }
    return QueueTreeFoldPlan(unmatched.map { it.region }, add)
}
