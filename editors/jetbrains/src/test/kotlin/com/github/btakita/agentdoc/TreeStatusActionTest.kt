package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

/**
 * `#treestatusjetbrains`: "Agent Doc: Tree Status" shows the native tree-status `text` verbatim
 * and offers one delete quick-fix per native `deletable` row (`line..=end_line`).
 */
class TreeStatusActionTest {
    private val doc =
        "<!-- agent:queue id=\"main\" -->\n" + // line 1
            "- ~~do [#alpha]~~\n" + // line 2
            "  - ~~step one~~\n" + // line 3
            "- 🚧 [#subagent:beta#1] do [#beta]\n" + // line 4
            "<!-- /agent:queue -->\n" // line 5

    private val nativeText =
        "L1–L5 queue main — 0 you · 1 agent · 1 deletable · 0 need review\n" +
            "3 items: 2 done · 1 running\n" +
            "Next: you: none\n" +
            "Next: agent:\n" +
            "         L4 🚧 do [#beta] — let the running attempt finish and report\n" +
            "Deletable:\n" +
            "      L2–L3 ✓ do [#alpha] — every row is struck and its response records no remaining work\n" +
            "Needs agent review: none\n"

    /** The shape `agent_doc_queue_tree_status_json` returns for [doc] at line 1. */
    private val json =
        """
        {"queue":"main","line":1,"end_line":5,"whole_queue":true,"text":${com.google.gson.JsonPrimitive(nativeText)},
         "summary":{"items":3,"counts":[{"state":"done","count":2},{"state":"running","count":1}]},
         "you":[],
         "agent":[{"line":4,"end_line":4,"state":"running","glyph":"🚧","text":"do [#beta]","id":"beta","action":"let the running attempt finish and report"}],
         "deletable":[{"line":2,"end_line":3,"state":"done","glyph":"✓","text":"do [#alpha]","id":"alpha","action":"every row is struck and its response records no remaining work"}],
         "needs_review":[]}
        """.trimIndent()

    @Test
    fun `answer keeps the native text verbatim and every deletable row`() {
        val answer = QueueTreeStatus.parseJson(json)!!
        assertEquals(nativeText, answer.text)
        assertEquals(listOf(TreeStatusDeletableRow(2, 3, "✓", "do [#alpha]", "alpha")), answer.deletable)
        assertNull("not a tree-status answer", QueueTreeStatus.parseJson("""{"nodes":[]}"""))
        assertNull(QueueTreeStatus.parseJson("not json"))
    }

    @Test
    fun `popup shows the text lines unselectable, then one delete per deletable row, then copy`() {
        val items = QueueTreeStatus.popupItems(QueueTreeStatus.parseJson(json)!!)
        val lines = nativeText.trimEnd('\n').split('\n')
        assertEquals(lines, items.take(lines.size).map { it.label })
        assertTrue(items.take(lines.size).none { it.selectable })
        val fixes = items.drop(lines.size)
        assertEquals(
            listOf("Delete L2–L3: ✓ do [#alpha]", "Copy tree status"),
            fixes.map { it.label },
        )
        assertTrue(fixes.all { it.selectable })
        assertEquals("the separator goes above the first quick-fix", fixes.first(), QueueTreeStatus.firstQuickFix(items))
        assertEquals("L4", TreeStatusPopupItem.lineRange(4, 4))
    }

    @Test
    fun `an answer with nothing deletable offers only copy`() {
        val items = QueueTreeStatus.popupItems(QueueTreeStatusAnswer("L2 do [#beta] — 0 you\n", emptyList()))
        assertEquals(listOf("L2 do [#beta] — 0 you", "Copy tree status"), items.map { it.label })
    }

    @Test
    fun `delete quick-fix removes exactly lines line through end_line`() {
        val range = QueueTreeStatus.deleteRange(doc, 2, 3)!!
        val after = doc.removeRange(range)
        assertEquals(
            "<!-- agent:queue id=\"main\" -->\n" +
                "- 🚧 [#subagent:beta#1] do [#beta]\n" +
                "<!-- /agent:queue -->\n",
            after,
        )
        assertNull("end_line past the document", QueueTreeStatus.deleteRange(doc, 2, 99))
        assertNull("line 0 is not a 1-based line", QueueTreeStatus.deleteRange(doc, 0, 1))
        assertNull("inverted range", QueueTreeStatus.deleteRange(doc, 3, 2))
    }

    @Test
    fun `delete quick-fix at the document end takes the break before the range`() {
        val text = "a\nb\nc"
        assertEquals("a\nb", text.removeRange(QueueTreeStatus.deleteRange(text, 3, 3)!!))
        assertEquals("a", text.removeRange(QueueTreeStatus.deleteRange(text, 2, 3)!!))
        assertEquals("", text.removeRange(QueueTreeStatus.deleteRange(text, 1, 3)!!))
    }

    @Test
    fun `a stale quick-fix never deletes text that moved or changed`() {
        val row = TreeStatusDeletableRow(2, 3, "✓", "do [#alpha]", "alpha")
        assertEquals(QueueTreeStatus.deleteRange(doc, 2, 3), QueueTreeStatus.verifiedDeleteRange(doc, doc, row))
        val edited = doc.replace("  - ~~step one~~\n", "  - ~~step one~~ and more\n")
        assertNull("the rows changed", QueueTreeStatus.verifiedDeleteRange(doc, edited, row))
        val shifted = "# heading\n$doc"
        assertNull("the rows moved down a line", QueueTreeStatus.verifiedDeleteRange(doc, shifted, row))
        val appended = doc + "after\n"
        assertEquals(
            "an edit after the rows leaves them deletable",
            QueueTreeStatus.deleteRange(doc, 2, 3),
            QueueTreeStatus.verifiedDeleteRange(doc, appended, row),
        )
    }

    private fun actionElements(): List<Element> {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder()
            .parse(File("src/main/resources/META-INF/plugin.xml"))
        val nodes = document.getElementsByTagName("action")
        return (0 until nodes.length).map { nodes.item(it) as Element }
    }

    private fun Element.children(tag: String): List<Element> {
        val nodes = getElementsByTagName(tag)
        return (0 until nodes.length).map { nodes.item(it) as Element }
    }

    private fun Element.defaultKeystrokes(): List<String> =
        children("keyboard-shortcut")
            .filter { it.getAttribute("keymap") == "\$default" }
            .map { it.getAttribute("first-keystroke").trim().lowercase().split(Regex("\\s+")).joinToString(" ") }

    @Test
    fun `tree status is declared with alt shift s, in the menus and the popup`() {
        val action = actionElements().single { it.getAttribute("id") == "AgentDoc.TreeStatus" }
        // GH #339 R1: the platform holds the shell forwarder; TreeStatusAction is its handler.
        assertEquals("com.github.btakita.agentdoc.shell.TreeStatusForwarder", action.getAttribute("class"))
        assertEquals("Agent Doc: Tree Status", action.getAttribute("text"))
        assertTrue(AgentDocImplGeneration.ACTION_HANDLERS.getValue("AgentDoc.TreeStatus")() is TreeStatusAction)
        assertEquals(listOf("alt shift s"), action.defaultKeystrokes())
        val otherDefaults = actionElements()
            .filter { it.getAttribute("id") != "AgentDoc.TreeStatus" }
            .flatMap { it.defaultKeystrokes() }
        assertFalse("alt shift s collides with another agent-doc default", "alt shift s" in otherDefaults)
        val groups = action.children("add-to-group").map { it.getAttribute("group-id") }
        assertTrue("tree status must be in the Tools menu: $groups", "ToolsMenu" in groups)
        assertTrue("tree status must be in the editor context menu: $groups", "EditorPopupMenu" in groups)
        assertTrue(
            "tree status must be reachable from the Agent Doc popup",
            "AgentDoc.TreeStatus" in AgentDocPopupAction.PRIMARY_ACTION_IDS,
        )
    }
}
