package com.github.btakita.agentdoc.split.frontend

import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.LanguageLineWrapPositionStrategy
import com.intellij.openapi.editor.LineWrapPositionStrategy
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import org.intellij.plugins.markdown.lang.MarkdownLanguage

/**
 * GH #391 against the real bundled Markdown wrap strategy and lexer highlighter. Each case asks
 * both strategies for every right margin across the item's line.
 */
class MarkdownBracketWrapStrategyPlatformTest : BasePlatformTestCase() {
    private val item =
        "  - [#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace " +
            "(procedure in runbooks/coder-workspace.md): install, register, remote extension host, " +
            "tmux attach, remoteName, layout sync; then update the VS Code on Coder status cell."

    private lateinit var bundled: LineWrapPositionStrategy
    private lateinit var ours: MarkdownBracketWrapStrategy

    override fun setUp() {
        super.setUp()
        bundled = LanguageLineWrapPositionStrategy.INSTANCE.forLanguage(MarkdownLanguage.INSTANCE)
        assertTrue(
            "the bundled Markdown strategy answers before ours is registered: ${bundled.javaClass.name}",
            bundled.javaClass.name.startsWith("org.intellij.plugins.markdown."),
        )
        ours = MarkdownBracketWrapStrategy()
        LanguageLineWrapPositionStrategy.INSTANCE.addExplicitExtension(MarkdownLanguage.INSTANCE, ours, testRootDisposable)
    }

    private fun open(text: String): Editor {
        myFixture.configureByText("session.md", text)
        val editor = myFixture.editor
        assertSame(ours, LanguageLineWrapPositionStrategy.INSTANCE.forEditor(editor))
        return editor
    }

    private fun LineWrapPositionStrategy.wrap(editor: Editor, start: Int, maxPreferred: Int): Int =
        MarkdownBracketWrapStrategy.editorWrapPosition(
            this, editor, start, editor.document.getLineEndOffset(0), maxPreferred, false, true,
        )

    /** 263 ships the bundled strategy whose link guard strands the marker; 262 GA does not. */
    private val bundledHasTheBug: Boolean
        get() = ApplicationInfo.getInstance().build.baselineVersion >= 263

    private fun margins(editor: Editor, from: Int): IntRange = from until editor.document.getLineEndOffset(0)

    fun testTheListMarkerKeepsItsFirstWords() {
        val editor = open(item)
        val bracket = item.indexOf('[')
        // Once the margin clears "Run ", the first visual line must carry more than "  - ".
        val margins = margins(editor, item.indexOf("Run ") + "Run ".length)
        // The bundled guard wraps at the bracket or, inside the indent, before the marker.
        val stranded = margins.filter { bundled.wrap(editor, 0, it) in 0..bracket }
        if (bundledHasTheBug) {
            assertFalse("the bundled strategy no longer strands the marker; GH #391's workaround may retire", stranded.isEmpty())
        }
        for (margin in margins) {
            val wrap = ours.wrap(editor, 0, margin)
            assertTrue("margin $margin wrapped at $wrap, stranding the list marker", wrap > bracket)
            assertTrue("margin $margin wrapped at $wrap, past the margin", wrap <= margin)
        }
    }

    fun testContinuationVisualLinesStillWrap() {
        val line = "  - [#vscodecodersmoke] Run the smoke (procedure in the runbook): install (a), " +
            "register (b), attach (c), sync (d), then update (e) the status cell in the spec."
        val editor = open(line)
        // A continuation visual line starts after the bracket; the bundled guard then moves the
        // wrap to a bracket before the line's start, which it answers with NEVER_WRAP.
        val start = line.indexOf("install")
        val margins = margins(editor, line.indexOf("(a), ") + "(a), ".length)
        if (bundledHasTheBug) {
            assertTrue(margins.any { bundled.wrap(editor, start, it) == MarkdownBracketWrapPolicy.NEVER_WRAP })
        }
        for (margin in margins) {
            val wrap = ours.wrap(editor, start, margin)
            assertTrue("margin $margin from $start wrapped at $wrap", wrap in (start + 1)..margin)
        }
    }

    fun testRealLinksAndPlainLinesKeepTheBundledAnswer() {
        val titled = "- see [the coder workspace runbook](runbooks/coder-workspace.md \"with a title\") for (the details) now"
        val inline = "- [#id] then [a real link](http://example.com/a/long/path) for (the details) and more words here"
        val reference = "- see [a full reference][coder] for (the details) and more words here to wrap"
        // Each line with the last margin at which the two strategies must agree. A titled
        // destination may hide a `)`, so it keeps the bundled answer everywhere; a link without a
        // title holds the wrap only until it closes (`#activesummaryurl`).
        val lines = listOf(
            titled to titled.length,
            inline to inline.indexOf(')'),
            reference to reference.indexOf("[coder]") + "[coder]".length - 1,
            "- no brackets on this line, only (a parenthetical remark) and more words here".let { it to it.length },
            "## [#id] a heading never wraps in the bundled strategy (even with a paren) here".let { it to it.length },
            "- a bare autolink https://example.com/a/very/long/path/that/keeps/going and (more)".let { it to it.length },
        )
        for ((line, lastMargin) in lines) {
            val editor = open(line)
            for (start in listOf(0, line.indexOf(' ', line.length / 2) + 1)) {
                for (margin in margins(editor, start + 2).filter { it <= lastMargin }) {
                    assertEquals(
                        "line `$line` from $start at margin $margin",
                        bundled.wrap(editor, start, margin),
                        ours.wrap(editor, start, margin),
                    )
                }
            }
        }
    }

    /**
     * `#activesummaryurl`: a real link that closed earlier on the line does not own a later bare
     * URL. The bundled guard moves the wrap in front of that link (stranding the list marker), and
     * on a continuation visual line it refuses to wrap, so the URL is split mid-word.
     */
    fun testAClosedLinkBeforeABareUrlDoesNotPullTheWrapBack() {
        val line = "- See [the notes](#notes) before you start working on the item, then " +
            "https://example.com/group/project/-/issues/42/a/very/long/path"
        val editor = open(line)
        val bracket = line.indexOf('[')
        val linkEnd = line.indexOf(')')
        val url = line.indexOf("https://")
        val margins = margins(editor, url + 1)
        if (bundledHasTheBug) {
            assertFalse(
                "the bundled strategy no longer moves the wrap to the closed link; #activesummaryurl's rule may retire",
                margins.none { bundled.wrap(editor, 0, it) in 0..bracket },
            )
        }
        for (margin in margins) {
            val wrap = ours.wrap(editor, 0, margin)
            assertEquals("margin $margin wrapped at $wrap, not in front of the bare URL", url, wrap)
        }
        val start = line.indexOf("before")
        assertTrue(start > linkEnd)
        if (bundledHasTheBug) {
            assertTrue(margins.any { bundled.wrap(editor, start, it) == MarkdownBracketWrapPolicy.NEVER_WRAP })
        }
        for (margin in margins) {
            val wrap = ours.wrap(editor, start, margin)
            assertEquals("margin $margin from $start wrapped at $wrap", url, wrap)
        }
    }
}
