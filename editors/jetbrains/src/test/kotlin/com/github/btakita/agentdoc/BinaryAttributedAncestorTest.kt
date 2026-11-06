package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#attachauthorattribution` — a 2026-11-05 IDE restart.
 *
 * While no replica was attached, agent-doc projected queue markers into the canonical itself (`📥`
 * lifecycle icons on two run rows and three subagent claim markers). The restarted editor's buffer
 * was the saved generation before those writes, and the IDE had discarded its settled shadow, so
 * the attach held `ambiguous-retained-projection` with nothing the operator wrote at stake. The
 * controller's op record now names the canonical generations only its own writes lead from; a
 * buffer that is one of them adopts canonical, and any other buffer keeps the hold.
 */
class BinaryAttributedAncestorTest {

    private val saved = "- https://example.test/pull/14\n  - #adversarial-pr\n- ### 📥 do [#implement]\n"
    private val canonical =
        "- https://example.test/pull/14\n  - 📥 #adversarial-pr\n- ### ☑️ [#subagent:impl#3] do [#implement]\n"

    private fun hash(text: String) = contentHashUtil(text)

    private fun attributed(
        buffer: String? = saved,
        canonicalText: String? = canonical,
        canonicalHash: String? = hash(canonical),
        ancestors: List<String> = listOf(hash(saved)),
    ) = bufferIsBinaryAttributedAncestorUtil(buffer, canonicalText, canonicalHash, ancestors, ::hash)

    @Test
    fun `a buffer the controller attributes to its own writes is attributed`() {
        assertTrue(attributed())
    }

    @Test
    fun `an operator edit the controller never recorded is unattributed`() {
        // The same marker differences plus one operator line: its hash is not in the record.
        assertFalse(attributed(buffer = saved + "- operator typed this after the save\n"))
        // Text that merely looks like the marker projection proves nothing without the record.
        assertFalse(attributed(ancestors = emptyList()))
        assertFalse(attributed(ancestors = listOf(hash("some other generation\n"))))
    }

    @Test
    fun `the record proves nothing about a canonical it was not proven against`() {
        assertFalse(attributed(canonicalHash = hash("a newer canonical\n")))
        assertFalse(attributed(canonicalHash = null))
        assertFalse(attributed(canonicalText = null))
        assertFalse(attributed(buffer = null))
        // Convergence needs no attribution.
        assertFalse(attributed(buffer = canonical, ancestors = listOf(hash(canonical))))
    }

    private fun restarted(contains: Boolean?) =
        RegistrationProjection.Observation(
            deferCanonicalProjectionForPendingLocal = false,
            canonicalProjectionRetained = true,
            retainedReplicaReseedPending = false,
            canonicalCoversRetainedFrontier = null,
            publishedShadow = null,
            bufferText = saved,
            canonicalText = canonical,
            canonicalContainsOperatorEdits = contains,
            cleanMergeAvailable = null,
            diskText = saved,
            attachHeldForOperator = true,
        )

    @Test
    fun `an attributed restart adopts canonical and an unattributed one still holds`() {
        // The plugin folds attribution into the containment fact, so every rule set that answers
        // (the frozen floor and its execution guards, an older native generation, the current
        // native rules) adopts canonical for an attributed buffer.
        val adopt = RegistrationProjection.Decision(RetainedRegistrationProjectionAction.ApplyCanonical, null)
        val proven = restarted(contains = if (attributed()) true else null)
        assertEquals(adopt, RegistrationProjection.decide(proven, EditorDerivation.Unavailable))
        assertEquals(
            adopt,
            RegistrationProjection.decide(proven) { _, _ -> """{"action":"apply_canonical","hold_reason":null}""" },
        )
        val held = RegistrationProjection.decide(restarted(contains = null), EditorDerivation.Unavailable)
        assertEquals(RetainedRegistrationProjectionAction.HoldOperatorBuffer, held.action)
        assertEquals("ambiguous-retained-projection", held.holdReason)
    }

    @Test
    fun `the controller ack carries the record`() {
        val source = java.io.File("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt").readText()
        assertTrue(source.contains("data.get(\"binary_attributed_ancestor_hashes\")"))
        val manager = java.io.File("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt").readText()
        assertTrue(manager.contains("canonicalContainsOperatorEdits = attributedOrProvenContainment"))
    }
}
