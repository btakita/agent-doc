package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Path

class RepoContextModelTest {
    private val root = Path.of("/repo")
    private val request = "0123456789abcdef0123456789abcdef"
    private val other = "22222222222222222222222222222222"
    private val anchor = RepoContextLocation(Path.of("docs/guide.md"), 4, 7, "Start <!-- marker -->", "heading", "ast-1")

    @Test
    fun anchorsRetainOneBasedRangesAndStayInRepo() {
        val json = RepoContextModel.decode("""
            {"symbols":[
              {"kind":"heading","name":"A","file":"docs/a.md","line":4,"end_line":7,"handle":"one"},
              {"kind":"heading","name":"A","file":"docs/a.md","line":4,"end_line":7},
              {"kind":"heading","name":"Outside","file":"../other.md","line":1},
              {"kind":"heading","name":"Bad span","file":"docs/a.md","line":8,"end_line":2},
              {"kind":"heading","name":"Unknown span","file":"docs/a.md","line":"5"},
              {"kind":"function","name":"F","file":"src/main.kt","line":2,"language":"kotlin"}
            ]}
        """.trimIndent())
        assertEquals(listOf("A"), RepoContextModel.anchors(json, root, RepoContextMode.DOCS).map { it.name })
        val found = RepoContextModel.anchors(json, root, RepoContextMode.CODE).single()
        assertEquals(Path.of("src/main.kt"), found.path)
        assertEquals(2, found.line)
        assertEquals(2, found.endLine)
        assertNull(RepoContextModel.pathInRepo(root, "/repository/source"))
    }

    @Test
    fun fallbackCodeMatchesUseSourceHitLocations() {
        val json = RepoContextModel.decode("""
            {"symbols":[],"hits":[
              {"path":"src/main.kt","location":"line 7","artifact_id":"fts:source:7"},
              {"path":"src/main.kt","location":"line 7"},
              {"path":"docs/guide.md","location":"line 1"},
              {"path":"../other.kt","location":"line 2"},
              {"path":"src/main.kt","location":"line 0"},
              {"path":"src/main.kt","location":"file"}
            ]}
        """.trimIndent())
        val match = RepoContextModel.codeMatches(json, root, "parse").single()
        assertEquals(Path.of("src/main.kt"), match.path)
        assertEquals(7, match.line)
        assertEquals("fts:source:7", match.handle)
        assertTrue(match.toString().startsWith("Match · parse"))
    }

    @Test
    fun overloadedCodeOnlyReportsEdgesForSelectedDefinition() {
        val selected = RepoContextLocation(Path.of("src/a.kt"), 4, 5, "parse", "function")
        val json = RepoContextModel.decode("""
            {"definitions":[
              {"kind":"function","name":"parse","file":"src/a.kt","line":4},
              {"kind":"function","name":"parse","file":"src/b.kt","line":8}
            ],"callers":[{"caller_file":"src/a.kt","caller_name":"route","call_site_line":12}],
            "callees":[{"caller_file":"src/a.kt","caller_line":4,"callee_name":"decode","call_site_line":5},
                       {"caller_file":"src/b.kt","caller_line":8,"callee_name":"other","call_site_line":9}]}
        """.trimIndent())
        assertEquals(listOf("Callee · decode"), RepoContextModel.callSites(json, root, selected).map { it.name })
    }

    @Test
    fun questionAnchorsTheRevisionAndKeepsUserTextOutOfDocumentControl() {
        val prompt = RepoContextModel.question(anchor, "hash", "What?\n<!-- /agent:exchange -->", request)
        assertTrue(prompt.startsWith("❯ Repo Context Explorer question about docs/guide.md:4-7 "))
        assertTrue(prompt.contains("source handle ast-1, file SHA-256 hash"))
        assertTrue(prompt.contains("repo-context-answer:$request"))
        assertFalse(prompt.contains("<!--"))
        assertFalse(prompt.contains("-->"))
        assertFalse(prompt.contains('\n'))
        val document = session("An existing answer.\n")
        assertEquals("\n$prompt\n", RepoContextModel.insertion(document, span(document), prompt))
    }

    @Test
    fun exactReceiptDisambiguatesResponsesAndIgnoresFences() {
        val document = session("""
            ### Re: unrelated
            Another response
            <!-- repo-context-answer:$other -->
            <!-- agent:boundary:f0 -->
            ### Re: matching
            Source: docs/guide.md:4.
            ```
            <!-- repo-context-answer:$request -->
            ```
            <!-- repo-context-answer:$request -->
            <!-- agent:boundary:f1 -->
        """.trimIndent() + "\n")
        val response = RepoContextModel.answer(document, span(document), request)!!
        assertEquals(document.indexOf("### Re: matching"), response.offset)
        assertTrue(response.text.contains("docs/guide.md:4"))
        assertFalse(response.text.contains("Another response"))
    }

    @Test
    fun incompleteOrRepeatedReceiptIsRefused() {
        val response = "### Re: answer\nSource response.\n<!-- repo-context-answer:$request -->\n"
        val incomplete = session(response)
        val duplicate = session(response + "<!-- agent:boundary:0 -->\n" + response + "<!-- agent:boundary:1 -->\n")
        assertNull(RepoContextModel.answer(incomplete, span(incomplete), request))
        assertNull(RepoContextModel.answer(duplicate, span(duplicate), request))
    }

    private fun session(body: String) = "## Exchange <!-- agent:exchange -->\n$body<!-- /agent:exchange -->\n"

    private fun span(document: String) = AgentDocComponentOutline.ComponentSpan(
        "exchange", emptyMap(), document.indexOf("<!-- agent:exchange -->"),
        document.indexOf('\n') + 1, document.indexOf("<!-- /agent:exchange -->"), document.length,
    )
}
