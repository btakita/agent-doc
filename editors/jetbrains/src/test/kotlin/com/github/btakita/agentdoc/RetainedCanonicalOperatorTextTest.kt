package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
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
    fun `a live buffer is published when retained canonical is its exact shadow`() {
        val shadow = "# doc\n\nlast published state\n"
        assertEquals(
            RetainedRegistrationProjectionAction.PublishOperatorBuffer,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = shadow,
                bufferText = "$shadow\ndo the thing I just typed\n",
                canonicalText = shadow,
            ),
        )
    }

    @Test
    fun `three divergent generations hold without overwriting the operator`() {
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = "# doc\n\nlast published state\n",
                bufferText = "# doc\n\nlast published state\n\ndo the thing I just typed\n",
                canonicalText = "# doc\n\na different remote generation\n",
            ),
        )
    }

    @Test
    fun `a restarted IDE has no shadow, so adoption stays the recovery`() {
        // The shadow map is in-memory and does not survive a restart. Without it
        // the buffer is a stale reconstruction and canonical must win — the
        // behaviour this guard must not regress.
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = null,
                bufferText = "# doc\n\nanything at all\n",
                canonicalText = "# doc\n\ncontroller state\n",
            ),
        )
    }

    @Test
    fun `an unacknowledged local shadow cannot authorize an older controller overwrite`() {
        val localText = "# doc\n\noperator edit accepted only by the retiring controller\n"
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = localText,
                bufferText = localText,
                canonicalText = "# doc\n\nolder replacement controller snapshot\n",
            ),
        )
    }

    @Test
    fun `a replacement canonical that catches the live buffer ends the hold`() {
        val liveBuffer = "# doc\n\noperator edit\n"
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = "# doc\n\nolder settled projection\n",
                bufferText = liveBuffer,
                canonicalText = liveBuffer,
            ),
        )
    }

    @Test
    fun `an ambiguous hold suppresses refresh until its retry is due`() {
        assertFalse(
            retainedProjectionHoldAllowsRefreshUtil(
                holdActive = true,
                registrationAttemptDue = false,
            ),
        )
        assertTrue(
            retainedProjectionHoldAllowsRefreshUtil(
                holdActive = true,
                registrationAttemptDue = true,
            ),
        )
        assertTrue(
            retainedProjectionHoldAllowsRefreshUtil(
                holdActive = false,
                registrationAttemptDue = false,
            ),
        )
    }

    @Test
    fun `unknown buffer text is not divergence`() {
        // A closed or unreadable document proves nothing, and guessing "diverged"
        // would strand every retained projection behind a document nobody has open.
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            retainedRegistrationProjectionActionUtil(
                publishedShadow = "# doc\n",
                bufferText = null,
                canonicalText = "# doc\n\ncontroller response\n",
            ),
        )
    }
}
