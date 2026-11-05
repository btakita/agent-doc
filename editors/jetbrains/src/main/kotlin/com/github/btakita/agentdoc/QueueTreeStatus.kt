package com.github.btakita.agentdoc

import com.google.gson.JsonParser

/**
 * `#treestatusjetbrains`: editor presentation of the native tree-status answer
 * (`agent_doc_queue_tree_status_json`, `#treestatusquery`).
 *
 * The native answer owns every state, glyph and word: the popup shows its `text` verbatim and
 * offers one delete quick-fix per `deletable` row, deleting lines `line..=end_line`. This file
 * only parses that answer and maps 1-based lines to document offsets; it never derives a state.
 */
internal data class TreeStatusDeletableRow(
    val line: Int,
    val endLine: Int,
    val glyph: String,
    val text: String,
    val id: String?,
)

internal data class QueueTreeStatusAnswer(
    /** The CLI rendering of `agent-doc queue tree-status FILE --line N`, shown verbatim. */
    val text: String,
    val deletable: List<TreeStatusDeletableRow>,
)

/** One row of the Tree Status popup. */
internal sealed class TreeStatusPopupItem {
    abstract val label: String

    /** One line of the native rendering; shown, never selectable. */
    data class StatusLine(override val label: String) : TreeStatusPopupItem()

    /** Delete quick-fix for one deletable subtree. */
    data class DeleteRows(val row: TreeStatusDeletableRow) : TreeStatusPopupItem() {
        override val label: String =
            "Delete ${lineRange(row.line, row.endLine)}: ${listOf(row.glyph, row.text).filter { it.isNotBlank() }.joinToString(" ")}"
    }

    /** Copy the native rendering to the clipboard. */
    data object CopyText : TreeStatusPopupItem() {
        override val label: String = "Copy tree status"
    }

    val selectable: Boolean get() = this !is StatusLine

    companion object {
        /** Same range form as the CLI rendering (`L5` or `L5–L9`). */
        fun lineRange(line: Int, endLine: Int): String = if (endLine > line) "L$line–L$endLine" else "L$line"
    }
}

/** The outcome of one native tree-status call. */
internal sealed class TreeStatusQueryResult {
    data class Answer(val answer: QueueTreeStatusAnswer) : TreeStatusQueryResult()

    /** The caret line is outside every queue, or the document does not parse mid-edit. */
    data object NoQueue : TreeStatusQueryResult()

    /** No native library, or one too old to export the symbol. */
    data class Unavailable(val reason: String) : TreeStatusQueryResult()
}

internal object QueueTreeStatus {
    const val POPUP_TITLE = "Agent Doc: Tree Status"
    const val NO_QUEUE_MESSAGE = "Tree Status: the caret is not inside an agent:queue component (or the document does not parse)."
    const val STALE_MESSAGE =
        "Tree Status: those lines changed since the status was taken; nothing was deleted. Run Tree Status again."

    private val LOG = com.intellij.openapi.diagnostic.Logger.getInstance(QueueTreeStatus::class.java)

    /**
     * Ask the native library about the queue subtree at 1-based [line] of [text]. Background
     * thread only (native call).
     */
    fun query(text: String, line: Int): TreeStatusQueryResult {
        if (!text.contains("<!-- agent:queue")) return TreeStatusQueryResult.NoQueue
        val lib = AgentDocLib.get() ?: return TreeStatusQueryResult.Unavailable("the agent-doc native library is not loaded")
        val ptr =
            try {
                lib.agent_doc_queue_tree_status_json(text, line)
            } catch (error: UnsatisfiedLinkError) {
                LOG.debug("[native] queue_tree_status_json unavailable: ${error.message}")
                return TreeStatusQueryResult.Unavailable(
                    "the loaded agent-doc native library predates the tree-status query; update agent-doc",
                )
            } catch (error: Throwable) {
                LOG.warn("[native] queue_tree_status_json failed: ${error.message}")
                return TreeStatusQueryResult.Unavailable("the native tree-status call failed: ${error.message}")
            } ?: return TreeStatusQueryResult.NoQueue
        return try {
            parseJson(ptr.readNativeUtf8())?.let { TreeStatusQueryResult.Answer(it) }
                ?: TreeStatusQueryResult.Unavailable("the native tree-status answer did not parse")
        } finally {
            lib.agent_doc_free_string(ptr)
        }
    }

    /** Parse the native JSON, or null when it is not a tree-status answer. */
    fun parseJson(raw: String): QueueTreeStatusAnswer? =
        try {
            val obj = JsonParser.parseString(raw).asJsonObject
            val text = obj.get("text")?.takeIf { it.isJsonPrimitive }?.asString ?: return null
            val deletable = obj.getAsJsonArray("deletable")?.mapNotNull { element ->
                val row = element.takeIf { it.isJsonObject }?.asJsonObject ?: return@mapNotNull null
                val line = row.get("line")?.asInt ?: return@mapNotNull null
                TreeStatusDeletableRow(
                    line = line,
                    endLine = row.get("end_line")?.asInt ?: line,
                    glyph = row.get("glyph")?.asString.orEmpty(),
                    text = row.get("text")?.asString.orEmpty(),
                    id = row.get("id")?.takeIf { it.isJsonPrimitive }?.asString,
                )
            }.orEmpty()
            QueueTreeStatusAnswer(text, deletable)
        } catch (error: Exception) {
            LOG.warn("[native] queue_tree_status_json error: ${error.message}")
            null
        }

    /** The popup rows: the native text line by line, then one delete per deletable row, then copy. */
    fun popupItems(answer: QueueTreeStatusAnswer): List<TreeStatusPopupItem> = buildList {
        answer.text.trimEnd('\n').split('\n').forEach { add(TreeStatusPopupItem.StatusLine(it)) }
        answer.deletable.forEach { add(TreeStatusPopupItem.DeleteRows(it)) }
        add(TreeStatusPopupItem.CopyText)
    }

    /** The first quick-fix row, which a separator sets apart from the status text. */
    fun firstQuickFix(items: List<TreeStatusPopupItem>): TreeStatusPopupItem? = items.firstOrNull { it.selectable }

    /**
     * Document offsets that delete lines [line]..=[endLine] (1-based) of [text], including the
     * line break that ends the range, or the one before it when the range ends the document.
     * Null when the range is not inside [text].
     */
    fun deleteRange(text: CharSequence, line: Int, endLine: Int): IntRange? {
        if (line < 1 || endLine < line) return null
        val lineStarts = ArrayList<Int>()
        lineStarts.add(0)
        for (index in text.indices) {
            if (text[index] == '\n') lineStarts.add(index + 1)
        }
        if (endLine > lineStarts.size) return null
        var start = lineStarts[line - 1]
        val end =
            if (endLine < lineStarts.size) {
                lineStarts[endLine]
            } else {
                // Last line of the document: take the break before the range instead.
                if (start > 0) start -= 1
                text.length
            }
        return start until end
    }

    /**
     * The range to delete in [current] for a row the answer for [snapshot] marked deletable, or
     * null when those lines are no longer byte-identical at the same line numbers (the buffer
     * changed under the popup), so a stale quick-fix never deletes other text.
     */
    fun verifiedDeleteRange(snapshot: CharSequence, current: CharSequence, row: TreeStatusDeletableRow): IntRange? {
        val expected = deleteRange(snapshot, row.line, row.endLine) ?: return null
        val actual = deleteRange(current, row.line, row.endLine) ?: return null
        if (expected != actual) return null
        if (snapshot.subSequence(expected.first, expected.last + 1).toString() !=
            current.subSequence(actual.first, actual.last + 1).toString()
        ) {
            return null
        }
        return actual
    }
}
