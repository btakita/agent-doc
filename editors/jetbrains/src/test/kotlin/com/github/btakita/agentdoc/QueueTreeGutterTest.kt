package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#queuetreegutter`: the JetBrains gutter icons and folds come from the native queue tree; the
 * plugin only maps its 1-based lines to document offsets and reconciles fold regions.
 */
class QueueTreeGutterTest {
    private val doc =
        "<!-- agent:queue id=\"main\" -->\n" + // line 1
            "- ### ~~Ship alpha [#q-alpha]~~\n" + // line 2
            "  - ~~step one~~\n" + // line 3
            "- ### ☑️ Fix gamma [#q-gamma]\n" + // line 4
            "<!-- /agent:queue -->\n"

    private val json =
        """
        {"summary":{"items":3,"counts":[]},"nodes":[
          {"line":2,"end_line":3,"depth":0,"queue":"main","state":"done","glyph":"✓","text":"Ship alpha","closed":true,"waiting_item":false,"tooltip":"done"},
          {"line":3,"end_line":3,"depth":1,"queue":"main","state":"done","glyph":"✓","text":"step one","closed":true,"waiting_item":false,"tooltip":"done"},
          {"line":4,"end_line":4,"depth":0,"queue":"main","state":"needs-integration","glyph":"☑️","text":"Fix gamma","closed":false,"waiting_item":false}
        ]}
        """.trimIndent()

    private fun lineStart(line: Int): Int = doc.split('\n').take(line - 1).sumOf { it.length + 1 }

    private fun lineEnd(line: Int): Int = lineStart(line) + doc.split('\n')[line - 1].length

    @Test
    fun `each node maps to its row line and closed subtrees fold below the row`() {
        val nodes = QueueTreeGutter.parseJson(json, doc)!!
        assertEquals(3, nodes.size)

        val alpha = nodes[0]
        assertEquals(lineStart(2), alpha.rowStart)
        assertEquals(lineEnd(2), alpha.rowEnd)
        assertEquals("✓", alpha.glyph)
        assertEquals("agent-doc queue: done", alpha.tooltip)
        assertEquals("fold starts at the end of the row line", lineEnd(2), alpha.foldStart)
        assertEquals("fold ends at the end of end_line", lineEnd(3), alpha.foldEnd)

        assertNull("a closed leaf has nothing to fold", nodes[1].foldStart)

        val gamma = nodes[2]
        assertNull("an open node never folds", gamma.foldStart)
        assertEquals(
            "an older library without tooltip shows the state label",
            "agent-doc queue: needs integration",
            gamma.tooltip,
        )

        val folds = QueueTreeGutter.folds(nodes, doc)
        assertEquals(listOf(QueueTreeFold(lineEnd(2), lineEnd(3), " … 1 line")), folds)
    }

    @Test
    fun `utf16 offsets follow multi-unit glyphs in earlier lines`() {
        val nodes = QueueTreeGutter.parseJson(json, doc)!!
        // Line 4 holds a two-unit emoji, so its end is measured in UTF-16 units like the editor.
        assertEquals(doc.indexOf("- ### ☑️"), nodes[2].rowStart)
        assertEquals(doc.indexOf("\n<!-- /agent:queue"), nodes[2].rowEnd)
    }

    @Test
    fun `malformed json keeps the last presentation`() {
        assertNull(QueueTreeGutter.parseJson("not json", doc))
        assertNull(QueueTreeGutter.parseJson("{}", doc))
    }

    @Test
    fun `an unavailable tree keeps the queue icons already shown`() {
        val queueKey = VisualHighlightKey(5, 9, 1, QueueTreeGutter.KIND, "attrs", listOf("✓", "tip"))
        val other = VisualHighlightKey(0, 4, 1, "bold", "attrs")
        assertEquals(listOf(queueKey), queueTreeGutterKeys(null, listOf(other, queueKey), 1, "attrs"))
        assertEquals(emptyList<VisualHighlightKey>(), queueTreeGutterKeys(emptyList(), listOf(queueKey), 1, "attrs"))
    }

    @Test
    fun `gutter identity round-trips through user data`() {
        val key = VisualHighlightKey(5, 9, 1, QueueTreeGutter.KIND, "attrs", listOf("✓", "tip"))
        assertEquals(key, VisualHighlightKey.fromUserData(key.toUserData()))
        val legacy = listOf<Any?>(0, 4, 1, "bold", "attrs")
        assertEquals(VisualHighlightKey(0, 4, 1, "bold", "attrs"), VisualHighlightKey.fromUserData(legacy))
    }

    @Test
    fun `a kept fold keeps the operator's expansion and a new one starts collapsed`() {
        val kept = QueueTreeFold(10, 20, " … 1 line")
        val fresh = QueueTreeFold(30, 50, " … 2 lines")
        val plan =
            planQueueTreeFolds(
                listOf(ExistingQueueTreeFold("kept", 10, 20, expanded = true), ExistingQueueTreeFold("gone", 60, 70, false)),
                listOf(kept, fresh),
                caretOffsets = listOf(0),
            )
        assertEquals(listOf("gone"), plan.remove)
        assertEquals(listOf(fresh to false), plan.add)
    }

    @Test
    fun `a regrown fold the operator had open stays open, and the caret line is never hidden`() {
        val regrown = QueueTreeFold(10, 25, " … 2 lines")
        val underCaret = QueueTreeFold(40, 60, " … 2 lines")
        val plan =
            planQueueTreeFolds(
                listOf(ExistingQueueTreeFold("old", 10, 20, expanded = true)),
                listOf(regrown, underCaret),
                caretOffsets = listOf(45),
            )
        assertEquals(listOf("old"), plan.remove)
        assertEquals(listOf(regrown to true, underCaret to true), plan.add)
        assertTrue(planQueueTreeFolds(emptyList<ExistingQueueTreeFold<String>>(), emptyList(), emptyList()).add.isEmpty())
    }
}
