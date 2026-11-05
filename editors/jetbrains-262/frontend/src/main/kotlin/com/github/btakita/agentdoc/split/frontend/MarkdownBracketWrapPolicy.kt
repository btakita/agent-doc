package com.github.btakita.agentdoc.split.frontend

import java.util.function.BooleanSupplier

/**
 * Decides whether the bundled Markdown strategy moved a wrap because of a bracket that opens no
 * link (GH #391). Pure text rules, so the frontend unit tests cover them without an IDE.
 */
object MarkdownBracketWrapPolicy {
    /** The bundled strategy's NEVER_WRAP answer. */
    const val NEVER_WRAP = -1

    /** How far back the bundled strategy scans for a `[`, in characters. */
    const val SCAN_LIMIT = 4096

    /**
     * The wrap offset to use, given the plain [whitespaceWrap] and the bundled strategy's
     * [delegated] answer for the same request.
     *
     * Returns [whitespaceWrap] only when the bundled strategy moved the wrap back (or refused it),
     * at least one `[` precedes the wrap on its line, and none of those brackets opens a link the
     * wrap could fall inside. A [delegated] NEVER_WRAP is replaced only when
     * [neverWrapCameFromLinkGuard] confirms it came from the link guard, not from a table, an ATX
     * heading or a setext heading. Every other answer is returned unchanged.
     */
    @JvmStatic
    fun correct(
        text: CharSequence,
        whitespaceWrap: Int,
        delegated: Int,
        neverWrapCameFromLinkGuard: BooleanSupplier,
    ): Int {
        if (whitespaceWrap <= 0 || whitespaceWrap > text.length || delegated >= whitespaceWrap) return delegated
        val lineStart = lineStart(text, whitespaceWrap)
        val lineEnd = lineEnd(text, whitespaceWrap)
        if (!onlyNonLinkBracketsBefore(text, lineStart, lineEnd, whitespaceWrap)) return delegated
        if (delegated == NEVER_WRAP && !neverWrapCameFromLinkGuard.asBoolean) return delegated
        return whitespaceWrap
    }

    /**
     * A cheap pre-check before computing the whitespace wrap: whether any `[` lies between the
     * start of [startOffset]'s line and [endOffset], within [SCAN_LIMIT] of [endOffset].
     */
    @JvmStatic
    fun mayHaveBracket(text: CharSequence, startOffset: Int, endOffset: Int): Boolean {
        val end = minOf(endOffset, text.length)
        val floor = maxOf(lineStart(text, minOf(startOffset, end)), end - SCAN_LIMIT)
        for (i in floor until end) if (text[i] == '[') return true
        return false
    }

    /**
     * True when at least one unescaped `[` lies in [lineStart, wrap) within [SCAN_LIMIT], and every
     * such bracket closes before [wrap] and opens no link that is still open at [wrap]: it is not
     * followed by `(` or `[` whose destination or reference ends at or after [wrap], nor by a
     * definition `:`.
     */
    @JvmStatic
    fun onlyNonLinkBracketsBefore(text: CharSequence, lineStart: Int, lineEnd: Int, wrap: Int): Boolean {
        val floor = maxOf(lineStart, wrap - SCAN_LIMIT)
        var sawBracket = false
        for (open in wrap - 1 downTo floor) {
            if (text[open] != '[' || isEscaped(text, open, lineStart)) continue
            sawBracket = true
            if (mayOpenLinkAcross(text, lineStart, lineEnd, open, wrap)) return false
        }
        return sawBracket
    }

    /**
     * True when the line starting at [lineStart] might be one the bundled strategy never wraps for
     * layout reasons: an ATX heading, a line holding a `|` (a possible table row), or the content
     * line of a setext heading. Errs towards true, which keeps the bundled answer.
     */
    @JvmStatic
    fun mayNeverWrapByLayout(text: CharSequence, lineStart: Int): Boolean {
        val lineEnd = lineEnd(text, lineStart)
        val line = text.subSequence(lineStart, lineEnd)
        if (ATX_HEADING.containsMatchIn(line) || line.contains('|')) return true
        if (lineEnd >= text.length) return false
        val next = text.subSequence(lineEnd + 1, lineEnd(text, lineEnd + 1))
        return SETEXT_UNDERLINE.matches(next)
    }

    private val ATX_HEADING = Regex("""^ {0,3}#{1,6}(?:[ \t]|$)""")
    private val SETEXT_UNDERLINE = Regex("""^ {0,3}(?:=+|-+)[ \t]*$""")

    private fun mayOpenLinkAcross(text: CharSequence, lineStart: Int, lineEnd: Int, open: Int, wrap: Int): Boolean {
        val close = matchingClose(text, open, '[', ']', lineEnd)
        // The wrap sits inside this bracket's text, or the bracket does not close on this line:
        // it may be link text, so the bundled guard is entitled to it.
        if (close < 0 || close >= wrap) return true
        val next = close + 1
        if (next >= lineEnd) return false
        return when (text[next]) {
            // An inline link or image `[text](dest)`. It holds the wrap only while its destination
            // is still open at the wrap (`#activesummaryurl`): a link that closed before the wrap
            // cannot own a later bare URL or parenthesis, which is what the bundled guard assumes.
            '(' -> destinationOwnsWrap(text, next, lineEnd, wrap)
            // A full or collapsed reference `[text][ref]` / `[text][]`, by the same rule.
            '[' -> closesAtOrAfter(matchingClose(text, next, '[', ']', lineEnd), wrap)
            // A link reference definition `[label]: dest "title"`: the rest of the line is the link.
            ':' -> isBlank(text, lineStart, open, maxIndent = 3)
            else -> false
        }
    }

    /**
     * Whether the inline destination opening at [paren] may still be open at [wrap]. A destination
     * holding `<`, `"` or `'` may hide a `)` inside an angle destination or a title, where the
     * plain paren match would end it too early; such a link keeps the bundled answer.
     */
    private fun destinationOwnsWrap(text: CharSequence, paren: Int, lineEnd: Int, wrap: Int): Boolean {
        val close = matchingClose(text, paren, '(', ')', lineEnd)
        if (close < 0) return true
        for (i in paren + 1 until close) if (text[i] == '<' || text[i] == '"' || text[i] == '\'') return true
        return closesAtOrAfter(close, wrap)
    }

    /** An unclosed construct (`close < 0`), or one that ends at or after the wrap, holds the wrap. */
    private fun closesAtOrAfter(close: Int, wrap: Int): Boolean = close < 0 || close >= wrap

    private fun matchingClose(text: CharSequence, open: Int, openChar: Char, closeChar: Char, lineEnd: Int): Int {
        var depth = 0
        var i = open
        while (i < lineEnd) {
            when (text[i]) {
                '\\' -> i++
                openChar -> depth++
                closeChar -> if (--depth == 0) return i
            }
            i++
        }
        return -1
    }

    private fun isEscaped(text: CharSequence, offset: Int, lineStart: Int): Boolean {
        var backslashes = 0
        var i = offset - 1
        while (i >= lineStart && text[i] == '\\') {
            backslashes++
            i--
        }
        return backslashes % 2 == 1
    }

    private fun isBlank(text: CharSequence, from: Int, to: Int, maxIndent: Int): Boolean =
        to - from <= maxIndent && (from until to).all { text[it] == ' ' }

    private fun lineStart(text: CharSequence, offset: Int): Int {
        var i = offset - 1
        while (i >= 0 && text[i] != '\n') i--
        return i + 1
    }

    private fun lineEnd(text: CharSequence, offset: Int): Int {
        var i = offset
        while (i < text.length && text[i] != '\n') i++
        return i
    }
}
