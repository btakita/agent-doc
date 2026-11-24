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
 * `#acceptedwriterollforward` (agent-doc-bugs.md, 2026-11-24 02:17Z): publishing such a buffer
 * deletes writes the binary already accepted (an `--ack-text` strike, `status set` payloads). The
 * op record is therefore the one proof that rolls canonical forward onto a differing buffer; text
 * containment stays diagnostic under `#idebufferauthority`.
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
    fun `attribution is diagnostic and never overrides the IDE buffer`() {
        val adopt = RegistrationProjection.Decision(RetainedRegistrationProjectionAction.AdoptOperatorBuffer, null)
        val proven = restarted(contains = if (attributed()) true else null)
        assertEquals(adopt, RegistrationProjection.decide(proven, EditorDerivation.Unavailable))
        assertEquals(
            adopt,
            RegistrationProjection.decide(proven) { _, _ -> """{"action":"apply_canonical","hold_reason":null}""" },
        )
        val held = RegistrationProjection.decide(restarted(contains = null), EditorDerivation.Unavailable)
        assertEquals(RetainedRegistrationProjectionAction.AdoptOperatorBuffer, held.action)
        assertEquals(null, held.holdReason)
    }

    private fun live(
        shadow: String?,
        buffer: String,
        canonicalText: String,
        bufferAttributed: Boolean? = null,
        shadowAttributed: Boolean? = null,
        cleanMerge: Boolean? = null,
    ) = RegistrationProjection.Observation(
        deferCanonicalProjectionForPendingLocal = false,
        canonicalProjectionRetained = false,
        retainedReplicaReseedPending = false,
        canonicalCoversRetainedFrontier = true,
        publishedShadow = shadow,
        bufferText = buffer,
        canonicalText = canonicalText,
        canonicalContainsOperatorEdits = null,
        cleanMergeAvailable = cleanMerge,
        bufferIsBinaryAttributedAncestor = bufferAttributed,
        shadowIsBinaryAttributedAncestor = shadowAttributed,
    )

    @Test
    fun `the op record rolls an accepted write forward onto a stale buffer`() {
        val apply = """{"action":"apply_canonical","hold_reason":null}"""
        val adopt = """{"action":"adopt_operator_buffer","hold_reason":null}"""
        val proven = live(saved, saved, canonical, bufferAttributed = true)
        assertEquals(
            RetainedRegistrationProjectionAction.ApplyCanonical,
            RegistrationProjection.decide(proven) { _, _ -> apply }.action,
        )
        // An older native generation that still adopts keeps adopting; the guard never invents a projection.
        assertEquals(
            RetainedRegistrationProjectionAction.AdoptOperatorBuffer,
            RegistrationProjection.decide(proven) { _, _ -> adopt }.action,
        )
        // Without the op record (or with an empty canonical) the readable buffer still wins.
        for (unproven in listOf(live(saved, saved, canonical), live(saved, saved, canonical, bufferAttributed = false))) {
            assertEquals(
                RetainedRegistrationProjectionAction.AdoptOperatorBuffer,
                RegistrationProjection.decide(unproven) { _, _ -> apply }.action,
            )
        }
        assertEquals(
            RetainedRegistrationProjectionAction.AdoptOperatorBuffer,
            RegistrationProjection.decide(live(saved, saved, "", bufferAttributed = true)) { _, _ -> apply }.action,
        )
        // Pending local delivery still defers.
        val pending = proven.copy(deferCanonicalProjectionForPendingLocal = true)
        assertEquals(
            RetainedRegistrationProjectionAction.DeferCanonicalProjection,
            RegistrationProjection.decide(pending) { _, _ -> """{"action":"defer_canonical_projection","hold_reason":null}""" }.action,
        )
    }

    @Test
    fun `an attributed shadow merges operator typing forward with the accepted write`() {
        val merge = """{"action":"merge_forward","hold_reason":null}"""
        val typed = saved + "- operator typed this\n"
        assertEquals(
            RetainedRegistrationProjectionAction.MergeForward,
            RegistrationProjection.decide(live(saved, typed, canonical, shadowAttributed = true, cleanMerge = true)) { _, _ -> merge }.action,
        )
        for (observation in listOf(
            live(saved, typed, canonical, shadowAttributed = true, cleanMerge = false),
            live(saved, typed, canonical, shadowAttributed = true, cleanMerge = null),
            live(saved, typed, canonical, cleanMerge = true),
            live(null, typed, canonical, shadowAttributed = true, cleanMerge = true),
        )) {
            assertEquals(
                RetainedRegistrationProjectionAction.AdoptOperatorBuffer,
                RegistrationProjection.decide(observation) { _, _ -> merge }.action,
            )
        }
    }

    @Test
    fun `the roll-forward facts come from the op record, never the text`() {
        val merged = { _: String, buffer: String, current: String -> current + buffer.removePrefix(saved) }
        fun facts(shadow: String?, buffer: String?, ancestors: List<String> = listOf(hash(saved))) =
            acceptedWriteRollForwardFactsUtil(shadow, buffer, canonical, hash(canonical), ancestors, ::hash, merged)
        assertEquals(AcceptedWriteRollForwardFacts(true, false, null), facts(saved, saved))
        assertEquals(AcceptedWriteRollForwardFacts(true, false, null), facts(null, saved))
        val typed = saved + "- operator typed this\n"
        assertEquals(AcceptedWriteRollForwardFacts(false, true, true), facts(saved, typed))
        val refused = acceptedWriteRollForwardFactsUtil(saved, typed, canonical, hash(canonical), listOf(hash(saved)), ::hash) { _, _, _ -> null }
        assertEquals(AcceptedWriteRollForwardFacts(false, true, false), refused)
        val none = AcceptedWriteRollForwardFacts(false, false, null)
        assertEquals(none, facts(saved, typed, ancestors = emptyList()))
        assertEquals(none, facts(null, typed))
        assertEquals(none, facts(saved, canonical))
        assertEquals(none, facts(saved, null))
        assertEquals(
            none,
            acceptedWriteRollForwardFactsUtil(saved, saved, canonical, hash("newer"), listOf(hash(saved)), ::hash, merged),
        )
    }

    @Test
    fun `the controller ack carries the record`() {
        val source = java.io.File("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt").readText()
        assertTrue(source.contains("data.get(\"binary_attributed_ancestor_hashes\")"))
        val manager = java.io.File("src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt").readText()
        assertTrue(manager.contains("canonicalContainsOperatorEdits = null"))
        assertTrue(manager.contains("bufferIsBinaryAttributedAncestor = rollForward.bufferAttributed"))
        assertTrue(manager.contains("shadowIsBinaryAttributedAncestor = rollForward.shadowAttributed"))
    }
}
