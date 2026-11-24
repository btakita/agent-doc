package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#acceptedwriterollforward` beyond registration. 5a855605c5 stopped a re-registering replica
 * from publishing a stale buffer over writes the binary already accepted (an `--ack-text` strike,
 * `status set` payloads; agent-doc-bugs.md, ops.log 2026-11-24 02:17Z). Two other callers of
 * `ensureEditorText(editorText)` published the same stale buffer after a registration had
 * already rolled those writes forward:
 *
 * - the template-guard recovery re-registered a replacement replica (whose registration may
 *   answer `apply_canonical` / `merge_forward` under the op record) and then unconditionally
 *   published the editor baseline over it;
 * - the closing-document cut published the closing buffer even when the op record names it as a
 *   stale copy (a registration projection queued but not yet landed when the tab closed).
 *
 * The visible roll-forward caller is pinned in [VisibleRollForwardTest].
 */
class AcceptedWriteRollForwardCallersTest {
    private val saved = "- ### 📥 do [#docscleanup]\n\noperator prompt\n"
    private val canonical = "- ~~### 📥 do [#docscleanup]~~\n\noperator prompt\n"

    private fun hash(text: String) = contentHashUtil(text)

    @Test
    fun `template-guard recovery keeps a registration's accepted-write roll-forward`() {
        assertEquals(
            TemplateGuardRecoveryBaseline.KeepAcceptedWriteRollForward,
            templateGuardRecoveryBaselineUtil(
                editorText = saved,
                replicaText = canonical,
                acceptedWriteRollForwardTarget = canonical,
                replicaStructureExact = true,
            ),
        )
    }

    @Test
    fun `template-guard recovery publishes the editor baseline without that proof`() {
        val publish = TemplateGuardRecoveryBaseline.PublishEditorBaseline
        // No roll-forward recorded: the registration adopted (or had nothing to attribute).
        assertEquals(publish, templateGuardRecoveryBaselineUtil(saved, canonical, null, true))
        // The replica moved past the rolled-forward target: not the generation the proof named.
        assertEquals(publish, templateGuardRecoveryBaselineUtil(saved, canonical + "x", canonical, true))
        // A malformed target is exactly what the template guard exists to refuse.
        assertEquals(publish, templateGuardRecoveryBaselineUtil(saved, canonical, canonical, false))
        // Nothing to keep, or nothing to project (`#emptycanonicaldeadlock`).
        assertEquals(publish, templateGuardRecoveryBaselineUtil(saved, saved, saved, true))
        assertEquals(publish, templateGuardRecoveryBaselineUtil(saved, "", "", true))
        assertEquals(publish, templateGuardRecoveryBaselineUtil(saved, null, canonical, true))
    }

    @Test
    fun `the closing cut never publishes a stale copy of accepted writes`() {
        fun publishes(
            closing: String,
            replica: String? = canonical,
            canonicalHash: String? = hash(canonical),
            ancestors: List<String> = listOf(hash(saved)),
        ) = closingCutPublishesBufferUtil(closing, replica, canonicalHash, ancestors, ::hash)

        assertFalse("the op record names the closing buffer as the strike's pre-image", publishes(saved))
        assertTrue("operator text the canonical lacks is still the final cut", publishes(saved + "typed\n"))
        assertTrue("no record", publishes(saved, ancestors = emptyList()))
        assertTrue("record proven against another canonical", publishes(saved, canonicalHash = hash("newer")))
        assertTrue("an unrecorded replica generation", publishes(saved, replica = canonical + "relay\n"))
        assertTrue("already converged: the no-op publish stays", publishes(canonical))
        assertTrue("no replica text", publishes(saved, replica = null))
    }

    private fun source(name: String): String {
        val path = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/$name"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/$name"),
        ).first { Files.exists(it) }
        return Files.readString(path)
    }

    @Test
    fun `the callers are wired to the op-record decisions`() {
        val manager = source("CrdtReplicaManager.kt")
        val recovery = manager
            .substringAfter("private fun recoverRejectedRemoteCanonical(")
            .substringBefore("private fun scheduleTemplateGuardRecoveryRetry(")
        val decision = recovery.indexOf("templateGuardRecoveryBaselineUtil(")
        assertTrue(decision >= 0)
        assertTrue(
            "the decision precedes the editor-baseline publish",
            decision < recovery.indexOf("replacement.ensureEditorText(editorText)"),
        )
        assertTrue(recovery.contains("replacement.acceptedWriteRollForwardTarget"))

        val closing = manager
            .substringAfter("val closingText = ReadAction.compute<String, RuntimeException> { document.text }")
            .substringBefore("private fun clearClosingDocumentState(")
        val guard = closing.indexOf("closingCutPublishesBufferUtil(")
        assertTrue(guard >= 0)
        assertTrue(guard < closing.indexOf("forwarder.ensureEditorText(closingText)"))

        val finalize = manager
            .substringAfter("private fun finalizeRegistrationProjection(")
            .substringBefore("private fun retainCanonicalProjectionAfterRegistration(")
        assertTrue(
            "registration records what it rolled forward, for the callers that follow it",
            finalize.contains("forwarder.acceptedWriteRollForwardTarget = canonicalText") &&
                finalize.contains("forwarder.acceptedWriteRollForwardTarget = merged"),
        )
    }
}
