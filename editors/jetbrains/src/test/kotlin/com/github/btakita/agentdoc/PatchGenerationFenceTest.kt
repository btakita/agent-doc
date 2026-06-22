package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test
import java.io.File

/**
 * Apply-side generation fence coverage (#late-ipc-patch-plugin-apply-fence).
 *
 * A queued file reposition patch carries a generation token — `cycle_id` and
 * `baseline_hash` (SHA-256 of the live doc it targeted). A LATE applier must
 * drop a superseded patch (cycle already committed, or live doc moved on from
 * the baseline) instead of re-applying it and re-materializing a duplicate
 * `### Re:` block. The fence must fail OPEN when no token is present so a
 * legitimate patch is never dropped.
 */
class PatchGenerationFenceTest {

    private fun patchWith(
        file: String,
        cycleId: String?,
        baselineHash: String?,
        baselineNormalizedHash: String? = null,
    ): IpcPatch =
        IpcPatch(
            file = file,
            patches = emptyList(),
            unmatched = "",
            frontmatter = null,
            fullContent = null,
            repositionBoundary = true,
            cycleId = cycleId,
            baselineHash = baselineHash,
            baselineNormalizedHash = baselineNormalizedHash,
        )

    @Test
    fun `parses cycle_id and baseline hash tokens`() {
        val json =
            """{"type":"patch","file":"/tmp/plan.md","patches":[],"reposition_boundary":true,
               "cycle_id":"cycle-123","baseline_hash":"deadbeef","baseline_normalized_hash":"facefeed",
               "node_patches":[{"component":"queue","node_key":"queue:0:beta:0","op":"strike",
                 "expected_content":"- do [#beta]\n","expected_content_hash":"cafebabe"}]}"""
        val patch = parsePatchJson(json)
        assertNotNull(patch)
        assertEquals("cycle-123", patch!!.cycleId)
        assertEquals("deadbeef", patch.baselineHash)
        assertEquals("facefeed", patch.baselineNormalizedHash)
        assertEquals("- do [#beta]\n", patch.nodePatches.single().expectedContent)
        assertEquals("cafebabe", patch.nodePatches.single().expectedContentHash)
    }

    @Test
    fun `tokens are null when absent`() {
        val json = """{"type":"patch","file":"/tmp/plan.md","patches":[],"reposition_boundary":true}"""
        val patch = parsePatchJson(json)
        assertNotNull(patch)
        assertNull(patch!!.cycleId)
        assertNull(patch.baselineHash)
        assertNull(patch.baselineNormalizedHash)
    }

    @Test
    fun `no token fails open (not superseded)`() {
        val patch = patchWith("/tmp/plan.md", cycleId = null, baselineHash = null)
        assertFalse(PatchWatcher.isPatchGenerationSuperseded(patch, "anything"))
    }

    @Test
    fun `matching baseline hash is not superseded`() {
        val live = "doc body the patch targeted\n"
        val patch = patchWith("/tmp/plan.md", cycleId = null, baselineHash = PatchWatcher.contentHash(live))
        assertFalse(PatchWatcher.isPatchGenerationSuperseded(patch, live))
    }

    @Test
    fun `baseline drift is superseded`() {
        val live = "doc body the patch targeted\n"
        val patch = patchWith("/tmp/plan.md", cycleId = null, baselineHash = PatchWatcher.contentHash(live))
        assertTrue(PatchWatcher.isPatchGenerationSuperseded(patch, "the doc moved on to a later cycle\n"))
    }

    @Test
    fun `normalized baseline hash tolerates transient editor markers`() {
        val baseline = """
            ---
            keep: true
            agent_doc_pipeline:
              phase: write_applied
            ---

            <!-- agent:exchange -->
            ### Re: stale queue
            Body
            <!-- /agent:exchange -->
        """.trimIndent()
        val live = """
            ---
            keep: true
            agent_doc_pipeline:
              phase: write_applied
            ---

            <!-- agent:boundary:abc123 -->
            <!-- agent:exchange -->
            ### Re: stale queue (HEAD)
            Body <!-- no-pending-capture -->
            <!-- /agent:exchange -->
        """.trimIndent() + "\n"
        val patch = patchWith(
            "/tmp/plan.md",
            cycleId = null,
            baselineHash = PatchWatcher.contentHash(baseline),
            baselineNormalizedHash = PatchWatcher.generationFenceContentHash(baseline),
        )
        assertFalse(PatchWatcher.isPatchGenerationSuperseded(patch, live))
    }

    @Test
    fun `normalized baseline hash rejects real live queue drift`() {
        val baseline = """
            <!-- agent:queue -->
            go [#old]
            <!-- /agent:queue -->
        """.trimIndent()
        val live = """
            <!-- agent:queue -->
            go [#new]
            <!-- /agent:queue -->
        """.trimIndent()
        val patch = patchWith(
            "/tmp/plan.md",
            cycleId = null,
            baselineHash = PatchWatcher.contentHash(baseline),
            baselineNormalizedHash = PatchWatcher.generationFenceContentHash(baseline),
        )
        assertTrue(PatchWatcher.isPatchGenerationSuperseded(patch, live))
    }

    @Test
    fun `content hash mirrors sha256 hex`() {
        // SHA-256 of "" is the well-known empty-string digest — locks the mirror of debounce::content_hash.
        assertEquals(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            PatchWatcher.contentHash(""),
        )
    }

    @Test
    fun `cycle already committed is superseded`() {
        val root = File.createTempFile("adoc-fence", "").let {
            it.delete(); it.mkdirs(); it
        }
        try {
            val doc = File(root, "plan.md")
            doc.writeText("body\n")
            val cyclesDir = File(root, ".agent-doc/state/cycles")
            cyclesDir.mkdirs()
            val stateFile = File(cyclesDir, "${PatchWatcher.docHash(doc.path)}.json")
            stateFile.writeText("""{"cycle_id":"cycle-committed","file":"${doc.path}","phase":"committed"}""")

            assertTrue(PatchWatcher.cycleAlreadyCommitted(doc.path, "cycle-committed"))
            // A different cycle id, or a non-committed phase, must NOT fence.
            assertFalse(PatchWatcher.cycleAlreadyCommitted(doc.path, "other-cycle"))

            val patch = patchWith(doc.path, cycleId = "cycle-committed", baselineHash = null)
            assertTrue(PatchWatcher.isPatchGenerationSuperseded(patch, doc.readText()))
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun `open cycle is not superseded`() {
        val root = File.createTempFile("adoc-fence-open", "").let {
            it.delete(); it.mkdirs(); it
        }
        try {
            val doc = File(root, "plan.md")
            doc.writeText("body\n")
            val cyclesDir = File(root, ".agent-doc/state/cycles")
            cyclesDir.mkdirs()
            val stateFile = File(cyclesDir, "${PatchWatcher.docHash(doc.path)}.json")
            stateFile.writeText("""{"cycle_id":"cycle-open","file":"${doc.path}","phase":"write_applied"}""")

            assertFalse(PatchWatcher.cycleAlreadyCommitted(doc.path, "cycle-open"))
        } finally {
            root.deleteRecursively()
        }
    }
}
