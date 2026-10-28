package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#ackedreplicabase` — agent-doc-bugs.md, 2026-10-28 12:57:53.
 *
 * The operator typed a queue row without pause. Each burst reached the controller through the
 * cached replica, but the settled shadow advances only on an accepted visible projection, which
 * every keystroke deferred, so it stayed 38 s old. The controller meanwhile wrote a resume id into
 * the frontmatter (undelivered), and a replace delivery made the editor re-register. Canonical
 * held the typing minus the last keystroke (a one-character retype) plus the resume id. From the
 * stale shadow, canonical's copy of the typing and the buffer's copy are two different changes of
 * one line, so neither the containment proof nor the line merge answered, and registration held
 * `ambiguous-retained-projection` although the two sides touched different lines.
 *
 * The merge and proof here are line-granular fakes of the native rebase and containment calls:
 * they refuse any line both sides changed differently, as the native rebase does.
 */
class AcknowledgedReplicaBaseTest {

    private val shadow = "---\nresume: old-conversation\n---\n- run row\n"
    private val acknowledged = "---\nresume: old-conversation\n---\n- run row the operator typed\n"
    private val buffer = "---\nresume: old-conversation\n---\n- run row the operator typod\n"
    private val canonical = "---\nresume: new-conversation\n---\n- run row the operator typed\n"

    private fun lines(text: String) = text.split('\n')

    /** Line-wise three-way merge that refuses a line both sides changed differently. */
    private fun lineMerge(base: String, buffer: String, canonical: String): String? {
        val b = lines(base)
        val o = lines(buffer)
        val c = lines(canonical)
        if (b.size != o.size || b.size != c.size) return null
        if (buffer == canonical || buffer == base || canonical == base) return null
        return b.indices.map { i ->
            when {
                o[i] == b[i] -> c[i]
                c[i] == b[i] || c[i] == o[i] -> o[i]
                else -> return null
            }
        }.joinToString("\n")
    }

    /** Every line the operator changed since [base] already reads the same in canonical. */
    private fun lineContained(base: String, buffer: String, canonical: String): Boolean {
        val b = lines(base)
        val o = lines(buffer)
        val c = lines(canonical)
        if (b.size != o.size || b.size != c.size) return false
        return b.indices.all { i -> o[i] == b[i] || c[i] == o[i] }
    }

    private fun proof(acknowledgedBase: String?, canonicalText: String = canonical, bufferText: String = buffer) =
        registrationCausalProofUtil(
            publishedShadow = shadow,
            bufferText = bufferText,
            canonicalText = canonicalText,
            acknowledgedBase = acknowledgedBase,
            shadowContained = lineContained(shadow, bufferText, canonicalText),
            contained = ::lineContained,
            merge = ::lineMerge,
        )

    private fun decide(proof: RegistrationCausalProof, bufferText: String = buffer, canonicalText: String = canonical) =
        retainedRegistrationProjectionActionForAttachUtil(
            deferCanonicalProjectionForPendingLocal = false,
            canonicalProjectionRetained = true,
            canonicalCoversRetainedFrontier = true,
            publishedShadow = shadow,
            bufferText = bufferText,
            canonicalText = canonicalText,
            canonicalContainsOperatorEdits = proof.canonicalContainsOperatorEdits,
            cleanMergeAvailable = proof.cleanMergeAvailable,
        )

    @Test
    fun `the stale settled shadow alone holds the live 12-57 sequence`() {
        val fromShadow = proof(acknowledgedBase = null)
        assertEquals(false, fromShadow.canonicalContainsOperatorEdits)
        assertFalse(fromShadow.cleanMergeAvailable)
        assertEquals(shadow, fromShadow.mergeBase)
        assertEquals(RetainedRegistrationProjectionAction.HoldOperatorBuffer, decide(fromShadow))
    }

    @Test
    fun `the acknowledged replica text merges the retype beside the controller write`() {
        val fromAcknowledged = proof(acknowledgedBase = acknowledged)
        assertTrue(fromAcknowledged.cleanMergeAvailable)
        assertEquals(acknowledged, fromAcknowledged.mergeBase)
        assertEquals(RetainedRegistrationProjectionAction.MergeForward, decide(fromAcknowledged))
        assertEquals(
            "both sides survive: the operator's retype and the controller's resume id",
            "---\nresume: new-conversation\n---\n- run row the operator typod\n",
            registrationMergeFromBaseUtil(fromAcknowledged.mergeBase, buffer, canonical, ::lineMerge),
        )
    }

    @Test
    fun `a buffer equal to the acknowledged text adopts canonical losslessly`() {
        val fromAcknowledged = proof(acknowledgedBase = acknowledged, bufferText = acknowledged)
        assertEquals(true, fromAcknowledged.canonicalContainsOperatorEdits)
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            decide(fromAcknowledged, bufferText = acknowledged),
        )
    }

    @Test
    fun `canonical unmoved past the acknowledged text publishes the buffer`() {
        val fromAcknowledged = proof(acknowledgedBase = acknowledged, canonicalText = acknowledged)
        assertTrue(fromAcknowledged.cleanMergeAvailable)
        assertEquals(buffer, registrationMergeFromBaseUtil(acknowledged, buffer, acknowledged, ::lineMerge))
    }

    @Test
    fun `a controller change of the line the operator retyped still holds`() {
        // Canonical rewrote the same row the operator is retyping: no ancestor proves either
        // direction lossless, so the buffer stays exactly as typed.
        val conflicting = "---\nresume: new-conversation\n---\n- run row rewritten by the controller\n"
        val fromAcknowledged = proof(acknowledgedBase = acknowledged, canonicalText = conflicting)
        assertFalse(fromAcknowledged.cleanMergeAvailable)
        assertEquals(false, fromAcknowledged.canonicalContainsOperatorEdits)
        assertEquals(shadow, fromAcknowledged.mergeBase)
        assertEquals(
            RetainedRegistrationProjectionAction.HoldOperatorBuffer,
            decide(fromAcknowledged, canonicalText = conflicting),
        )
    }

    @Test
    fun `a proof the settled shadow already gives keeps its ancestor`() {
        // The shadow-derived answer is unchanged by this rule.
        val typedOnTop = "---\nresume: old-conversation\n---\n- run row\n- a new row\n"
        val p =
            registrationCausalProofUtil(
                publishedShadow = shadow,
                bufferText = shadow.replace("- run row\n", "- run row typed\n"),
                canonicalText = shadow.replace("resume: old-conversation", "resume: new-conversation"),
                acknowledgedBase = typedOnTop,
                shadowContained = false,
                contained = ::lineContained,
                merge = ::lineMerge,
            )
        assertTrue(p.cleanMergeAvailable)
        assertEquals(shadow, p.mergeBase)
    }

    @Test
    fun `a restarted IDE without a settled shadow keeps its historical adoption path`() {
        val p =
            registrationCausalProofUtil(
                publishedShadow = null,
                bufferText = buffer,
                canonicalText = canonical,
                acknowledgedBase = acknowledged,
                shadowContained = null,
                contained = ::lineContained,
                merge = ::lineMerge,
            )
        assertNull(p.canonicalContainsOperatorEdits)
        assertFalse(p.cleanMergeAvailable)
        assertNull(p.mergeBase)
    }

    @Test
    fun `the acknowledged text is an ancestor only when the controller covers its frontier`() {
        assertEquals(
            acknowledged,
            acknowledgedReplicaBaseUtil(
                resumeStateFromCachedEndpoint = true,
                cachedTextBeforeCapture = acknowledged,
                cachedTextAfterCapture = acknowledged,
                canonicalCoversRetainedFrontier = true,
            ),
        )
        for (covers in listOf(false, null)) {
            assertNull(
                "covers=$covers",
                acknowledgedReplicaBaseUtil(true, acknowledged, acknowledged, covers),
            )
        }
        assertNull(
            "a burst applied during the capture: the text and the frontier disagree",
            acknowledgedReplicaBaseUtil(true, acknowledged, buffer, true),
        )
        assertNull(
            "a library-reload resume state carries no replica text",
            acknowledgedReplicaBaseUtil(false, acknowledged, acknowledged, true),
        )
        assertNull(acknowledgedReplicaBaseUtil(true, null, null, true))
    }
}
