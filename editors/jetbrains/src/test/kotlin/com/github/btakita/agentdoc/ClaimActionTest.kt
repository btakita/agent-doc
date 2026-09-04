package com.github.btakita.agentdoc

import java.nio.file.Paths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ClaimActionTest {
    @Test
    fun `claim saves and fences the target editor before bounded background mutation`() {
        val claimSource = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/ClaimAction.kt",
        ).toFile().readText()
        val forceSource = Paths.get(
            "src/main/kotlin/com/github/btakita/agentdoc/ForceClaimAction.kt",
        ).toFile().readText()

        val saveIdx = claimSource.indexOf("fileDocumentManager.saveDocument(document)")
        val dirtyCheckIdx = claimSource.indexOf("isDocumentUnsaved(document)")
        val viewerFenceIdx = claimSource.indexOf("it.isViewer = true")
        val pooledIdx = claimSource.indexOf("executeOnPooledThread")
        val refreshIdx = claimSource.indexOf("file.refresh(false, false)")
        val restoreIdx = claimSource.indexOf("editor.isViewer = false")

        assertTrue("Claim should save the target document", saveIdx >= 0)
        assertTrue("Claim should fail closed when the target remains dirty", dirtyCheckIdx > saveIdx)
        assertTrue("Claim should fence editor typing after saving", viewerFenceIdx > dirtyCheckIdx)
        assertTrue("Claim should leave the EDT only after acquiring the fence", pooledIdx > viewerFenceIdx)
        assertTrue("Claim should refresh the file before restoring editing", refreshIdx >= 0)
        assertTrue("Claim should restore editing after refresh", restoreIdx > refreshIdx)
        assertTrue("Claim should use the bounded command runner", claimSource.contains("runCommandWithTimeout"))
        assertFalse("Claim must not save unrelated documents", claimSource.contains("saveAllDocuments()"))
        assertTrue(
            "Force Claim should use the same document fence",
            forceSource.indexOf("ClaimDocumentFence.acquire") in 0 until forceSource.indexOf("executeOnPooledThread"),
        )
        assertTrue("Force Claim should use the bounded command runner", forceSource.contains("runCommandWithTimeout"))
    }

    @Test
    fun `parses cross-session reject marker from merged claim output`() {
        // Mirrors `agent-doc claim` stderr (claim.rs cross_session_reject_marker +
        // the human bail) merged via redirectErrorStream.
        val output = """
            [claim] cross-session-reject pane_id=%43 pane_session=5 configured=0
            Error: pane %43 is in tmux session '5' but project session is '0'; switch to the configured session or pass --force
        """.trimIndent()
        val reject = ClaimAction.parseCrossSessionReject(output)
        assertEquals("%43", reject?.paneId)
        assertEquals("5", reject?.paneSession)
        assertEquals("0", reject?.configured)
    }

    @Test
    fun `field order is not assumed`() {
        val output = "[claim] cross-session-reject configured=main pane_session=work pane_id=%7"
        val reject = ClaimAction.parseCrossSessionReject(output)
        assertEquals("%7", reject?.paneId)
        assertEquals("work", reject?.paneSession)
        assertEquals("main", reject?.configured)
    }

    @Test
    fun `returns null when no marker present`() {
        assertNull(ClaimAction.parseCrossSessionReject("Claimed plan.md"))
        assertNull(ClaimAction.parseCrossSessionReject("Error: some other failure"))
    }

    @Test
    fun `returns null when a required field is missing`() {
        // Missing `configured` — must not half-parse into a misleading dialog.
        assertNull(ClaimAction.parseCrossSessionReject("[claim] cross-session-reject pane_id=%1 pane_session=2"))
    }

    @Test
    fun `failed claim does not request layout sync`() {
        assertFalse(ClaimAction.shouldSyncLayoutAfterClaim(1))
        assertFalse(ClaimAction.shouldSyncLayoutAfterClaim(124))
    }

    @Test
    fun `successful claim requests layout sync`() {
        assertTrue(ClaimAction.shouldSyncLayoutAfterClaim(0))
    }

    @Test
    fun `new pane recovery invokes only binary owned new pane mode`() {
        assertEquals(
            listOf("agent-doc", "claim", "plan.md", "--new-pane"),
            ClaimAction.buildClaimCommand(
                "agent-doc",
                "plan.md",
                "right",
                force = false,
                newPane = true,
            ),
        )
    }

    @Test
    fun `normal claim preserves positional targeting`() {
        assertEquals(
            listOf("agent-doc", "claim", "plan.md", "--position", "right"),
            ClaimAction.buildClaimCommand(
                "agent-doc",
                "plan.md",
                "right",
                force = false,
                newPane = false,
            ),
        )
    }
}
