package com.github.btakita.agentdoc

import com.github.btakita.agentdoc.AgentDocComponentOutline.ComponentSpan
import com.github.btakita.agentdoc.AgentDocComponentOutline.ItemKind
import com.intellij.codeInsight.daemon.LineMarkerProvider
import com.intellij.ide.structureView.StructureViewExtension
import com.intellij.lang.folding.FoldingBuilderEx
import com.intellij.openapi.project.DumbAware
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Element

/** GH #19 (plugin UX phases 5-7): component folding, gutter markers, and structure view. */
class AgentDocComponentOutlineTest {

    /**
     * Build the `agent_doc_parse_components` JSON for [names] in [doc] exactly as the native
     * parser reports it: UTF-8 byte offsets, `open_end`/`close_end` past the trailing newline.
     */
    private fun nativeJson(doc: String, vararg names: String): String {
        fun bytes(charIndex: Int) = doc.substring(0, charIndex).toByteArray(Charsets.UTF_8).size
        fun markerEnd(start: Int): Int {
            var end = doc.indexOf("-->", start) + 3
            if (end < doc.length && doc[end] == '\n') end++
            return end
        }
        return names.joinToString(",", "[", "]") { name ->
            val openStart = doc.indexOf("<!-- agent:$name")
            val closeStart = doc.indexOf("<!-- /agent:$name -->")
            require(openStart >= 0 && closeStart > openStart) { name }
            val openEnd = markerEnd(openStart)
            val closeEnd = markerEnd(closeStart)
            val attrs = doc.substring(openStart + "<!-- agent:$name".length, doc.indexOf("-->", openStart))
                .trim().split(Regex("\\s+")).filter { it.contains('=') }
                .joinToString(",", "{", "}") { kv -> kv.split('=', limit = 2).let { "\"${it[0]}\":\"${it[1]}\"" } }
            """{"name":"$name","attrs":$attrs,"open_start":${bytes(openStart)},"open_end":${bytes(openEnd)},""" +
                """"close_start":${bytes(closeStart)},"close_end":${bytes(closeEnd)},"content":""}"""
        }
    }

    private fun spans(doc: String, vararg names: String): List<ComponentSpan> =
        AgentDocComponentOutline.spansFromNativeJson(nativeJson(doc, *names), doc)

    private val doc = """
        |---
        |agent_doc_session: s-1
        |---
        |# Plan — café ☕ 😀
        |
        |<!-- agent:exchange patch=append -->
        |### Re: first 😀
        |- detail
        |### Re: second
        |```
        |# not a heading
        |```
        |<!-- /agent:exchange -->
        |
        |<!-- agent:backlog -->
        |- [ ] one [#a1]
        |  - nested detail
        |- [x] two
        |1. three
        |<!-- agent:inner -->
        |- inner item
        |<!-- /agent:inner -->
        |<!-- /agent:backlog -->
        |<!-- agent:queue --><!-- /agent:queue -->
        |""".trimMargin()

    private val all by lazy { spans(doc, "exchange", "backlog", "inner", "queue") }
    private fun span(name: String) = all.single { it.name == name }

    @Test
    fun `native utf8 byte offsets map to utf16 editor offsets`() {
        val exchange = span("exchange")
        assertEquals(doc.indexOf("<!-- agent:exchange"), exchange.openStart)
        assertEquals(doc.indexOf("### Re: first"), exchange.openEnd)
        assertEquals(doc.indexOf("<!-- /agent:exchange -->"), exchange.closeStart)
        assertEquals(doc.indexOf("\n<!-- agent:backlog") , exchange.closeEnd)
        assertEquals(mapOf("patch" to "append"), exchange.attrs)
        assertEquals(listOf("exchange", "backlog", "inner", "queue"), all.map { it.name })
    }

    @Test
    fun `utf8 offset conversion counts surrogate pairs as four bytes and two units`() {
        val text = "a😀é☕b"
        val map = AgentDocComponentOutline.utf8ToUtf16Offsets(text, listOf(0, 1, 5, 7, 10, 11, 99))
        assertEquals(mapOf(0 to 0, 1 to 1, 5 to 3, 7 to 4, 10 to 5, 11 to 6, 99 to 6), map)
    }

    @Test
    fun `items prefer headings, else unindented list items, skipping code fences and nested components`() {
        val exchange = AgentDocComponentOutline.items(doc, span("exchange"), all)
        assertEquals(listOf("Re: first 😀", "Re: second"), exchange.map { it.label })
        assertTrue(exchange.all { it.kind == ItemKind.HEADING })
        assertEquals(doc.indexOf("### Re: second"), exchange[1].offset)

        val backlog = AgentDocComponentOutline.items(doc, span("backlog"), all)
        assertEquals(listOf("one [#a1]", "two", "three"), backlog.map { it.label })
        assertTrue(backlog.all { it.kind == ItemKind.LIST_ITEM })

        assertEquals(listOf("inner item"), AgentDocComponentOutline.items(doc, span("inner"), all).map { it.label })
        assertEquals(emptyList<Any>(), AgentDocComponentOutline.items(doc, span("queue"), all))
    }

    @Test
    fun `fold regions cover each multi-line component with a name and item count placeholder`() {
        val folds = AgentDocComponentOutline.foldSpecs(doc, all)
        assertEquals(listOf("exchange", "backlog", "inner"), folds.map { it.name })
        val exchange = folds[0]
        assertEquals(doc.indexOf("<!-- agent:exchange"), exchange.start)
        assertEquals(doc.indexOf("<!-- /agent:exchange -->") + "<!-- /agent:exchange -->".length, exchange.end)
        assertEquals("<!-- agent:exchange · 2 items -->", exchange.placeholder)
        assertEquals("<!-- agent:backlog · 3 items -->", folds[1].placeholder)
        assertEquals("<!-- agent:inner · 1 item -->", folds[2].placeholder)
        // Nested folds nest inside their parent instead of overlapping it.
        assertTrue(folds[1].start < folds[2].start && folds[2].end < folds[1].end)
    }

    @Test
    fun `single-line and empty components are not folded`() {
        val text = "<!-- agent:queue --><!-- /agent:queue -->\n"
        assertEquals(emptyList<Any>(), AgentDocComponentOutline.foldSpecs(text, spans(text, "queue")))
        assertEquals("<!-- agent:queue · empty -->", AgentDocComponentOutline.placeholder(spans(text, "queue")[0], 0))
    }

    @Test
    fun `gutter marker claims exactly one leaf per component open marker`() {
        // Partition the document into arbitrary leaves (the Markdown PSI shape is not part of
        // the contract) and require each open marker to be claimed exactly once.
        val cuts = (listOf(0, doc.length) + all.flatMap { listOf(it.openStart + 3, it.closeStart) } +
            (0 until doc.length step 7)).distinct().sorted()
        val claimed = cuts.zipWithNext().mapNotNull { (s, e) ->
            AgentDocComponentOutline.componentOpeningIn(all, s, e)
        }
        assertEquals(all.map { it.name }, claimed.map { it.name })
        assertNull(AgentDocComponentOutline.componentOpeningIn(all, 0, 0))
        assertNull(AgentDocComponentOutline.componentOpeningIn(emptyList(), 0, doc.length))
    }

    @Test
    fun `gutter tooltip names the component, item count and attributes, escaped`() {
        val tooltip = AgentDocComponentOutline.tooltip(doc, span("exchange"), all)
        assertEquals("<html><b>agent:exchange</b> · 2 items<br>patch=append<br>Click to fold or unfold</html>", tooltip)
        val odd = ComponentSpan("a<b", mapOf("k" to "\"&\""), 0, 1, 2, 3)
        assertTrue(AgentDocComponentOutline.tooltip("xxxx", odd, listOf(odd)).contains("agent:a&lt;b</b>"))
        assertTrue(AgentDocComponentOutline.tooltip("xxxx", odd, listOf(odd)).contains("k=&quot;&amp;&quot;"))
    }

    @Test
    fun `innermost component at an offset`() {
        val innerItem = doc.indexOf("- inner item")
        assertEquals("inner", AgentDocComponentOutline.componentAt(all, innerItem)?.name)
        assertEquals("backlog", AgentDocComponentOutline.componentAt(all, doc.indexOf("1. three"))?.name)
        assertNull(AgentDocComponentOutline.componentAt(all, 0))
    }

    @Test
    fun `structure model lists components with items and nests inner components`() {
        val tree = AgentDocComponentOutline.structure(doc, all)
        assertEquals(listOf("agent:exchange", "agent:backlog", "agent:queue"), tree.map { it.presentableText })
        assertEquals(listOf("2 items", "3 items", "empty"), tree.map { it.locationString })
        val backlog = tree[1]
        assertEquals(listOf("agent:inner"), backlog.children.map { it.presentableText })
        assertEquals(listOf("inner item"), backlog.children.single().items.map { it.label })
        assertTrue(tree[0].children.isEmpty())
    }

    @Test
    fun `long item labels are truncated`() {
        val text = "<!-- agent:backlog -->\n- ${"x".repeat(200)}\n<!-- /agent:backlog -->\n"
        val label = AgentDocComponentOutline.items(text, spans(text, "backlog")[0], spans(text, "backlog")).single().label
        assertEquals(80, label.length)
        assertTrue(label.endsWith("…"))
    }

    // ---- registration -----------------------------------------------------------------------

    private fun resource(name: String) = File("src/main/resources/META-INF/$name")

    @Test
    fun `markdown extensions are registered through an optional markdown dependency`() {
        val plugin = resource("plugin.xml").readText()
        assertTrue(
            plugin.contains(
                """<depends optional="true" config-file="agent-doc-markdown.xml">org.intellij.plugins.markdown</depends>""",
            ),
        )
        val xml = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(resource("agent-doc-markdown.xml"))
        fun attr(tag: String, attribute: String) =
            (xml.getElementsByTagName(tag).item(0) as Element).getAttribute(attribute)
        fun load(name: String): Class<*> = Class.forName(name, false, javaClass.classLoader)

        val folding = load(attr("lang.foldingBuilder", "implementationClass"))
        assertEquals("Markdown", attr("lang.foldingBuilder", "language"))
        assertTrue(FoldingBuilderEx::class.java.isAssignableFrom(folding))
        assertTrue("folding must work while indexing", DumbAware::class.java.isAssignableFrom(folding))

        val marker = load(attr("codeInsight.lineMarkerProvider", "implementationClass"))
        assertEquals("Markdown", attr("codeInsight.lineMarkerProvider", "language"))
        assertTrue(LineMarkerProvider::class.java.isAssignableFrom(marker))
        assertTrue(DumbAware::class.java.isAssignableFrom(marker))

        val structure = load(attr("lang.structureViewExtension", "implementation"))
        assertTrue(StructureViewExtension::class.java.isAssignableFrom(structure))
    }

    @Test
    fun `extension callbacks never call native code`() {
        // Folding, line markers and structure run inside read actions (some on the EDT); the
        // native parse belongs to VisualHighlighterManager's background refresh only.
        val dir = File("src/main/kotlin/com/github/btakita/agentdoc")
        for (name in listOf(
            "AgentDocComponentFoldingBuilder.kt",
            "AgentDocComponentLineMarkerProvider.kt",
            "AgentDocComponentStructureViewExtension.kt",
            "ComponentOutlineStore.kt",
        )) {
            val source = File(dir, name).readText()
            assertFalse(name, source.contains("NativePatching"))
            assertFalse(name, source.contains("AgentDocLib"))
        }
        val highlighter = File(dir, "VisualHighlighterManager.kt").readText()
        assertTrue(highlighter.contains("NativePatching.componentSpansOrNull(snapshot.text)"))
    }
}
