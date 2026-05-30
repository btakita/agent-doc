package com.github.btakita.agentdoc

import org.junit.Assert.*
import org.junit.Test

/**
 * Tests for the full-document IPC corruption diagnostic instrumentation
 * (#ipcfullprompt-recur). The diagnostic must surface a pre/post user-region
 * change at every editor-visible whole-buffer mutation so a real corruption
 * packet is reconstructable from idea.log.
 */
class DocumentMutationDiagnosticTest {

    private val withBoundary = """
<!-- agent:exchange patch=append -->
### Re: prior — opus-4-8
Answered.
<!-- agent:boundary:abc12345 -->
do the next thing
<!-- /agent:exchange -->
""".trimStart()

    @Test
    fun `post-boundary region is the live prompt after the last boundary`() {
        assertEquals("do the next thing", postBoundaryExchangeRegionUtil(withBoundary))
    }

    @Test
    fun `no boundary yields whole exchange body as region`() {
        val doc = """
<!-- agent:exchange patch=append -->
typed prompt only
<!-- /agent:exchange -->
""".trimStart()
        assertEquals("typed prompt only", postBoundaryExchangeRegionUtil(doc))
    }

    @Test
    fun `no exchange component yields empty region`() {
        assertEquals("", postBoundaryExchangeRegionUtil("no components here"))
    }

    @Test
    fun `diagnostic flags an unchanged user region`() {
        val line = documentMutationDiagnosticUtil(
            "repositionBoundary", "doc.md", "pid1", "document_api",
            withBoundary, withBoundary, 42L, true,
        )
        assertTrue(line.contains("op=repositionBoundary"))
        assertTrue(line.contains("patch_id=pid1"))
        assertTrue(line.contains("transport=document_api"))
        assertTrue(line.contains("mod_stamp=42"))
        assertTrue(line.contains("idle=true"))
        assertTrue(line.contains("user_region_changed=false"))
    }

    @Test
    fun `diagnostic flags a duplicated live prompt region`() {
        // Simulated corruption: the live prompt appears twice after the mutation.
        val corrupted = withBoundary.replace(
            "do the next thing",
            "do the next thing\ndo the next thing",
        )
        val line = documentMutationDiagnosticUtil(
            "applyPatch.component", "doc.md", "pid2", "document_api",
            withBoundary, corrupted, 7L, false,
        )
        assertTrue("duplication must be flagged: $line", line.contains("user_region_changed=true"))
        // The post region must be longer than the pre region when text is duplicated.
        val pre = Regex("pre_user_region_len=(\\d+)").find(line)!!.groupValues[1].toInt()
        val post = Regex("post_user_region_len=(\\d+)").find(line)!!.groupValues[1].toInt()
        assertTrue("post region should grow on duplication", post > pre)
    }

    @Test
    fun `content fingerprint distinguishes different content`() {
        assertNotEquals(
            documentMutationContentHashUtil("alpha"),
            documentMutationContentHashUtil("beta"),
        )
        assertEquals(
            documentMutationContentHashUtil("same"),
            documentMutationContentHashUtil("same"),
        )
    }

    // #ipcfullprompt-recur2 — VFS whole-buffer write must re-validate disk
    // immediately before writing, the not-open-file analog of the editor
    // apply-proof. A disk change between compute and write fails closed.

    @Test
    fun `vfs guard allows write when disk is unchanged since compute`() {
        val computedFrom = withBoundary
        val diskNow = withBoundary
        assertTrue(vfsDiskContentStillCurrentUtil(computedFrom, diskNow))
    }

    @Test
    fun `vfs guard rejects write when disk changed under us`() {
        val computedFrom = withBoundary
        // The file was opened and a fresh prompt typed after the patch was computed.
        val diskNow = withBoundary.replace("do the next thing", "do the next thing now")
        assertFalse(
            "stale-disk whole-buffer write must fail closed",
            vfsDiskContentStillCurrentUtil(computedFrom, diskNow),
        )
    }
}
