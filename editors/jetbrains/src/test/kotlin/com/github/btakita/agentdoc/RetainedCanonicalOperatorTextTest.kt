package com.github.btakita.agentdoc

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#retainedprojectionclobbersoperatortext` — the 2026-08-12 lost prompt.
 *
 * Registration was refused for hours, the operator typed into a buffer that
 * reached nothing, and a compaction computed against disk before those
 * keystrokes was retained and then adopted over them on attach. The prompt
 * existed only in that buffer, so it survived nowhere: not git, not the CRDT,
 * not the compaction archive.
 */
class RetainedCanonicalOperatorTextTest {
    @Test
    fun `a live buffer past its published shadow is holding operator text`() {
        assertTrue(
            retainedCanonicalWouldClobberOperatorTextUtil(
                publishedShadow = "# doc\n\nlast published state\n",
                bufferText = "# doc\n\nlast published state\n\ndo the thing I just typed\n",
            ),
        )
    }

    @Test
    fun `a restarted IDE has no shadow, so adoption stays the recovery`() {
        // The shadow map is in-memory and does not survive a restart. Without it
        // the buffer is a stale reconstruction and canonical must win — the
        // behaviour this guard must not regress.
        assertFalse(
            retainedCanonicalWouldClobberOperatorTextUtil(
                publishedShadow = null,
                bufferText = "# doc\n\nanything at all\n",
            ),
        )
    }

    @Test
    fun `a buffer that still matches its shadow published everything it has`() {
        val text = "# doc\n\nconverged\n"
        assertFalse(
            retainedCanonicalWouldClobberOperatorTextUtil(
                publishedShadow = text,
                bufferText = text,
            ),
        )
    }

    @Test
    fun `unknown buffer text is not divergence`() {
        // A closed or unreadable document proves nothing, and guessing "diverged"
        // would strand every retained projection behind a document nobody has open.
        assertFalse(
            retainedCanonicalWouldClobberOperatorTextUtil(
                publishedShadow = "# doc\n",
                bufferText = null,
            ),
        )
    }
}
