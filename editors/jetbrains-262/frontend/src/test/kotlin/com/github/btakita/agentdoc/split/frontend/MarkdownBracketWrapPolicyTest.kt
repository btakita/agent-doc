package com.github.btakita.agentdoc.split.frontend

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class MarkdownBracketWrapPolicyTest {
    private val item =
        "  - [#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace " +
            "(procedure in runbooks/coder-workspace.md): install, register"

    private fun wrapBefore(text: String, word: String): Int = text.indexOf(word).also { check(it > 0) }

    @Test
    fun anIdBracketDoesNotPullTheWrapBackToTheListMarker() {
        val wrap = wrapBefore(item, "(procedure")
        val bracket = item.indexOf('[')
        assertEquals(wrap, MarkdownBracketWrapPolicy.correct(item, wrap, bracket) { true })
    }

    @Test
    fun aContinuationLineRefusalFromTheLinkGuardIsReplaced() {
        val wrap = wrapBefore(item, "install,")
        assertEquals(wrap, MarkdownBracketWrapPolicy.correct(item, wrap, MarkdownBracketWrapPolicy.NEVER_WRAP) { true })
    }

    @Test
    fun aRefusalTheLinkGuardDidNotCauseIsKept() {
        val wrap = wrapBefore(item, "install,")
        assertEquals(
            MarkdownBracketWrapPolicy.NEVER_WRAP,
            MarkdownBracketWrapPolicy.correct(item, wrap, MarkdownBracketWrapPolicy.NEVER_WRAP) { false },
        )
    }

    @Test
    fun realLinksKeepTheBundledAnswer() {
        val lines = listOf(
            "- see [the coder workspace runbook](runbooks/coder-workspace.md \"a long title\") now",
            "- see ![an image](img.png) and (more)",
            "- see [a full reference][coder] and (more)",
            "- see [a collapsed reference][] and (more)",
            "[coder]: https://example.com/runbooks/coder-workspace \"a long title\"",
            "- [#id] then [a real link](http://example.com) and (more)",
            "- [an outer [#id] link](http://example.com) and (more)",
        )
        for (line in lines) {
            val wrap = line.lastIndexOf(' ') + 1
            val bracket = line.indexOf('[')
            assertEquals(bracket, MarkdownBracketWrapPolicy.correct(line, wrap, bracket) { true }, line)
        }
    }

    @Test
    fun aWrapInsideBracketTextKeepsTheBundledAnswer() {
        val line = "- [some bracketed text that wraps] (more)"
        val wrap = wrapBefore(line, "wraps]")
        assertEquals(2, MarkdownBracketWrapPolicy.correct(line, wrap, 2) { true })
        val unclosed = "- [an unclosed bracket that wraps (more)"
        assertEquals(2, MarkdownBracketWrapPolicy.correct(unclosed, wrapBefore(unclosed, "(more"), 2) { true })
    }

    @Test
    fun answersWithoutABracketAreUnchanged() {
        val line = "- no brackets here at all (more)"
        val wrap = wrapBefore(line, "(more")
        assertEquals(5, MarkdownBracketWrapPolicy.correct(line, wrap, 5) { true })
        assertEquals(wrap, MarkdownBracketWrapPolicy.correct(item, wrap, wrap) { true })
    }

    @Test
    fun anEscapedBracketIsNotABracket() {
        val line = "- \\[#id] text (more)"
        assertFalse(MarkdownBracketWrapPolicy.onlyNonLinkBracketsBefore(line, 0, line.length, wrapBefore(line, "(more")))
    }

    @Test
    fun onlyTheWrappedLineIsScanned() {
        val text = "[a link](x)\n- [#id] text (more)"
        val wrap = wrapBefore(text, "(more")
        assertTrue(MarkdownBracketWrapPolicy.onlyNonLinkBracketsBefore(text, text.indexOf('\n') + 1, text.length, wrap))
        assertEquals(wrap, MarkdownBracketWrapPolicy.correct(text, wrap, text.indexOf("[#id]")) { true })
    }

    @Test
    fun layoutRulesThatNeverWrapAreRecognised() {
        assertTrue(MarkdownBracketWrapPolicy.mayNeverWrapByLayout("## [#id] heading (x)", 0))
        assertTrue(MarkdownBracketWrapPolicy.mayNeverWrapByLayout("| [#id] cell | (x) |", 0))
        assertTrue(MarkdownBracketWrapPolicy.mayNeverWrapByLayout("[#id] heading (x)\n---", 0))
        assertFalse(MarkdownBracketWrapPolicy.mayNeverWrapByLayout("[#id] paragraph (x)\nmore", 0))
        assertFalse(MarkdownBracketWrapPolicy.mayNeverWrapByLayout("#hashtag [#id] (x)", 0))
    }
}
