package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RunAgentDocAttemptLedgerTest {
    @Test
    fun `attempt ledger writes latest stage and append-only history`() {
        val cwd = Files.createTempDirectory("agent-doc-jb-route-attempt").toFile()
        val attempt = RunAgentDocAttemptLedger.begin(
            cwd = cwd.path,
            relativePath = "tasks/root.md",
            filePath = "${cwd.path}/tasks/root.md",
            focusedFile = "${cwd.path}/tasks/root.md",
        )

        attempt.recordIfCurrent("active_document_saved")
        attempt.finishIfCurrent("route_failed", error = "exit=1 output=boom")

        // `#jbedtledger`: stage writes are async so they never block the EDT.
        // Flush deterministically instead of sleeping before reading them back.
        RunAgentDocAttemptLedger.awaitPendingWrites()
        val diagnostics = RunAgentDocAttemptLedger.attemptDiagnosticsFile(cwd.path, "tasks/root.md")
        val text = diagnostics.readText()
        assertTrue(text.contains("attempt_id=${attempt.id}"))
        assertTrue(text.contains("relative_path=tasks/root.md"))
        assertTrue(text.contains("stage=route_failed"))
        assertTrue(text.contains("error=exit=1 output=boom"))
        assertTrue(text.contains("stage=clicked"))
        assertTrue(text.contains("stage=active_document_saved"))
    }

    @Test
    fun `superseded attempts cannot overwrite the newer route key snapshot`() {
        val cwd = Files.createTempDirectory("agent-doc-jb-route-supersede").toFile()
        val first = RunAgentDocAttemptLedger.begin(
            cwd = cwd.path,
            relativePath = "tasks/root.md",
            filePath = "${cwd.path}/tasks/root.md",
            focusedFile = "${cwd.path}/tasks/root.md",
        )
        val second = RunAgentDocAttemptLedger.begin(
            cwd = cwd.path,
            relativePath = "tasks/root.md",
            filePath = "${cwd.path}/tasks/root.md",
            focusedFile = "${cwd.path}/tasks/root.md",
        )

        first.finishIfCurrent("route_failed", error = "stale failure")

        // `#jbedtledger`: stage writes are async so they never block the EDT.
        // Flush deterministically instead of sleeping before reading them back.
        RunAgentDocAttemptLedger.awaitPendingWrites()
        val diagnostics = RunAgentDocAttemptLedger.attemptDiagnosticsFile(cwd.path, "tasks/root.md")
        val text = diagnostics.readText()
        assertTrue(text.contains("attempt_id=${second.id}"))
        assertTrue(text.contains("previous_attempt_id=${first.id}"))
        assertTrue(text.contains("stage=clicked"))
        assertTrue(text.contains("stage=superseded"))
        assertFalse(text.contains("stale failure"))
    }

    /**
     * `#jbedtledger`: Run Agent Doc drives five ledger stages from the EDT
     * (SubmitAction 31/49/59, TerminalUtil 367/444). Each one used to do mkdirs +
     * a full readLines + a full writeText synchronously, i.e. blocking file I/O
     * on the UI thread for every click.
     *
     * The caller path must now only capture values and enqueue: all file I/O
     * belongs to `persistStage`, which runs solely on the single ledger worker.
     */
    @Test
    fun `stage writes never touch the filesystem on the calling thread`() {
        val source = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/RunAgentDocAttemptLedger.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/RunAgentDocAttemptLedger.kt"),
        ).first { Files.exists(it) }.let { Files.readString(it) }

        // Anchor on the STAGE handoff specifically: `awaitPendingWrites` also
        // enqueues, and it appears earlier in the file.
        val captureIdx = source.indexOf("val callerThread = Thread.currentThread().name")
        assertTrue("the calling thread name must be captured for the record", captureIdx >= 0)
        val enqueueIdx = source.indexOf("ledgerWriter.execute {", captureIdx)
        assertTrue("stage writes must be handed to the ledger worker", enqueueIdx >= 0)

        val persistIdx = source.indexOf("private fun persistStage(")
        assertTrue("the I/O body must live in persistStage", persistIdx >= 0)
        for (io in listOf("mkdirs()", "readEventLines(", "writeText(")) {
            val ioIdx = source.indexOf(io)
            assertTrue("$io must exist", ioIdx >= 0)
            assertTrue(
                "$io must run inside persistStage (the worker), not on the calling thread",
                ioIdx > persistIdx,
            )
        }

        // A single worker, not a pool: the ledger is an append-ordered event log.
        assertTrue(
            "the ledger writer must be single-threaded so events cannot interleave",
            source.contains("newSingleThreadExecutor"),
        )
        // The caller's thread name is the diagnostic that reveals EDT usage, so it
        // must be captured before handing off, never read on the worker.
        assertTrue(
            "the calling thread name must be captured before the handoff",
            captureIdx < enqueueIdx,
        )
    }
}
