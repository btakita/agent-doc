package com.github.btakita.agentdoc

import java.nio.file.Paths
import java.util.concurrent.ConcurrentHashMap
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#resumefrontierlostonfailedattach` (live 2026-10-26 14:36-14:59, agent-doc-bugs.md).
 *
 * A `make install` reloaded the native library and recycled the controller. The
 * first re-registration reseeded the fresh controller from the editor's retained
 * replica, then refused at the swap check, but it had already released the
 * retained native resume state. Every later registration sent no state vector,
 * so the controller answered `canonical_covers_retained_frontier = unknown`, and
 * a buffer that still equalled its settled shadow (`shadow_hash == buffer_hash`)
 * held as `ambiguous-retained-projection` on every recovery probe for 23 minutes.
 * With the frontier still supplied, canonical (the reseeded replica plus the
 * controller's own queue-claim projection) provably covers it, and adoption is
 * the safe recovery.
 */
class ResumeFrontierRetentionTest {

    @Test
    fun `a committed registration releases exactly the retained state it consumed`() {
        val states = ConcurrentHashMap<String, String>()
        states["doc.md"] = "retained"
        releaseRetainedResumeStateOnCommitUtil(states, "doc.md", false, "retained")
        assertNull(states["doc.md"])

        // A newer capture that landed while this registration ran survives it.
        states["doc.md"] = "newer"
        releaseRetainedResumeStateOnCommitUtil(states, "doc.md", false, "retained")
        assertEquals("newer", states["doc.md"])

        // A canonical bootstrap supersedes any retained frontier for the path.
        releaseRetainedResumeStateOnCommitUtil(states, "doc.md", true, null)
        assertNull(states["doc.md"])
    }

    @Test
    fun `the retained frontier is released only at the commit points of forwarderFor`() {
        val source = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt",
        ).toFile().readText()
        assertFalse(
            "no registration path may drop the retained frontier before it commits",
            source.contains("nativeReloadResumeStates.remove("),
        )
        val forwarderFor = source
            .substringAfter("    private fun forwarderFor(")
            .substringBefore("    private fun cleanRegistrationMerge(")
        val firstRelease = forwarderFor.indexOf("return commitRegistration(forwarder)")
        assertTrue("forwarderFor commits through commitRegistration", firstRelease > 0)
        // Every refusal before the swap returns without releasing it.
        for (refusal in listOf(
            "refusing ambiguous retained projection for",
            "registration swap refused for",
            "recordRegisterFailure(filePath, \"replacement-promotion\")",
        )) {
            val at = forwarderFor.indexOf(refusal)
            assertTrue("missing refusal marker: $refusal", at > 0)
        }
        assertEquals(
            2,
            Regex("""return commitRegistration\(forwarder\)""").findAll(forwarderFor).count(),
        )
        // The release helper is only invoked inside the commit lambda.
        assertEquals(
            1,
            Regex("""releaseRetainedResumeStateOnCommitUtil\(""").findAll(forwarderFor).count(),
        )
    }
}
